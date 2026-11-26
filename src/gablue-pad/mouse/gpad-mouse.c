// gpad-mouse : emulation souris/clavier via manette (evdev + uinput)
// Repris de src/gamepadshortcuts/mouse.c (banc gablue-pad).
// Remplace l'ancien mouse.py (python-uinput casse sur Python 3.14, distutils supprime)
//
// Fonctionnement (remaniement du 26/11/2026) :
//   - Stick gauche       : deplacement du pointeur (courbe progressive style FPS)
//   - Stick droit        : molette, proportionnelle a la deflection
//                          (Y = verticale, X = horizontale)
//   - Croix / Rond / R3  : clic gauche / clic droit / clic milieu
//   - Carre / Triangle   : Retour arriere / Espace
//   - D-pad              : fleches du clavier
//   - L3                 : Echap
//   - L1 / R1            : Alt gauche / Super gauche (a tenir)
//   - L2 / R2            : Ctrl gauche / Shift gauche (a tenir)
//   - Select / Start     : Tab / Entree
//   - Home + R3          : quitter (gere aussi par gamepadshortcuts)
//
// Le clic est sur Croix, sous le pouce DROIT : le pointeur ne peut donc pas
// etre le stick droit, sinon le pouce qui vise devrait lacher pour cliquer.
// Pointeur = stick gauche, et le stick droit se libere pour la molette. Le
// glisser-deposer se fait a deux mains (Croix tenue + stick gauche).
//
// Le binaire est lance/arrete par gpad-trigger (Home + R3).
//
// PENDANT LE MODE, LA MANETTE N'APPARTIENT QU'AU POINTEUR : le noeud evdev est
// grabbe (EVIOCGRAB) et les nodes js/hidraw sont coupes par le gate BPF du
// pack — les memes deux verrous que l'overlay quand son panneau est ouvert.
// Sans ca, le jeu continuait de recevoir stick et boutons : deplacer le
// pointeur faisait aussi marcher le personnage (signale le 24/11/2026).

#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <linux/uinput.h>
#include <math.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

// =============================================================================
// CONFIGURATION (reprise de mouse.py)
// =============================================================================

#define DEAD_ZONE 0.08
#define BASE_SENSITIVITY 2.0
#define MAX_SENSITIVITY 12.0
#define UPDATE_RATE 120

// Molette au stick droit : crans par seconde, proportionnels a la deflection
// (1 cran/s au ras de la zone morte, 22 a fond). Pas de courbe quadratique :
// sur une molette elle donnerait un depart mou puis incontrolable.
#define MOLETTE_MIN 1.0
#define MOLETTE_MAX 22.0

// Sons : surchargeables par environnement, sinon ceux du banc (importés).
#define START_SOUND_DEFAUT "clic.wav"
#define EXIT_SOUND_DEFAUT  "noclic.wav"

// =============================================================================
// SORTIE PROPRE ET GATE
// =============================================================================

static volatile sig_atomic_t running = 1;

/* Socket IPC du resident (`--ipc`) : sert a lui demander d'ouvrir le panneau
 * quand on quitte. Vide = lance sans resident (banc, ou declencheur sans
 * panneau) : on sort alors simplement, le prochain Home ouvrira le menu. */
static char ipc_socket[300] = "";
static void on_signal(int sig) { (void)sig; running = 0; }

// Nodes js/hidraw des manettes (donnes par le declencheur : `padnodes.sh`).
static const char *gate_targets[8];
static int n_gate_targets;
static const char *gate_socket;
static bool gate_ouvert;

/* Une commande par connexion, aucune reponse (protocole du demon) : `target`
 * resout le node en dev_t et l'ajoute, `open` attache le hook. On n'envoie
 * JAMAIS `clear` (il viderait les cibles de l'overlay) : le demon est
 * idempotent, plusieurs `target` s'accumulent. */
static bool gate_cmd(const char *sock, const char *message)
{
    if (!sock || !*sock)
        return false;

    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return false;

    struct sockaddr_un addr = {0};
    addr.sun_family = AF_UNIX;
    snprintf(addr.sun_path, sizeof(addr.sun_path), "%s", sock);

    // connexion bornee a 500 ms : un demon bloque ne fige pas le mode souris
    int flags = fcntl(fd, F_GETFL, 0);
    fcntl(fd, F_SETFL, flags | O_NONBLOCK);
    bool ok = false;
    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) == 0) {
        ok = true;
    } else if (errno == EINPROGRESS) {
        struct pollfd p = {.fd = fd, .events = POLLOUT};
        ok = poll(&p, 1, 500) > 0;
    }
    if (ok) {
        char ligne[256];
        int n = snprintf(ligne, sizeof(ligne), "%s\n", message);
        ok = n > 0 && write(fd, ligne, (size_t)n) == n;
    }
    close(fd);
    return ok;
}

static void gate_fermer(void)
{
    if (gate_ouvert) {
        gate_cmd(gate_socket, "close");
        gate_ouvert = false;
    }
}

// =============================================================================
// DETECTION DE LA MANETTE
// =============================================================================

static const char *SKIP_KEYWORDS[] = {
    "Touchpad", "Motion Sensors", "Headset", "Accelerometer", "Gyroscope"
};

static const char *GAMEPAD_KEYWORDS[] = {
    "Sony", "DualSense", "DualShock", "Nintendo", "Xbox",
    "Microsoft", "Wireless Controller", "Gamepad"
};

static bool str_contains(const char *haystack, const char *needle)
{
    return strcasestr(haystack, needle) != NULL;
}

static bool test_bit(int bit, const unsigned long *array)
{
    return (array[bit / (8 * sizeof(unsigned long))] &
            (1UL << (bit % (8 * sizeof(unsigned long))))) != 0;
}

// Verifie si le device expose un code abs donne
static bool has_abs(int fd, int code)
{
    struct input_absinfo info;
    return ioctl(fd, EVIOCGABS(code), &info) == 0;
}

/* Un axe lu a l'ouverture : presence, valeur courante (repos), plage. */
struct axe {
    bool present;
    int value;
    int minimum;
    int maximum;
};

static void lire_axe(int fd, int code, struct axe *a)
{
    struct input_absinfo info;
    a->present = ioctl(fd, EVIOCGABS(code), &info) == 0;
    if (a->present) {
        a->value = info.value;
        a->minimum = info.minimum;
        a->maximum = info.maximum;
    }
}

// Cherche une manette valide parmi /dev/input/event*
// Retourne le chemin du device (buffer statique) ou NULL
static const char *find_controller(void)
{
    static char path[64];
    char name[256];
    unsigned long key_bits[(KEY_MAX + 8 * sizeof(unsigned long)) /
                           (8 * sizeof(unsigned long))] = {0};

    for (int i = 0; i < 64; i++) {
        snprintf(path, sizeof(path), "/dev/input/event%d", i);

        int fd = open(path, O_RDONLY | O_NONBLOCK);
        if (fd < 0)
            continue;

        if (ioctl(fd, EVIOCGNAME(sizeof(name)), name) < 0) {
            close(fd);
            continue;
        }
        name[sizeof(name) - 1] = '\0';

        fprintf(stderr, "[INFO] Device: %s - %s\n", path, name);

        // Ignorer les sous-devices (touchpad, gyro, casque...)
        bool skip = false;
        for (size_t k = 0; k < sizeof(SKIP_KEYWORDS) / sizeof(SKIP_KEYWORDS[0]); k++) {
            if (str_contains(name, SKIP_KEYWORDS[k])) {
                skip = true;
                break;
            }
        }
        if (skip) {
            close(fd);
            continue;
        }

        // Manettes connues : exiger les axes du stick droit
        bool known = false;
        for (size_t k = 0; k < sizeof(GAMEPAD_KEYWORDS) / sizeof(GAMEPAD_KEYWORDS[0]); k++) {
            if (str_contains(name, GAMEPAD_KEYWORDS[k])) {
                known = true;
                break;
            }
        }
        if (known) {
            if (has_abs(fd, ABS_RX) && has_abs(fd, ABS_RY)) {
                fprintf(stderr, "[INFO] Trouve : %s - %s\n", path, name);
                close(fd);
                return path;
            }
            // Nom connu mais pas de stick droit : on ignore ce device
            close(fd);
            continue;
        }

        // Fallback : boutons de manette generiques (BTN_SOUTH == BTN_A == BTN_GAMEPAD)
        if (ioctl(fd, EVIOCGBIT(EV_KEY, sizeof(key_bits)), key_bits) >= 0 &&
            test_bit(BTN_GAMEPAD, key_bits)) {
            fprintf(stderr, "[INFO] Trouve (capacites) : %s - %s\n", path, name);
            close(fd);
            return path;
        }

        close(fd);
    }

    fprintf(stderr, "[ERREUR] Aucune manette valide trouvee (Sony, Nintendo, Xbox).\n");
    return NULL;
}

// =============================================================================
// PERIPHERIQUE UINPUT VIRTUEL
// =============================================================================

static int create_virtual_device(void)
{
    int fd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);
    if (fd < 0) {
        perror("open /dev/uinput");
        return -1;
    }

    ioctl(fd, UI_SET_EVBIT, EV_KEY);
    ioctl(fd, UI_SET_EVBIT, EV_REL);
    ioctl(fd, UI_SET_EVBIT, EV_SYN);

    // Boutons souris
    ioctl(fd, UI_SET_KEYBIT, BTN_LEFT);
    ioctl(fd, UI_SET_KEYBIT, BTN_RIGHT);
    ioctl(fd, UI_SET_KEYBIT, BTN_MIDDLE);

    // Touches clavier (meme liste que la table `buttons` ci-dessous)
    ioctl(fd, UI_SET_KEYBIT, KEY_LEFT);
    ioctl(fd, UI_SET_KEYBIT, KEY_RIGHT);
    ioctl(fd, UI_SET_KEYBIT, KEY_UP);
    ioctl(fd, UI_SET_KEYBIT, KEY_DOWN);
    ioctl(fd, UI_SET_KEYBIT, KEY_ENTER);
    ioctl(fd, UI_SET_KEYBIT, KEY_ESC);
    ioctl(fd, UI_SET_KEYBIT, KEY_LEFTSHIFT);
    ioctl(fd, UI_SET_KEYBIT, KEY_LEFTALT);
    ioctl(fd, UI_SET_KEYBIT, KEY_LEFTCTRL);
    ioctl(fd, UI_SET_KEYBIT, KEY_LEFTMETA);
    ioctl(fd, UI_SET_KEYBIT, KEY_BACKSPACE);
    ioctl(fd, UI_SET_KEYBIT, KEY_SPACE);
    ioctl(fd, UI_SET_KEYBIT, KEY_TAB);

    ioctl(fd, UI_SET_RELBIT, REL_X);
    ioctl(fd, UI_SET_RELBIT, REL_Y);
    ioctl(fd, UI_SET_RELBIT, REL_WHEEL);
    ioctl(fd, UI_SET_RELBIT, REL_HWHEEL);

    struct uinput_setup usetup = {0};
    snprintf(usetup.name, sizeof(usetup.name), "virtual-mouse");
    usetup.id.bustype = BUS_USB;
    usetup.id.vendor = 0x1;
    usetup.id.product = 0x1;
    usetup.id.version = 1;

    if (ioctl(fd, UI_DEV_SETUP, &usetup) < 0 ||
        ioctl(fd, UI_DEV_CREATE) < 0) {
        perror("uinput setup");
        close(fd);
        return -1;
    }

    fprintf(stderr, "[INFO] Peripherique virtuel cree avec succes.\n");
    return fd;
}

// =============================================================================
// EMISSION D'EVENEMENTS
// =============================================================================

static void emit_events(int fd, uint16_t type, uint16_t code, int32_t value)
{
    struct input_event evs[2] = {0};
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);

    evs[0].time.tv_sec = ts.tv_sec;
    evs[0].time.tv_usec = ts.tv_nsec / 1000;
    evs[0].type = type;
    evs[0].code = code;
    evs[0].value = value;

    evs[1] = evs[0];
    evs[1].type = EV_SYN;
    evs[1].code = SYN_REPORT;
    evs[1].value = 0;

    write(fd, evs, sizeof(evs));
}

// =============================================================================
// SON DE DEMARRAGE / ARRET (ffplay detache)
// =============================================================================

static void play_sound(const char *path)
{
    pid_t pid = fork();
    if (pid != 0)
        return;

    // Enfant : silencie ffplay puis le lance
    int null_fd = open("/dev/null", O_WRONLY);
    if (null_fd >= 0) {
        dup2(null_fd, STDOUT_FILENO);
        dup2(null_fd, STDERR_FILENO);
        close(null_fd);
    }
    execlp("ffplay", "ffplay", "-nodisp", "-autoexit", path, (char *)NULL);
    _exit(1);
}

// =============================================================================
// COURBE DE SENSIBILITE (reprise exacte de mouse.py)
// =============================================================================

static double apply_easing_quadratic_curve(double normalized_value)
{
    double abs_val = fabs(normalized_value);
    double sign = normalized_value > 0 ? 1.0 : -1.0;
    double multiplier;

    if (abs_val <= 0.35) {
        multiplier = 0.2;
    } else if (abs_val <= 0.75) {
        double t = (abs_val - 0.35) / 0.4;
        multiplier = 0.2 + 0.5 * t;
    } else {
        double t = (abs_val - 0.75) / 0.25;
        double eased = t * t;
        multiplier = 0.7 + 0.3 * eased;
    }

    return sign * multiplier * abs_val;
}

static double normalize_axis(int value, int min_val, int max_val)
{
    if (max_val <= min_val)
        return 0.0;
    double normalized = (double)(value - min_val) / (double)(max_val - min_val);
    return (normalized * 2.0) - 1.0;
}

static double now_seconds(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec / 1e9;
}

// =============================================================================
// MAPPING BOUTONS MANETTE -> CLAVIER/SOURIS
// =============================================================================

struct button_map {
    uint16_t in_code;   // bouton manette
    uint16_t out_code;  // touche/clic emis
    int state;          // etat courant
    int last;           // etat precedent
};

static struct button_map buttons[] = {
    { BTN_SOUTH,  BTN_LEFT,      0, 0 },   // Croix : clic gauche
    { BTN_EAST,   BTN_RIGHT,     0, 0 },   // Rond : clic droit
    { BTN_THUMBR, BTN_MIDDLE,    0, 0 },   // R3 : clic milieu
    { BTN_WEST,   KEY_BACKSPACE, 0, 0 },   // Carre : retour arriere
    { BTN_NORTH,  KEY_SPACE,     0, 0 },   // Triangle : espace
    { BTN_TL,     KEY_LEFTALT,   0, 0 },   // L1 : Alt (tenir)
    { BTN_TR,     KEY_LEFTMETA,  0, 0 },   // R1 : Super (tenir)
    { BTN_TL2,    KEY_LEFTCTRL,  0, 0 },   // L2 : Ctrl (tenir)
    { BTN_TR2,    KEY_LEFTSHIFT, 0, 0 },   // R2 : Maj (tenir)
    { BTN_THUMBL, KEY_ESC,       0, 0 },   // L3 : Echap
    { BTN_SELECT, KEY_TAB,       0, 0 },
    { BTN_START,  KEY_ENTER,     0, 0 },
};
#define NB_BUTTONS (sizeof(buttons) / sizeof(buttons[0]))

// =============================================================================
// MOLETTE (stick droit) : crans discrets, proportionnels a la deflection
// =============================================================================

/* `v` > 0 = cran positif. On ne peut PAS envoyer une valeur fractionnaire :
 * les compositeurs ne lisent que des crans entiers et les valeurs inferieures
 * a 1 sont perdues. On accumule donc dans `carry` et on n'emet qu'au
 * franchissement de l'unite (meme logique que l'accumulateur sub-pixel du
 * pointeur, mais par axe). */
static void molette_axe(int ufd, double v, double *carry, uint16_t code, double dt)
{
    double a = fabs(v);
    if (a < DEAD_ZONE) {
        *carry = 0.0;
        return;
    }
    double m = (a - DEAD_ZONE) / (1.0 - DEAD_ZONE);
    if (m > 1.0)
        m = 1.0;

    double vitesse = MOLETTE_MIN + (MOLETTE_MAX - MOLETTE_MIN) * m;
    *carry += (v > 0.0 ? vitesse : -vitesse) * dt;

    int crans = (int)*carry;
    if (crans != 0) {
        emit_events(ufd, EV_REL, code, crans);
        *carry -= crans;
    }
}

// =============================================================================
// BOUCLE PRINCIPALE
// =============================================================================

/* Demande au resident d'ouvrir le panneau. A appeler APRES avoir relache le
 * grab : la requete part en un datagramme sur son socket, elle ne touche pas
 * la manette. */
static void demander_panneau(const char *node)
{
    if (!ipc_socket[0] || !node || !*node)
        return;
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0)
        return;
    struct sockaddr_un addr = {0};
    addr.sun_family = AF_UNIX;
    snprintf(addr.sun_path, sizeof addr.sun_path, "%s", ipc_socket);
    if (connect(fd, (struct sockaddr *)&addr, sizeof addr) == 0) {
        char msg[512];
        int len = snprintf(msg, sizeof msg, "open %s\n", node);
        if (len > 0)
            (void)!write(fd, msg, (size_t)len);
    }
    close(fd);
}

int main(int argc, char **argv)
{
    // Reaper automatique des enfants (ffplay)
    struct sigaction sa = {0};
    sa.sa_handler = SIG_IGN;
    sa.sa_flags = SA_NOCLDWAIT;
    sigaction(SIGCHLD, &sa, NULL);

    // arret sur signal : la gate DOIT etre refermee (sinon la manette reste
    // coupee pour tout le monde jusqu'au prochain `close` du demon)
    signal(SIGINT, on_signal);
    signal(SIGTERM, on_signal);
    signal(SIGHUP, on_signal);

    // --gate <socket> et --gate-target <node> (un par node js/hidraw), donnes
    // par gpad-trigger : les memes cibles que l'overlay.
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--gate") == 0 && i + 1 < argc) {
            gate_socket = argv[++i];
        } else if (strcmp(argv[i], "--ipc") == 0 && i + 1 < argc) {
            snprintf(ipc_socket, sizeof ipc_socket, "%s", argv[++i]);
        } else if (strcmp(argv[i], "--gate-target") == 0 && i + 1 < argc &&
                   n_gate_targets < (int)(sizeof(gate_targets) / sizeof(gate_targets[0]))) {
            gate_targets[n_gate_targets++] = argv[++i];
        }
    }

    const char *device_path = find_controller();
    if (!device_path)
        return 1;

    int ctrl = open(device_path, O_RDONLY | O_NONBLOCK);
    if (ctrl < 0) {
        perror("open controller");
        return 1;
    }

    // evdev nous seul : les jeux (SDL/wine) ne voient plus stick ni boutons.
    // Le grab est RETENTÉ dans la boucle : lancé depuis le panneau, il est
    // tenu par l'overlay et le premier essai échoue — sans le retenter, le
    // mode souris restait sans grab après la fermeture du panneau (le jeu
    // continuait de recevoir la manette).
    int grabe = 0;
    if (ioctl(ctrl, EVIOCGRAB, 1) == 0)
        grabe = 1;
    else
        fprintf(stderr, "[WARN] grab refusé (%s) : nouvel essai dans la boucle\n",
                strerror(errno));

    // js/hidraw : le trou que le grab ne ferme pas, coupe par le gate BPF
    if (gate_socket && n_gate_targets > 0) {
        for (int i = 0; i < n_gate_targets; i++) {
            char commande[300];
            snprintf(commande, sizeof(commande), "target %s", gate_targets[i]);
            gate_cmd(gate_socket, commande);
        }
        gate_ouvert = gate_cmd(gate_socket, "open");
        if (!gate_ouvert)
            fprintf(stderr, "[WARN] gate %s : open refusé\n", gate_socket);
    }

    // Plages reelles des quatre axes. Le pointeur est le stick GAUCHE (le
    // pouce droit tient les boutons : Croix = clic gauche) ; le stick droit
    // sert de molette. Si la manette n'a pas de stick gauche (banc, manette
    // exotique), on retombe sur l'ancien comportement : stick droit =
    // pointeur, pas de molette.
    struct axe gx = {0}, gy = {0}, dx = {0}, dy = {0};
    lire_axe(ctrl, ABS_X, &gx);
    lire_axe(ctrl, ABS_Y, &gy);
    lire_axe(ctrl, ABS_RX, &dx);
    lire_axe(ctrl, ABS_RY, &dy);

    bool pointeur_gauche = gx.present && gy.present;
    if (!pointeur_gauche && !(dx.present && dy.present)) {
        fprintf(stderr, "[ERREUR] Aucun stick utilisable (ABS_X/Y ni ABS_RX/RY).\n");
        close(ctrl);
        return 1;
    }
    bool molette = pointeur_gauche && dx.present && dy.present;
    if (!pointeur_gauche)
        fprintf(stderr, "[WARN] pas de stick gauche : pointeur sur le stick droit, molette absente\n");

    int ufd = create_virtual_device();
    if (ufd < 0) {
        close(ctrl);
        return 1;
    }

    play_sound(getenv("CLIC_WAV") ? getenv("CLIC_WAV") : START_SOUND_DEFAUT);
    fprintf(stderr, "[INFO] Son de demarrage lance en arriere-plan.\n");

    // Etats courants
    int hat_x = 0, hat_y = 0;
    int last_hat_up = 0, last_hat_down = 0, last_hat_left = 0, last_hat_right = 0;
    int mode_state = 0, r3_state = 0;
    double dernier_grab = 0.0;
    /* Fenêtre pendant laquelle on NE retente PAS le grab : on vient de préter
     * la manette au panneau, il doit pouvoir la prendre. Sans ça, notre retry
     * (0,5 s) reprenait le grab avant l'overlay, qui se retrouvait avec un
     * panneau ouvert et aveugle — Home « marchait une fois sur deux ». */
    double pause_reprise = 0.0;

    // Accumulateur de mouvement sub-pixel (reprise de mouse.py) + crans de
    // molette en attente (un cran n'est emis qu'entier)
    double carry_x = 0.0, carry_y = 0.0;
    double carry_mx = 0.0, carry_my = 0.0;

    double last_time = now_seconds();
    const double update_interval = 1.0 / UPDATE_RATE;

    while (running) {
        double loop_start = now_seconds();

        // Vidage des evenements en attente : mise a jour des etats
        struct input_event ev;
        while (true) {
            ssize_t bytes = read(ctrl, &ev, sizeof(ev));
            if (bytes < 0) {
                if (errno == EAGAIN || errno == EWOULDBLOCK)
                    break;
                fprintf(stderr, "[ERREUR] Manette deconnectee.\n");
                gate_fermer();
                ioctl(ctrl, EVIOCGRAB, 0);
                close(ctrl);
                ioctl(ufd, UI_DEV_DESTROY);
                close(ufd);
                return 1;
            }
            if (bytes != sizeof(ev))
                continue;

            if (ev.type == EV_ABS) {
                switch (ev.code) {
                case ABS_X:     gx.value = ev.value; break;
                case ABS_Y:     gy.value = ev.value; break;
                case ABS_RX:    dx.value = ev.value; break;
                case ABS_RY:    dy.value = ev.value; break;
                case ABS_HAT0X: hat_x = ev.value; break;
                case ABS_HAT0Y: hat_y = ev.value; break;
                }
            } else if (ev.type == EV_KEY) {
                if (ev.code == BTN_MODE) {
                    mode_state = ev.value;
                    // Home seul : on rend la manette et on redemande le
                    // panneau. Sans ca, le grab du mode souris rendait le
                    // declencheur sourd a Home et le menu ne revenait plus
                    // (signale le 25/11/2026).
                    // Seulement si NOUS tenons la manette : panneau ouvert,
                    // c'est l'overlay qui a le grab et Home lui appartient.
                    // On ne quitte PAS le mode souris : on prête la manette au
                    // panneau (il doit pouvoir grabber) et on la reprend à sa
                    // fermeture par le grab retenté plus haut. Quitter ici
                    // laissait un `gpad-mouse` mort mais vu « ACTIF » par le
                    // panneau, et le mode souris ne revenait plus (constaté le
                    // 25/11/2026).
                    if (ev.value == 1 && grabe) {
                        fprintf(stderr, "[INFO] Home : manette pretee au panneau.\n");
                        ioctl(ctrl, EVIOCGRAB, 0);
                        grabe = 0;
                        pause_reprise = now_seconds() + 2.0;
                        demander_panneau(device_path);
                    }
                } else {
                    // R3 est aussi le combo de sortie (Home+R3) : on garde son
                    // etat *et* on le mappe (clic milieu)
                    if (ev.code == BTN_THUMBR)
                        r3_state = ev.value;
                    for (size_t b = 0; b < NB_BUTTONS; b++) {
                        if (buttons[b].in_code == ev.code) {
                            buttons[b].state = ev.value;
                            break;
                        }
                    }
                }
            }
        }

        double current_time = now_seconds();
        // grab différé : le panneau le rend en se fermant (~0,5 s de latence)
        if (!grabe && current_time >= pause_reprise &&
            current_time - dernier_grab > 0.5) {
            dernier_grab = current_time;
            if (ioctl(ctrl, EVIOCGRAB, 1) == 0) {
                grabe = 1;
                fprintf(stderr, "[INFO] manette grabbe (panneau fermé).\n");
            }
        }
        double dt = current_time - last_time;
        last_time = current_time;
        if (dt > 0.1)
            dt = 0.1;

        // ----- Mouvement souris (stick gauche, courbe progressive) -----
        const struct axe *ax = pointeur_gauche ? &gx : &dx;
        const struct axe *ay = pointeur_gauche ? &gy : &dy;
        double norm_x = normalize_axis(ax->value, ax->minimum, ax->maximum);
        double norm_y = normalize_axis(ay->value, ay->minimum, ay->maximum);
        double raw_magnitude = sqrt(norm_x * norm_x + norm_y * norm_y);

        double magnitude = 0.0, dir_x = 0.0, dir_y = 0.0;
        if (raw_magnitude >= DEAD_ZONE) {
            if (raw_magnitude > 0.001) {
                dir_x = norm_x / raw_magnitude;
                dir_y = norm_y / raw_magnitude;
            }
            magnitude = (raw_magnitude - DEAD_ZONE) / (1.0 - DEAD_ZONE);
        }

        if (magnitude > 0.001) {
            double curve = apply_easing_quadratic_curve(magnitude);
            double speed = BASE_SENSITIVITY +
                           (MAX_SENSITIVITY - BASE_SENSITIVITY) * curve;

            carry_x += dir_x * speed * dt * 60.0;
            carry_y += dir_y * speed * dt * 60.0;

            int int_x = (int)llround(carry_x);
            int int_y = (int)llround(carry_y);
            carry_x -= int_x;
            carry_y -= int_y;

            if (int_x != 0)
                emit_events(ufd, EV_REL, REL_X, int_x);
            if (int_y != 0)
                emit_events(ufd, EV_REL, REL_Y, int_y);
        }

        // ----- Molette (stick droit) : Y = verticale, X = horizontale -----
        // Le noyau donne Y positif vers le bas ; une molette positive fait
        // monter le contenu : d'ou le signe inverse. REL_HWHEEL positif va
        // vers la droite, comme X.
        if (molette) {
            double wx = normalize_axis(dx.value, dx.minimum, dx.maximum);
            double wy = normalize_axis(dy.value, dy.minimum, dy.maximum);
            molette_axe(ufd, wx, &carry_mx, REL_HWHEEL, dt);
            molette_axe(ufd, -wy, &carry_my, REL_WHEEL, dt);
        }

        // ----- Boutons : emission sur changement d'etat -----
        for (size_t b = 0; b < NB_BUTTONS; b++) {
            if (buttons[b].state != buttons[b].last) {
                emit_events(ufd, EV_KEY, buttons[b].out_code, buttons[b].state);
                buttons[b].last = buttons[b].state;
            }
        }

        // ----- D-pad -> fleches -----
        int hat_up = (hat_y == -1), hat_down = (hat_y == 1);
        int hat_left = (hat_x == -1), hat_right = (hat_x == 1);
        if (hat_up != last_hat_up) {
            emit_events(ufd, EV_KEY, KEY_UP, hat_up);
            last_hat_up = hat_up;
        }
        if (hat_down != last_hat_down) {
            emit_events(ufd, EV_KEY, KEY_DOWN, hat_down);
            last_hat_down = hat_down;
        }
        if (hat_left != last_hat_left) {
            emit_events(ufd, EV_KEY, KEY_LEFT, hat_left);
            last_hat_left = hat_left;
        }
        if (hat_right != last_hat_right) {
            emit_events(ufd, EV_KEY, KEY_RIGHT, hat_right);
            last_hat_right = hat_right;
        }

        // ----- Home + R3 : quitter -----
        if (mode_state == 1 && r3_state == 1) {
            fprintf(stderr, "[INFO] Bouton Home/Guide et R3 presses : arret.\n");
            play_sound(getenv("NOCLIC_WAV") ? getenv("NOCLIC_WAV") : EXIT_SOUND_DEFAUT);
            usleep(700000);
            running = 0;
            break;
        }

        // Cadence a 120 Hz
        double elapsed = now_seconds() - loop_start;
        if (elapsed < update_interval) {
            struct timespec pause = {
                .tv_sec = 0,
                .tv_nsec = (long)((update_interval - elapsed) * 1e9),
            };
            // nanosleep (pas usleep) : un signal l'interrompt et la boucle
            // sort par `running`
            nanosleep(&pause, NULL);
        }
    }

    // Sortie : on rend la manette et on referme la gate, toujours.
    gate_fermer();
    fprintf(stderr, "[INFO] mode souris : manette rendue (grab relâché).\n");
    // relâche les boutons encore enfoncés : un clic ou un modificateur ne doit
    // pas survivre au périphérique (R1 = Super tenu au moment de Home+R3)
    for (size_t b = 0; b < NB_BUTTONS; b++) {
        if (buttons[b].last)
            emit_events(ufd, EV_KEY, buttons[b].out_code, 0);
    }
    ioctl(ctrl, EVIOCGRAB, 0);
    close(ctrl);
    ioctl(ufd, UI_DEV_DESTROY);
    close(ufd);
    return 0;
}
