/* gpad-trigger - déclencheur Home, robuste aux (dé)connexions.
 *
 *   gpad-trigger [node] <binaire-overlay> [args...]
 *
 * Écoute TOUTES les manettes en evdev (lecture partagée, sans grab) et lance
 * l'overlay au premier BTN_MODE (Home). L'overlay prend alors le grab
 * exclusif sur la manette qui a servi ; gpad-trigger réarme dès sa fermeture.
 *
 * Robustesse : un fd mort (manette BT endormie, câble retiré) est détecté et
 * remplacé ; la liste des manettes est rescannée toutes les 2 s, donc une
 * reconnexion est reprise sans relancer gpad-trigger.
 *
 * Pas le daemon de production : gamepadshortcuts reste propriétaire des
 * combos. Ici on teste le cycle Home -> overlay grabé -> retour au jeu.
 */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define MAXPADS 8

static pid_t child = -1;
/* Overlay résident : lancé une fois, ouvert/fermé par le socket `ipc_path`.
 * `child` reste réservé aux overlays jetables (console, focus) et sert de
 * garde « un seul overlay à la fois » pour ceux-là. */
static pid_t resident = -1;
static char ipc_path[256];
/* Résident mort (il ne peut plus répondre) : on retentera son démarrage une
 * fois laissée passer une minute, le temps que Wayland soit prêt par ex. */
static volatile sig_atomic_t resident_dead;
static long resident_retry_at;
static volatile sig_atomic_t need_rescan;
/* Menu système demandé par l'overlay : il tient le grab, donc il est seul à
 * voir Home maintenu, et nous le signale par SIGUSR1. */
static volatile sig_atomic_t ask_console;
/* pid de CE gpad-trigger, passé en GPAD_TRIGGER_PID aux overlays : c'est lui qu'ils
 * préviennent quand Home est maintenu pendant qu'ils ont le grab. */
static char own_pid[16];

/* Inhibition de l'écran : tant qu'une manette est connectée, on empêche la
 * mise en veille / le verrouillage (même mécanisme que l'ancien
 * gamepadshortcuts : org.freedesktop.ScreenSaver.Inhibit). Le helper garde la
 * connexion D-Bus ouverte : on l'arrête en fermant son tube stdin. */
static pid_t inhibitor = -1;
static int inhibitor_pipe = -1;
static int inhibiting = 0;
static char g_inhibit[512] = "../inhibit/pad-inhibit";

/* Options utilisateur (Gablue Manager) : ~/.config/gablue/gpad-options,
 * une ligne `cle=valeur` ; clé absente = on. SIGHUP relit (le helper
 * `gablue-pad` l'envoie après écriture). */
static int opt_menu = 1;      /* Home ouvre le panneau / le menu système */
static int opt_shortcuts = 1; /* combos Home+bouton et Home+D-pad */
static int opt_inhibit = 1;   /* pas de veille tant qu'une manette est là */
static volatile sig_atomic_t reload_opt;

/* Gate BPF : socket du démon (service système) + nodes js/hidraw des manettes
 * courantes, passés à l'overlay en `--gate` / `--gate-target`. */
static char gate_sock[256];
static char gate_nodes[8][64];
static int n_gate;
static char g_padnodes[512] = "../gate/padnodes.sh";

/* L'overlay du panneau latéral demande le menu système (Home maintenu). */
static void on_sigusr1(int s)
{
    (void)s;
    ask_console = 1;
}

static void on_chld(int s)
{
    (void)s;
    int st;
    if (child > 0 && waitpid(child, &st, WNOHANG) == child) {
        fprintf(stderr, "[gpad-trigger] overlay terminé (status %d)\n", st);
        child = -1;
    }
    if (resident > 0 && waitpid(resident, &st, WNOHANG) == resident) {
        fprintf(stderr,
                "[gpad-trigger] overlay resident terminé (status %d) : "
                "ouverture par exec\n", st);
        resident = -1;
        resident_dead = 1;
    }
    if (inhibitor > 0 && waitpid(inhibitor, &st, WNOHANG) == inhibitor) {
        inhibitor = -1;
        inhibiting = 0;
    }
}

/* Ouvre l'inhibition (manette connectée). */
static void inhibit_start(void)
{
    int p[2];
    if (pipe(p) < 0)
        return;
    /* CLOEXEC obligatoire : sinon l'overlay (et les scripts) héritent du tube
     * et le helper ne voit JAMAIS l'EOF -> inhibition collée à vie. */
    fcntl(p[0], F_SETFD, FD_CLOEXEC);
    fcntl(p[1], F_SETFD, FD_CLOEXEC);
    pid_t pid = fork();
    if (pid == 0) {
        close(p[1]);
        dup2(p[0], STDIN_FILENO);
        close(p[0]);
        execl(g_inhibit, g_inhibit, (char *)NULL);
        _exit(127);
    }
    close(p[0]);
    inhibitor = pid;
    inhibitor_pipe = p[1];
    inhibiting = 1;
    fprintf(stderr, "[gpad-trigger] manette connectee -> ecran non verrouille\n");
    fflush(stderr);
}

/* Ferme l'inhibition : le helper voit EOF, relâche le cookie et sort. */
static void inhibit_stop(void)
{
    if (inhibitor_pipe >= 0) {
        close(inhibitor_pipe);
        inhibitor_pipe = -1;
    }
    inhibiting = 0;
    fprintf(stderr, "[gpad-trigger] plus de manette -> verrouillage rendu\n");
    fflush(stderr);
}

static void maj_inhibition(int nf)
{
    if (!opt_inhibit) {
        if (inhibiting)
            inhibit_stop();
        return;
    }
    if (nf > 0 && !inhibiting)
        inhibit_start();
    else if (nf == 0 && inhibiting)
        inhibit_stop();
}

static void on_hup(int s)
{
    (void)s;
    need_rescan = 1;
    reload_opt = 1;
}

/* Relit ~/.config/gablue/gpad-options. Fichier absent = tout à on (défaut de
 * l'image) ; une valeur inconnue est ignorée, jamais « off » par accident. */
static void lire_options(void)
{
    const char *home = getenv("HOME");
    if (!home)
        return;
    char p[400];
    snprintf(p, sizeof p, "%s/.config/gablue/gpad-options", home);
    FILE *f = fopen(p, "r");
    if (!f)
        return;
    int menu = 1, shortcuts = 1, inhibit = 1;
    char ligne[128];
    while (fgets(ligne, sizeof ligne, f)) {
        char *eq = strchr(ligne, '=');
        if (!eq)
            continue;
        *eq = 0;
        char *v = eq + 1;
        v[strcspn(v, " \t\r\n")] = 0;
        int on = strcmp(v, "off") != 0;
        if (!strcmp(ligne, "menu"))
            menu = on;
        else if (!strcmp(ligne, "shortcuts"))
            shortcuts = on;
        else if (!strcmp(ligne, "inhibit"))
            inhibit = on;
    }
    fclose(f);
    if (menu != opt_menu || shortcuts != opt_shortcuts || inhibit != opt_inhibit) {
        fprintf(stderr, "[gpad-trigger] options: menu=%s shortcuts=%s inhibit=%s\n",
                menu ? "on" : "off", shortcuts ? "on" : "off", inhibit ? "on" : "off");
        fflush(stderr);
    }
    opt_menu = menu;
    opt_shortcuts = shortcuts;
    opt_inhibit = inhibit;
}

static int is_pad_name(const char *n)
{
    if (strstr(n, "touchpad") || strstr(n, "touch pad") || strstr(n, "sensors") ||
        strstr(n, "motion"))
        return 0;
    return strstr(n, "wireless controller") || strstr(n, "dualsense") ||
           strstr(n, "dualshock") || strstr(n, "gamepad") || strstr(n, "xbox") ||
           strstr(n, "controller");
}

/* Liste des nodes evdev des manettes présentes (aucune dépendance à udev). */
static int scan_pads(char out[][64], int max)
{
    DIR *d = opendir("/sys/class/input");
    if (!d)
        return 0;
    int n = 0;
    struct dirent *de;
    while ((de = readdir(d)) && n < max) {
        if (strncmp(de->d_name, "input", 5) != 0)
            continue;
        char p[320], name[128] = {0};
        snprintf(p, sizeof p, "/sys/class/input/%s/name", de->d_name);
        FILE *f = fopen(p, "r");
        if (!f)
            continue;
        if (!fgets(name, sizeof name, f)) {
            fclose(f);
            continue;
        }
        fclose(f);
        for (char *c = name; *c; c++)
            *c = (char)((*c >= 'A' && *c <= 'Z') ? *c + 32 : *c);
        if (!is_pad_name(name))
            continue;

        char dirp[320];
        snprintf(dirp, sizeof dirp, "/sys/class/input/%s", de->d_name);
        DIR *d2 = opendir(dirp);
        if (!d2)
            continue;
        struct dirent *e2;
        while ((e2 = readdir(d2))) {
            if (strncmp(e2->d_name, "event", 5) == 0 && n < max) {
                snprintf(out[n], 64, "/dev/input/%s", e2->d_name);
                n++;
            }
        }
        closedir(d2);
    }
    closedir(d);
    return n;
}

static void launch_vue(char **argv, int argc, int first_free, const char *node,
                       const char *vue);

/* Ajoute `--gate <socket>` et `--gate-target <node>` (un par node js/hidraw) :
 * sans démon, l'overlay reste utilisable, il n'isole simplement pas la manette. */
static void args_gate(char **args, int *n)
{
    if (!gate_sock[0])
        return;
    args[(*n)++] = "--gate";
    args[(*n)++] = gate_sock;
    for (int i = 0; i < n_gate; i++) {
        args[(*n)++] = "--gate-target";
        args[(*n)++] = gate_nodes[i];
    }
}

static void launch(char **argv, int argc, int first_free, const char *node)
{
    launch_vue(argv, argc, first_free, node, NULL);
}

/* Lance l'overlay en vue « menu système » (panneau centré). */
static void launch_console(char **argv, int argc, int first_free, const char *node)
{
    child = fork();
    if (child < 0) {
        perror("fork");
        return;
    }
    if (child == 0) {
        char **args = calloc((size_t)(argc - first_free) + 8 + 2 * (size_t)n_gate,
                             sizeof *args);
        int n = 0;
        for (int i = first_free; i < argc; i++)
            args[n++] = argv[i];
        args_gate(args, &n);
        args[n++] = "--console";
        args[n++] = "--grab";
        args[n++] = (char *)node;
        args[n] = NULL;
        execv(args[0], args);
        perror("execv overlay (menu systeme)");
        _exit(127);
    }
    fprintf(stderr, "[gpad-trigger] HOME maintenu -> menu systeme pid %d\n", child);
    fflush(stderr);
}

static void launch_vue(char **argv, int argc, int first_free, const char *node,
                       const char *vue)
{
    child = fork();
    if (child < 0) {
        perror("fork");
        return;
    }
    if (child == 0) {
        char **args = calloc((size_t)(argc - first_free) + 8 + 2 * (size_t)n_gate,
                             sizeof *args);
        int n = 0;
        for (int i = first_free; i < argc; i++)
            args[n++] = argv[i];
        args_gate(args, &n);
        /* l'overlay tient le grab : lui seul voit Home. Il nous signale un
         * maintien par SIGUSR1 (voir on_sigusr1). */
        setenv("GPAD_TRIGGER_PID", own_pid, 1);
        if (vue) {
            args[n++] = "--focus";
            args[n++] = (char *)vue;
        }
        args[n++] = "--grab";
        args[n++] = (char *)node;
        args[n] = NULL;
        execv(args[0], args);
        perror("execv overlay");
        _exit(127);
    }
    fprintf(stderr, "[gpad-trigger] HOME%s sur %s -> overlay pid %d\n",
            vue ? (strcmp(vue, "fps") == 0 ? "+bas (ligne FPS)" : "+haut (ligne MANGOHUD)") : "",
            node, child);
    fflush(stderr);
}

/* Clavier virtuel sans résident : un overlay dédié qui quitte à la fermeture
 * (sans cela chaque appui laisserait un processus et une surface). */
static void launch_clavier(char **argv, int argc, int first_free, const char *node)
{
    child = fork();
    if (child < 0) {
        perror("fork");
        return;
    }
    if (child == 0) {
        char **args = calloc((size_t)(argc - first_free) + 8 + 2 * (size_t)n_gate,
                             sizeof *args);
        int n = 0;
        for (int i = first_free; i < argc; i++)
            args[n++] = argv[i];
        args_gate(args, &n);
        setenv("GPAD_TRIGGER_PID", own_pid, 1);
        args[n++] = "--clavier";
        args[n++] = (char *)node;
        args[n] = NULL;
        execv(args[0], args);
        perror("execv overlay (clavier)");
        _exit(127);
    }
    fprintf(stderr, "[gpad-trigger] Home+carre -> clavier virtuel pid %d\n", child);
    fflush(stderr);
}

/* --- overlay résident (ouverture instantanée) -----------------------------
 * Le panneau est lancé UNE fois : sa surface reste mappée, transparente et
 * hors écran, donc KWin ne rejoue pas son animation d'ouverture à chaque Home
 * et il n'y a plus d'exec à payer. Le déclencheur envoie `open <node>
 * <cibles…>` sur un socket Unix ; l'overlay fait le reste.
 *
 * Socket par déclencheur (`gpad-<pid>.sock`) : le banc peut tourner à côté du
 * service sans lui voler ses commandes. */
static void ipc_path_init(void)
{
    const char *rt = getenv("XDG_RUNTIME_DIR");
    char base[200];
    if (rt && *rt)
        snprintf(base, sizeof base, "%s/gablue", rt);
    else
        snprintf(base, sizeof base, "/tmp/gablue-%d", (int)getuid());
    mkdir(base, 0700);
    snprintf(ipc_path, sizeof ipc_path, "%s/gpad-%d.sock", base, (int)getpid());
}

static void resident_start(char **argv, int argc, int first_free)
{
    ipc_path_init();
    unlink(ipc_path);
    pid_t pid = fork();
    if (pid < 0) {
        perror("fork overlay resident");
        return;
    }
    if (pid == 0) {
        /* Le résident ne doit pas survivre au déclencheur : sinon chaque
         * redémarrage du service (Restart=always) laisserait un gpad fantôme
         * avec sa surface Wayland et son socket. */
        prctl(PR_SET_PDEATHSIG, SIGTERM);
        char **args = calloc((size_t)(argc - first_free) + 8, sizeof *args);
        int n = 0;
        for (int i = first_free; i < argc; i++)
            args[n++] = argv[i];
        if (gate_sock[0]) {
            args[n++] = "--gate";
            args[n++] = gate_sock;
        }
        args[n++] = "--resident";
        args[n++] = "--ipc";
        args[n++] = ipc_path;
        args[n] = NULL;
        setenv("GPAD_TRIGGER_PID", own_pid, 1);
        execv(args[0], args);
        perror("execv overlay resident");
        _exit(127);
    }
    resident = pid;
    /* le socket est créé après wayland + premier configure : on attend son
     * apparition (10 ms de pas, 3 s de plafond) pour que le premier Home soit
     * déjà instantané */
    for (int i = 0; i < 300; i++) {
        if (access(ipc_path, F_OK) == 0) {
            fprintf(stderr,
                    "[gpad-trigger] overlay resident pret (pid %d, ipc %s)\n",
                    pid, ipc_path);
            fflush(stderr);
            return;
        }
        struct timespec ts = {0, 10 * 1000 * 1000};
        nanosleep(&ts, NULL);
    }
    fprintf(stderr,
            "[gpad-trigger] overlay resident muet (pid %d) : ouverture par exec\n",
            pid);
    fflush(stderr);
}

/* Envoie une ligne de commande au résident ; 0 = pas de résident joignable
 * (l'appelant retombe sur l'exec classique). */
static int overlay_send(const char *ligne)
{
    if (resident <= 0 || !ipc_path[0] || !ligne || !*ligne)
        return 0;
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return 0;
    struct sockaddr_un sa = {0};
    sa.sun_family = AF_UNIX;
    if (strlen(ipc_path) >= sizeof sa.sun_path) {
        close(fd);
        return 0;
    }
    memcpy(sa.sun_path, ipc_path, strlen(ipc_path) + 1);
    if (connect(fd, (struct sockaddr *)&sa, sizeof sa) < 0) {
        close(fd);
        return 0;
    }
    size_t len = strlen(ligne);
    ssize_t w = write(fd, ligne, len);
    close(fd);
    return w == (ssize_t)len;
}

/* Le panneau est-il ouvert ? `state` au résident : « open » ou « closed ».
 * Pas de résident joignable = fermé (repli exec, rien à fermer). */
static int overlay_est_ouvert(void)
{
    if (resident <= 0 || !ipc_path[0])
        return 0;
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return 0;
    struct sockaddr_un sa = {0};
    sa.sun_family = AF_UNIX;
    if (strlen(ipc_path) >= sizeof sa.sun_path) {
        close(fd);
        return 0;
    }
    memcpy(sa.sun_path, ipc_path, strlen(ipc_path) + 1);
    if (connect(fd, (struct sockaddr *)&sa, sizeof sa) < 0) {
        close(fd);
        return 0;
    }
    struct timeval tv = {0, 200 * 1000};
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    static const char q[] = "state\n";
    int ouvert = 0;
    if (write(fd, q, sizeof q - 1) == (ssize_t)(sizeof q - 1)) {
        char buf[32] = {0};
        ssize_t n = read(fd, buf, sizeof buf - 1);
        ouvert = n > 0 && strncmp(buf, "open", 4) == 0;
    }
    close(fd);
    return ouvert;
}

/* Home+combo pendant que le panneau est ouvert : le fermer d'abord, sinon il
 * se filme dans la capture/le clip. Le résident répond `closed` une fois la
 * surface retirée ; plafond 1,2 s (un panneau qui ne répond pas ne bloque pas
 * le combo indéfiniment). */
static void panneau_ferme(void)
{
    if (!overlay_est_ouvert())
        return;
    overlay_send("close\n");
    for (int i = 0; i < 120; i++) {
        if (!overlay_est_ouvert())
            return;
        struct timespec ts = {0, 10 * 1000 * 1000};
        nanosleep(&ts, NULL);
    }
}

/* Arbitrage avec l'overlay (même jeton côté Rust) : quand le panneau est
 * ouvert, il tient le grab et voit le combo le premier ; il le relâche, et
 * nous relisons les MÊMES appuis — sans jeton, l'action partait deux fois
 * (double bip au démarrage du replay, bascule d'enregistrement annulée dans
 * la seconde, constaté le 21/11/2026). Premier à poser le jeton = seul à agir ;
 * au-delà de 400 ms c'est un nouvel appui, le jeton est repris. */
static int combo_jeton(void)
{
    const char *rt = getenv("XDG_RUNTIME_DIR");
    char p[400];
    snprintf(p, sizeof p, "%s/gablue/gpad-combo",
             rt && *rt ? rt : "/run/user/1000");
    for (int essai = 0; essai < 2; essai++) {
        int fd = open(p, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
        if (fd >= 0) {
            dprintf(fd, "%d\n", (int)getpid());
            close(fd);
            return 1;
        }
        if (errno != EEXIST)
            return 1;
        struct stat st;
        if (stat(p, &st) == 0) {
            struct timespec n;
            clock_gettime(CLOCK_REALTIME, &n);
            long age = (n.tv_sec - st.st_mtim.tv_sec) * 1000L
                       + (n.tv_nsec - st.st_mtim.tv_nsec) / 1000000L;
            if (age >= 0 && age < 400)
                return 0;
        }
        unlink(p);
    }
    return 1;
}

static int overlay_open_remote(const char *node)
{
    if (!node || !*node)
        return 0;
    char msg[512];
    int len = snprintf(msg, sizeof msg, "open %s", node);
    for (int i = 0; i < n_gate && len > 0 && len < (int)sizeof msg - 70; i++)
        len += snprintf(msg + len, sizeof msg - (size_t)len, " %s", gate_nodes[i]);
    if (len < 0 || len >= (int)sizeof msg - 2)
        len = (int)strlen(msg);
    msg[len++] = '\n';
    msg[len] = 0;
    return overlay_send(msg);
}

/* Clavier virtuel : c'est le MÊME résident qui le montre, avec le grab qu'il
 * tient déjà — aucun transfert de manette, aucune animation de fenêtre. */
static int overlay_kbd_remote(const char *node)
{
    if (!node || !*node)
        return 0;
    char msg[512];
    snprintf(msg, sizeof msg, "kbd %s\n", node);
    return overlay_send(msg);
}

/* --- combos Home + D-pad ---------------------------------------------------
 * Home seul ouvre le panneau au relâchement ; Home maintenu = menu système ;
 * Home+gauche = bascule de l'enregistrement ; Home+droite = capture d'écran ;
 * Home+haut = replay (démarre, ou sauvegarde s'il tourne) ; Home+bas = arrête
 * le replay, sinon ouvre le panneau sur la ligne FPS ; Home+L3 = rien chez
 * nous (Home part au jeu, aucun panneau). */
/* Home tenu = menu système (comme une vraie console) ; appui court = panneau
 * latéral, ouvert au relâchement. Aucune fenêtre de grâce : puisqu'un maintien
 * n'ouvre plus rien de suite, un combo n'a plus de course avec le panneau. */
#define HOME_LONG_MS 1200

static char g_recorder[512] = "../recorder.sh";
static char g_screenshot[512] = "../screenshot.sh";
static char g_scripts[512] = "../scripts";
static char g_mouse[512] = "../mouse/gpad-mouse";
static char g_replay[512] = "../replay.sh";
static char home_node[64];
static int home_down;
static int combo_done;
static int console_done;
/* L3 (BTN_THUMBL) tenu : Home+L3 = « Home pour le jeu », aucun panneau. */
static int l3_down;
static struct timespec home_at;

static long ms_since(const struct timespec *t)
{
    struct timespec n;
    clock_gettime(CLOCK_MONOTONIC, &n);
    return (n.tv_sec - t->tv_sec) * 1000L + (n.tv_nsec - t->tv_nsec) / 1000000L;
}

static void run_cmd(const char *cmd)
{
    pid_t p = fork();
    if (p == 0) {
        execl("/bin/sh", "sh", "-c", cmd, (char *)NULL);
        _exit(127);
    }
    if (p > 0) {
        int st;
        /* recorder/screenshot rendent la main tout de suite : son détaché,
         * capture ~40 ms (gpad-shot), repli spectacle seulement en échec */
        waitpid(p, &st, 0);
    }
}

/* État de l'enregistreur, lu sur SA socket (aucun fork, ~1 ms) : 1 si le champ
 * JSON `cle` vaut true. Démon absent = 0.
 *
 * Pourquoi pas `replay.sh status | grep actif` : « inactif » contient
 * « actif », donc le test était toujours vrai — Home+gauche sauvegardait au
 * lieu d'enregistrer. */
static int rec_etat(const char *cle)
{
    const char *sock = getenv("GPAD_REC_SOCK");
    const char *rt = getenv("XDG_RUNTIME_DIR");
    char chemin[400];
    if (sock && *sock)
        snprintf(chemin, sizeof chemin, "%s", sock);
    else
        snprintf(chemin, sizeof chemin, "%s/gablue/gpad-rec.sock",
                 rt && *rt ? rt : "/run/user/1000");
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return 0;
    struct sockaddr_un sa = {0};
    sa.sun_family = AF_UNIX;
    if (strlen(chemin) >= sizeof sa.sun_path) {
        close(fd);
        return 0;
    }
    memcpy(sa.sun_path, chemin, strlen(chemin) + 1);
    if (connect(fd, (struct sockaddr *)&sa, sizeof sa) < 0) {
        close(fd);
        return 0;
    }
    const char *q = "{\"cmd\":\"status\"}\n";
    if (write(fd, q, strlen(q)) != (ssize_t)strlen(q)) {
        close(fd);
        return 0;
    }
    char buf[768] = {0};
    ssize_t n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n <= 0)
        return 0;
    char motif[64];
    snprintf(motif, sizeof motif, "\"%s\":true", cle);
    return strstr(buf, motif) != NULL;
}

/* Home+gauche : l'enregistrement, une seule chose. Le buffer de replay est
 * indépendant (un seul pipeline encode, l'anneau TS et le mp4 écrivent côte à
 * côte) : il continue de tourner si on enregistre. Détaché, parce que le bip
 * de départ prend ~0,7 s et que le déclencheur doit rester à l'écoute. */
static void do_record(void)
{
    panneau_ferme();
    char cmd[700];
    fprintf(stderr, "[gpad-trigger] COMBO Home+gauche -> enregistrement (bascule)\n");
    fflush(stderr);
    snprintf(cmd, sizeof cmd,
             "setsid env PAD_NODE='%s' '%s' toggle > /dev/null 2>&1 < /dev/null &",
             home_node, g_recorder);
    run_cmd(cmd);
}

/* Home+L2 / Home+R2 : bascule jeu ⇄ application compagne, et cyclage de
 * TOUTES les fenêtres quand aucun jeu ne tourne (bureau). La décision est prise
 * par l'overlay résident : il tient déjà la liste des fenêtres, le canal KWin
 * et la mise en sourdine du jeu. Sans résident on le démarre — il reste ensuite
 * vivant pour la session, donc la bascule suivante ne paie rien.
 *
 * `sens` : -1 = L2 (vers le jeu / fenêtre précédente), +1 = R2 (vers l'app /
 * fenêtre suivante). */
static void do_bascule(char **argv, int argc, int first_free, int sens)
{
    if (resident <= 0) {
        fprintf(stderr, "[gpad-trigger] COMBO Home+L2/R2 : demarrage du resident\n");
        fflush(stderr);
        resident_start(argv, argc, first_free);
    }
    char cmd[64];
    snprintf(cmd, sizeof cmd, "bascule %d\n", sens);
    if (overlay_send(cmd)) {
        fprintf(stderr, "[gpad-trigger] COMBO Home+%s -> bascule envoyee\n",
                sens > 0 ? "R2" : "L2");
    } else {
        fprintf(stderr, "[gpad-trigger] COMBO Home+%s -> resident injoignable\n",
                sens > 0 ? "R2" : "L2");
    }
    fflush(stderr);
}

static void do_shot(void)
{
    panneau_ferme();
    fprintf(stderr, "[gpad-trigger] COMBO Home+droite -> capture\n");
    fflush(stderr);
    char cmd[600];
    snprintf(cmd, sizeof cmd, "'%s'", g_screenshot);
    run_cmd(cmd);
}

/* Home+haut : « je veux un clip ». Buffer arrêté → on le démarre ; buffer qui
 * tourne → on sauvegarde les N dernières secondes. Le script prend ~1 s (bip +
 * pause + démarrage) : laissé en arrière-plan pour que gpad-trigger reste à
 * l'écoute. */
static void do_replay_haut(void)
{
    panneau_ferme();
    int actif = rec_etat("replay");
    char cmd[800];
    fprintf(stderr, "[gpad-trigger] COMBO Home+haut -> replay (%s)\n",
            actif ? "sauvegarde" : "demarrage");
    fflush(stderr);
    snprintf(cmd, sizeof cmd,
             "setsid env PAD_NODE='%s' '%s' %s > /dev/null 2>&1 < /dev/null &",
             home_node, g_replay, actif ? "save" : "start");
    run_cmd(cmd);
}

/* Home+bas : arrête le buffer s'il tourne (1), sinon rien (0 : l'appelant
 * ouvre le panneau sur la ligne FPS, geste d'origine — aucune fonction perdue). */
static int do_replay_bas(void)
{
    if (!rec_etat("replay"))
        return 0;
    char cmd[700];
    fprintf(stderr, "[gpad-trigger] COMBO Home+bas -> arret du replay\n");
    fflush(stderr);
    snprintf(cmd, sizeof cmd,
             "setsid env PAD_NODE='%s' '%s' stop > /dev/null 2>&1 < /dev/null &",
             home_node, g_replay);
    run_cmd(cmd);
    return 1;
}

/* Raccourcis de l'ancien gamepadshortcuts, mêmes gestes : Home+SELECT =
 * killthemall (tuer jeux Windows/émulateurs), Home+START = openes
 * (EmulationStation). Home+triangle (launchyt, VacuumTube) est **retiré** :
 * YouTube s'ouvre déjà par la page Applications, et le combo partait trop
 * souvent en visant Home (demandé). */
static void do_script(const char *geste, const char *fichier)
{
    /* `killthemall` est partagé (KillEmu.desktop) : l'image le livre dans le
     * dossier de scripts commun et le service l'exporte en KILLTHEMALL. Au banc,
     * il vit dans SCRIPTS avec le reste. */
    const char *abs = !strcmp(fichier, "killthemall") ? getenv("KILLTHEMALL") : NULL;
    char cmd[700];
    if (abs && *abs) {
        fprintf(stderr, "[gpad-trigger] COMBO Home+%s -> %s\n", geste, abs);
        fflush(stderr);
        snprintf(cmd, sizeof cmd, "'%s'", abs);
    } else {
        fprintf(stderr, "[gpad-trigger] COMBO Home+%s -> %s\n", geste, fichier);
        fflush(stderr);
        snprintf(cmd, sizeof cmd, "'%s/%s'", g_scripts, fichier);
    }
    run_cmd(cmd);
}

/* Home+R3 : mode souris (s'il ne tourne pas déjà — gpad-mouse sort tout seul
 * sur Home+R3, c'est aussi la façon de le désactiver). La gate et ses cibles
 * (nodes js/hidraw) sont passées au binaire : le mode souris coupe la manette
 * pour les autres applications comme le fait l'overlay (grab evdev + gate). */
static void do_mouse(void)
{
    if (system("pgrep -x gpad-mouse > /dev/null 2>&1") == 0) {
        fprintf(stderr, "[gpad-trigger] COMBO Home+R3 -> mode souris deja actif\n");
        fflush(stderr);
        return;
    }
    char cmd[1400];
    if (!ipc_path[0])
        ipc_path_init();
    int len = snprintf(cmd, sizeof cmd, "setsid '%s'", g_mouse);
    /* socket du résident : gpad-mouse y demande le panneau quand on quitte au
     * Home (le mode souris grabbe la manette, nous ne voyons plus Home) */
    if (len > 0 && len < (int)sizeof cmd - 90)
        len += snprintf(cmd + len, sizeof cmd - (size_t)len, " --ipc '%s'", ipc_path);
    if (len < 0 || len >= (int)sizeof cmd - 2)
        len = (int)strlen(cmd);
    if (gate_sock[0]) {
        len += snprintf(cmd + len, sizeof cmd - (size_t)len, " --gate '%s'", gate_sock);
        for (int i = 0; i < n_gate && len > 0 && len < (int)sizeof cmd - 90; i++)
            len += snprintf(cmd + len, sizeof cmd - (size_t)len,
                            " --gate-target '%s'", gate_nodes[i]);
    }
    if (len < 0 || len >= (int)sizeof cmd - 45)
        len = (int)strlen(cmd);
    snprintf(cmd + len, sizeof cmd - (size_t)len, " > /dev/null 2>&1 < /dev/null &");
    fprintf(stderr, "[gpad-trigger] COMBO Home+R3 -> mode souris\n");
    fflush(stderr);
    run_cmd(cmd);
}

/* Home+carré : clavier virtuel. Le résident le montre (même grab, aucune
 * animation), sinon un overlay dédié. */
static void do_pad_kbd(char **argv, int argc, int first_free, const char *node)
{
    fprintf(stderr, "[gpad-trigger] COMBO Home+carre -> clavier virtuel\n");
    fflush(stderr);
    if (overlay_kbd_remote(node))
        return;
    launch_clavier(argv, argc, first_free, node);
}

static void init_recorder_path(void)
{
    char exe[512];
    ssize_t n = readlink("/proc/self/exe", exe, sizeof exe - 1);
    if (n <= 0)
        return;
    exe[n] = 0;
    char *slash = strrchr(exe, '/');
    if (slash) {
        *slash = 0;
        snprintf(g_recorder, sizeof g_recorder, "%s/../recorder.sh", exe);
        snprintf(g_screenshot, sizeof g_screenshot, "%s/../screenshot.sh", exe);
        snprintf(g_scripts, sizeof g_scripts, "%s/../scripts", exe);
        snprintf(g_inhibit, sizeof g_inhibit, "%s/../inhibit/pad-inhibit", exe);
        snprintf(g_mouse, sizeof g_mouse, "%s/../mouse/gpad-mouse", exe);
        snprintf(g_replay, sizeof g_replay, "%s/../replay.sh", exe);
        snprintf(g_padnodes, sizeof g_padnodes, "%s/../gate/padnodes.sh", exe);
    }
}

/* Surcharges d'installation : le service utilisateur exporte des chemins
 * absolus (/usr/bin, /usr/libexec/gablue-pad) ; le banc garde les chemins
 * relatifs au binaire. */
static void chemins_env(void)
{
    const char *v;
    if ((v = getenv("RECORDER")) && *v)   snprintf(g_recorder, sizeof g_recorder, "%s", v);
    if ((v = getenv("SCREENSHOT")) && *v) snprintf(g_screenshot, sizeof g_screenshot, "%s", v);
    if ((v = getenv("SCRIPTS")) && *v)    snprintf(g_scripts, sizeof g_scripts, "%s", v);
    if ((v = getenv("INHIBIT")) && *v)    snprintf(g_inhibit, sizeof g_inhibit, "%s", v);
    if ((v = getenv("MOUSE")) && *v)      snprintf(g_mouse, sizeof g_mouse, "%s", v);
    if ((v = getenv("REPLAY")) && *v)     snprintf(g_replay, sizeof g_replay, "%s", v);
    if ((v = getenv("PADNODES")) && *v)   snprintf(g_padnodes, sizeof g_padnodes, "%s", v);
    if ((v = getenv("GPAD_GATE")) && *v) {
        snprintf(gate_sock, sizeof gate_sock, "%s", v);
    } else if (access("/run/gablue-pad/gate.sock", W_OK) == 0) {
        snprintf(gate_sock, sizeof gate_sock, "/run/gablue-pad/gate.sock");
    }
}

/* Inventaire des nodes js/hidraw des manettes (padnodes.sh, même logique que le
 * banc : une manette = un périphérique physique, plusieurs nodes). Recalculé
 * seulement quand la liste des manettes change — jamais à chaque appui. */
static void inventaire_nodes(void)
{
    n_gate = 0;
    if (!gate_sock[0])
        return;
    FILE *f = popen(g_padnodes, "r");
    if (!f)
        return;
    char ligne[256];
    while (fgets(ligne, sizeof ligne, f) && n_gate < 8) {
        if (strncmp(ligne, "NODE ", 5) != 0)
            continue;
        ligne[strcspn(ligne, "\r\n")] = 0;
        snprintf(gate_nodes[n_gate], sizeof gate_nodes[n_gate], "%.*s",
                 (int)sizeof gate_nodes[n_gate] - 1, ligne + 5);
        n_gate++;
    }
    pclose(f);
}

/* Socket Wayland réellement vivant.
 *
 * La session console écrit `WAYLAND_DISPLAY=gablue-gaming` dans le gestionnaire
 * utilisateur PARTAGÉ (ses unités doivent trouver son compositeur). En revenant
 * au bureau, un service relancé avant que Plasma ne réécrive cet environnement
 * hérite du socket mort : l'overlay ne peut plus se mapper et chaque Home fait
 * planter `kscreen-doctor` (« Failed to create wl_display », dialogue d'erreur)
 * — constaté le 24/11/2026. On teste donc la connexion ; si elle échoue, on
 * adopte un `wayland-*` du dossier runtime qui accepte, et on l'exporte (les
 * enfants — overlay, kscreen-doctor, captures — héritent du bon socket).
 */
static int wayland_repond(const char *runtime, const char *nom)
{
    if (!nom || !*nom)
        return 0;
    char chemin[512];
    snprintf(chemin, sizeof chemin, "%s/%s", runtime, nom);
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return 0;
    struct sockaddr_un addr = {0};
    addr.sun_family = AF_UNIX;
    snprintf(addr.sun_path, sizeof addr.sun_path, "%s", chemin);
    int ok = connect(fd, (struct sockaddr *)&addr, sizeof addr) == 0;
    close(fd);
    return ok;
}

static void adopter_wayland(void)
{
    const char *runtime = getenv("XDG_RUNTIME_DIR");
    const char *actuel = getenv("WAYLAND_DISPLAY");
    if (!runtime || !*runtime || wayland_repond(runtime, actuel))
        return;

    DIR *d = opendir(runtime);
    if (!d)
        return;
    char choisi[128] = "";
    struct timespec meilleur = {0, 0};
    struct dirent *e;
    while ((e = readdir(d))) {
        size_t n = strlen(e->d_name);
        if (strncmp(e->d_name, "wayland-", 8) != 0)
            continue;
        if (n > 5 && strcmp(e->d_name + n - 5, ".lock") == 0)
            continue;
        char chemin[512];
        snprintf(chemin, sizeof chemin, "%s/%s", runtime, e->d_name);
        struct stat st;
        if (stat(chemin, &st) < 0 || !S_ISSOCK(st.st_mode))
            continue;
        /* le plus RÉCENT parmi ceux qui répondent : en cas de deux
         * compositeurs vivants (session qui se ferme), le nouveau est le bon */
        if (!wayland_repond(runtime, e->d_name))
            continue;
        if (st.st_mtim.tv_sec > meilleur.tv_sec ||
            (st.st_mtim.tv_sec == meilleur.tv_sec &&
             st.st_mtim.tv_nsec > meilleur.tv_nsec)) {
            meilleur = st.st_mtim;
            snprintf(choisi, sizeof choisi, "%s", e->d_name);
        }
    }
    closedir(d);
    if (!choisi[0])
        return;
    fprintf(stderr, "[gpad-trigger] WAYLAND_DISPLAY=%s mort, adopté %s\n",
            (actuel && *actuel) ? actuel : "(vide)", choisi);
    fflush(stderr);
    setenv("WAYLAND_DISPLAY", choisi, 1);
}

int main(int argc, char **argv)
{
    snprintf(own_pid, sizeof own_pid, "%d", (int)getpid());

    /* avant tout lancement d'enfant : l'environnement est hérité à l'exec */
    adopter_wayland();

    if (argc < 2) {
        fprintf(stderr, "usage: %s [node] <binaire-overlay> [args...]\n", argv[0]);
        return 2;
    }

    int first_free;
    const char *pinned = NULL;
    if (strncmp(argv[1], "/dev/", 5) == 0) { /* node explicite (banc de test) */
        pinned = argv[1];
        first_free = 2;
    } else {
        first_free = 1;
    }
    if (first_free >= argc) {
        fprintf(stderr, "usage: %s [node] <binaire-overlay> [args...]\n", argv[0]);
        return 2;
    }
    init_recorder_path();
    chemins_env();
    lire_options();
    inventaire_nodes();
    /* Home doit être instantané : le panneau démarre une fois, maintenant. */
    if (opt_menu)
        resident_start(argv, argc, first_free);
    printf("OVERLAY=%s\nRECORDER=%s\nSCREENSHOT=%s\nGATE=%s\n", argv[first_free],
           g_recorder, g_screenshot, gate_sock[0] ? gate_sock : "(aucun)");
    fflush(stdout);

    struct sigaction sa = {0};
    sa.sa_handler = on_sigusr1;
    sigaction(SIGUSR1, &sa, NULL);
    sa.sa_handler = on_chld;
    sa.sa_flags = SA_RESTART;
    sigaction(SIGCHLD, &sa, NULL);
    sa.sa_handler = on_hup;
    sigaction(SIGHUP, &sa, NULL); /* force un rescan (tests) */
    signal(SIGPIPE, SIG_IGN);

    int fds[MAXPADS];
    char paths[MAXPADS][64];
    int nf = 0;
    time_t last_scan = 0;
    int announced = 0;

    for (;;) {
        /* Un résident mort (Wayland pas encore prêt au démarrage du service,
         * par ex.) laisserait tous les Home suivants en exec : on retente,
         * une fois par minute, plutôt que de marteler — un échec bloque le
         * déclencheur jusqu'à 3 s (attente du socket). */
        if (resident_dead && resident <= 0) {
            struct timespec nm;
            clock_gettime(CLOCK_MONOTONIC, &nm);
            long nowm = (long)nm.tv_sec * 1000L + nm.tv_nsec / 1000000L;
            if (resident_retry_at == 0) {
                resident_retry_at = nowm + 60000L;
            } else if (nowm >= resident_retry_at) {
                resident_retry_at = 0;
                resident_dead = 0;
                fprintf(stderr, "[gpad-trigger] relance de l'overlay resident\n");
                fflush(stderr);
                resident_start(argv, argc, first_free);
            }
        }

        /* (re)scan périodique : nouvelles manettes, reconnexions */
        time_t now = time(NULL);
        if (nf == 0 || need_rescan || now - last_scan >= 2) {
            char found[MAXPADS][64];
            int n = pinned ? 1 : scan_pads(found, MAXPADS);
            if (pinned)
                snprintf(found[0], sizeof found[0], "%s", pinned);
            last_scan = now;
            need_rescan = 0;

            /* set identique ? on ne touche à rien */
            int same = (n == nf);
            for (int i = 0; same && i < n; i++)
                if (strcmp(found[i], paths[i]) != 0)
                    same = 0;
            if (!same) {
                for (int i = 0; i < nf; i++)
                    close(fds[i]);
                nf = 0;
                for (int i = 0; i < n; i++) {
                    int fd = open(found[i], O_RDONLY | O_CLOEXEC | O_NONBLOCK);
                    if (fd < 0)
                        continue;
                    fds[nf] = fd;
                    snprintf(paths[nf], sizeof paths[nf], "%s", found[i]);
                    nf++;
                }
                if (nf > 0 && !announced) {
                    announced = 1;
                    fprintf(stderr, "[gpad-trigger] manettes: ");
                    for (int i = 0; i < nf; i++)
                        fprintf(stderr, "%s%s", i ? " " : "", paths[i]);
                    fprintf(stderr, "\n[gpad-trigger] en attente de Home\n");
                } else if (nf == 0) {
                    announced = 0;
                    fprintf(stderr, "[gpad-trigger] aucune manette, attente...\n");
                } else {
                    fprintf(stderr, "[gpad-trigger] liste mise à jour (%d manette(s))\n", nf);
                }
                inventaire_nodes();
                fflush(stderr);
            }
        }

        if (nf == 0) {
            struct timespec ts = {0, 200 * 1000 * 1000};
            nanosleep(&ts, NULL);
            continue;
        }

        if (reload_opt) {
            reload_opt = 0;
            lire_options();
        }
        maj_inhibition(nf);

        struct pollfd pf[MAXPADS];
        for (int i = 0; i < nf; i++) {
            pf[i].fd = fds[i];
            pf[i].events = POLLIN;
            pf[i].revents = 0;
        }
        int r = poll(pf, (nfds_t)nf, nf == 0 ? 1000 : 50);
        /* `r == 0` (délai expiré) et `r < 0` (EINTR : SIGCHLD, SIGUSR1) doivent
         * descendre au bas de la boucle : c'est là que vivent le maintien de
         * Home et la demande de menu système venue de l'overlay. L'ancien
         * `if (r <= 0) continue;` les sautait — sans événement manette, le
         * maintien n'était jamais vu (17/11/2026). */
        if (r < 0 && errno != EINTR)
            continue;

        int dead = 0;
        for (int i = 0; i < nf; i++) {
            if (pf[i].revents == 0)
                continue;
            if (pf[i].revents & (POLLERR | POLLHUP | POLLNVAL)) {
                dead = 1;
                continue;
            }
            struct input_event ev;
            ssize_t got;
            while ((got = read(fds[i], &ev, sizeof ev)) == (ssize_t)sizeof ev) {
                if (ev.type == EV_KEY && ev.code == BTN_MODE) {
                    if (ev.value == 1) {
                        home_down = 1;
                        /* L3 déjà tenu = Home+L3 : le geste « Home pour le
                         * jeu ». Aucun panneau au relâchement, aucun menu
                         * système au maintien — mais Home et L3 partent
                         * quand même au jeu, la lecture n'étant pas grabée. */
                        combo_done = (opt_shortcuts && l3_down) ? 1 : 0;
                        console_done = 0;
                        clock_gettime(CLOCK_MONOTONIC, &home_at);
                        snprintf(home_node, sizeof home_node, "%s", paths[i]);
                        if (combo_done) {
                            fprintf(stderr,
                                    "[gpad-trigger] COMBO Home+L3 -> Home"
                                    " laisse au jeu (pas de panneau)\n");
                            fflush(stderr);
                        }
                    } else {
                        home_down = 0;
                        /* relâché sans combo : on ouvre tout de suite. Le
                         * résident répond en ~1 ms (aucun exec, aucune
                         * animation KWin) ; sans lui, l'exec classique. */
                        if (opt_menu && !combo_done && child < 0) {
                            if (!overlay_open_remote(home_node))
                                launch(argv, argc, first_free, home_node);
                            combo_done = 1;
                        }
                    }
                } else if (ev.type == EV_KEY && ev.code == BTN_THUMBL) {
                    /* Home+L3 : le jeu a déjà reçu Home (lecture sans grab) et
                     * en fait ce qu'il veut — overlay Steam, menu du jeu. Nous
                     * ne faisons rien, et le combo_done posé ici empêche le
                     * panneau de s'ouvrir au relâchement comme le menu système
                     * de s'ouvrir au maintien. C'est le geste des jeux dont le
                     * menu Home se superpose au nôtre. */
                    l3_down = ev.value != 0;
                    if (opt_shortcuts && ev.value == 1 && home_down &&
                        !combo_done && child < 0) {
                        combo_done = 1;
                        fprintf(stderr,
                                "[gpad-trigger] COMBO Home+L3 -> Home laisse"
                                " au jeu (pas de panneau)\n");
                        fflush(stderr);
                    }
                } else if (ev.type == EV_KEY && ev.value == 1
                           && (ev.code == BTN_SELECT || ev.code == BTN_START
                               || ev.code == BTN_WEST
                               || ev.code == BTN_THUMBR)) {
                    if (opt_shortcuts && home_down && !combo_done && child < 0) {
                        combo_done = 1;
                        if (ev.code == BTN_SELECT)
                            do_script("SELECT", "killthemall");
                        else if (ev.code == BTN_START)
                            do_script("START", "openes");
                        else if (ev.code == BTN_WEST)
                            do_pad_kbd(argv, argc, first_free, home_node);
                        else
                            do_mouse();
                    }
                } else if (ev.type == EV_KEY && ev.value == 1
                           && (ev.code == BTN_TL2 || ev.code == BTN_TR2)) {
                    /* Home+gâchette : bascule jeu ⇄ app hors panneau, et
                     * cyclage des fenêtres au bureau. Les gâchettes sont
                     * analogiques (ABS_Z/ABS_RZ) mais le noyau émet aussi le
                     * bouton numérique au seuil : c'est lui qu'on lit, sans
                     * dépendre d'un seuil à notre main. */
                    if (opt_shortcuts && home_down && !combo_done && child < 0) {
                        combo_done = 1;
                        if (!combo_jeton()) {
                            fprintf(stderr,
                                    "[gpad-trigger] combo L2/R2 ignore"
                                    " (deja traite par le panneau)\n");
                            fflush(stderr);
                        } else {
                            do_bascule(argv, argc, first_free,
                                       ev.code == BTN_TR2 ? 1 : -1);
                        }
                    }
                } else if (ev.type == EV_ABS && ev.code == ABS_HAT0X && ev.value != 0) {
                    if (opt_shortcuts && home_down && !combo_done && child < 0) {
                        combo_done = 1;
                        if (!combo_jeton()) {
                            fprintf(stderr,
                                    "[gpad-trigger] combo gauche/droite ignore"
                                    " (deja traite par le panneau)\n");
                            fflush(stderr);
                        } else if (ev.value < 0) {
                            do_record();
                        } else {
                            do_shot();
                        }
                    }
                } else if (ev.type == EV_ABS && ev.code == ABS_HAT0Y && ev.value != 0) {
                    /* Home+haut = replay (démarre, ou sauvegarde s'il tourne) ;
                     * Home+bas = arrête le buffer — et, s'il ne tourne pas,
                     * ouvre le panneau sur la ligne FPS (geste d'origine). */
                    if (opt_shortcuts && home_down && !combo_done && child < 0) {
                        combo_done = 1;
                        if (!combo_jeton()) {
                            fprintf(stderr,
                                    "[gpad-trigger] combo haut/bas ignore"
                                    " (deja traite par le panneau)\n");
                            fflush(stderr);
                        } else if (ev.value < 0) {
                            do_replay_haut();
                        } else {
                            panneau_ferme();
                            if (!do_replay_bas()) {
                                fprintf(stderr,
                                        "[gpad-trigger] COMBO Home+bas -> ligne fps\n");
                                fflush(stderr);
                                launch_vue(argv, argc, first_free, home_node, "fps");
                            }
                        }
                    }
                }
            }
            if (got < 0 && errno != EAGAIN && errno != EWOULDBLOCK)
                dead = 1;
        }
        /* Demande venue de l'overlay (il tenait le grab et a vu Home
         * maintenu) : il se ferme de son côté, on attend sa sortie — le pad
         * doit être rendu — puis on ouvre le menu système. */
        if (ask_console) {
            ask_console = 0;
            if (!console_done) {
                console_done = 1;
                combo_done = 1;
                fprintf(stderr, "[gpad-trigger] demande de menu systeme (overlay)\n");
                fflush(stderr);
                if (child > 0) {
                    pid_t c = child;
                    kill(c, SIGTERM);
                    int st;
                    while (waitpid(c, &st, 0) < 0 && errno == EINTR) {
                    }
                    child = -1;
                }
                launch_console(argv, argc, first_free, home_node);
            }
        }

        /* Home maintenu : menu système — et RIEN d'autre. Le panneau latéral
         * n'est ouvert qu'au RELÂCHEMENT d'un appui court : sinon il
         * apparaîtrait puis disparaîtrait quand la console prend sa place
         * (constaté à l'usage). Si un panneau latéral est déjà ouvert (Home
         * appuyé une seconde fois), on le ferme proprement AVANT de prendre sa
         * place : une seule instance peut grabber la manette, sinon EBUSY. */
        if (opt_menu && home_down && !combo_done && !console_done &&
            ms_since(&home_at) > HOME_LONG_MS) {
            console_done = 1;
            combo_done = 1;
            fprintf(stderr, "[gpad-trigger] HOME maintenu -> menu systeme\n");
            fflush(stderr);
            if (child > 0) {
                pid_t c = child;
                kill(c, SIGTERM);
                int st;
                while (waitpid(c, &st, 0) < 0 && errno == EINTR) {
                }
                child = -1;
            }
            launch_console(argv, argc, first_free, home_node);
        }

        if (dead) { /* manette partie : on rescannera */
            /* si c'est la manette qui a ouvert le panneau, on enlève le panneau
             * (sinon il resterait à l'écran, sans manette pour le piloter) */
            if (child > 0) {
                for (int i = 0; i < nf; i++) {
                    if (pf[i].revents & (POLLERR | POLLHUP | POLLNVAL)) {
                        if (strcmp(paths[i], home_node) == 0) {
                            /* L'overlay gère SIGTERM en fermeture PROPRE
                             * (animation + `close` au gate). On l'arrête donc
                             * tout de suite : un panneau sans manette ne sert
                             * à rien, et le laisser tourner laisserait les
                             * manettes coupées. */
                            fprintf(stderr,
                                    "[gpad-trigger] manette du panneau partie -> on enleve le panneau\n");
                            fflush(stderr);
                            kill(child, SIGTERM);
                        }
                    }
                }
            }
            for (int i = 0; i < nf; i++)
                close(fds[i]);
            nf = 0;
            announced = 0;
            last_scan = 0;
        }
    }
}
