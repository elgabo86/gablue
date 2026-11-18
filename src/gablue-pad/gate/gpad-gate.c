/* gpad-gate - démon de gate BPF LSM pour les nodes d'une manette.
 *
 *   gpad-gate [--socket <chemin>] [--target <node>]... [--timeout <secondes>]
 *
 * Principe : le programme BPF est chargé une fois, mais ATTACHÉ seulement
 * pendant le menu. Hors menu, le hook n'existe pas dans le noyau : coût nul.
 *
 * Commandes (socket unix SOCK_STREAM, une par connexion) :
 *   target <node>   ajoute un node à couper (résolu en dev_t noyau, idempotent)
 *   clear           vide la liste des cibles (le panneau la repose à chaque
 *                   ouverture : sans ça, un démon de session laissé tourner
 *                   épuise les 8 emplacements au fil des reconnexions)
 *   exempt <pid>    autorise un pid (le menu) à continuer à lire
 *   open            attache le hook -> plus personne d'autre ne lit
 *   close           détache le hook -> tout redevient normal
 *   status          état courant
 *   quit            termine
 *
 * Garde-fous : --timeout (défaut 120 s) coupe le démon s'il ne reçoit plus
 * rien ; à la sortie, quoi qu'il arrive, le hook est détaché.
 */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#include <bpf/bpf.h>
#include <bpf/libbpf.h>

#define MAX_TARGETS 8
#define MAX_EXEMPT 4
/* Connexions simultanées servies, et délai d'abandon d'un client muet. */
#define MAX_CLIENTS 8
#define CLIENT_IDLE 2

#define CFG_NTARGET 0
#define CFG_EXEMPT 1
#define CFG_TARGETS 8

static volatile sig_atomic_t stop;
static struct bpf_link *g_link;
static struct bpf_map *g_cfg, *g_stats;
static int g_ntarget, g_nexempt;

static void on_term(int s)
{
    (void)s;
    stop = 1;
}

static void set_u64(struct bpf_map *map, __u32 key, __u64 val)
{
    bpf_map_update_elem(bpf_map__fd(map), &key, &val, BPF_ANY);
}

static __u64 get_u64(struct bpf_map *map, __u32 key)
{
    __u64 v = 0;
    bpf_map_lookup_elem(bpf_map__fd(map), &key, &v);
    return v;
}

/* Ajoute une cible, sans doublon : le panneau rejoue `target` à chaque
 * ouverture et une même manette garde le même dev_t tant qu'elle est là. */
static void add_target(__u64 dev, const char *label)
{
    for (int i = 0; i < g_ntarget; i++)
        if (get_u64(g_cfg, CFG_TARGETS + i) == dev) {
            printf("ADD %s -> deja (0x%llx)\n", label, (unsigned long long)dev);
            fflush(stdout);
            return;
        }
    if (g_ntarget >= MAX_TARGETS) {
        printf("ERROR trop de cibles\n");
        fflush(stdout);
        return;
    }
    set_u64(g_cfg, CFG_TARGETS + g_ntarget, dev);
    g_ntarget++;
    set_u64(g_cfg, CFG_NTARGET, (__u64)g_ntarget);
    printf("ADD %s -> dev 0x%llx\n", label, (unsigned long long)dev);
    fflush(stdout);
}

static void clear_targets(void)
{
    g_ntarget = 0;
    set_u64(g_cfg, CFG_NTARGET, 0);
    printf("CLEAR\n");
    fflush(stdout);
}

/* dev_t noyau (major<<20 | minor) : c'est ce que contient inode->i_rdev */
static __u64 kernel_dev(const char *node)
{
    struct stat st;
    if (stat(node, &st) < 0)
        return 0;
    if (!S_ISCHR(st.st_mode) && !S_ISBLK(st.st_mode))
        return 0;
    return ((__u64)major(st.st_rdev) << 20) | (__u64)minor(st.st_rdev);
}

static void status(void)
{
    printf("STATUS state=%s ntarget=%d nexempt=%d refus=%llu\n", g_link ? "open" : "closed",
           g_ntarget, g_nexempt, (unsigned long long)get_u64(g_stats, 1));
    for (int i = 0; i < g_ntarget; i++) {
        __u64 v = get_u64(g_cfg, CFG_TARGETS + i);
        printf("TARGET %d:%u (0x%llx)\n", (int)(v >> 20), (unsigned)(v & 0xfffff),
               (unsigned long long)v);
    }
    for (int i = 0; i < g_nexempt; i++)
        printf("EXEMPT %llu\n", (unsigned long long)get_u64(g_cfg, CFG_EXEMPT + i));
    fflush(stdout);
}

static void do_open(struct bpf_program *prog)
{
    if (g_link)
        return;
    g_link = bpf_program__attach(prog);
    long e = libbpf_get_error(g_link);
    if (e) {
        g_link = NULL;
        printf("ERROR attach %ld\n", e);
    } else {
        printf("GATE open\n");
    }
    fflush(stdout);
}

static void do_close(void)
{
    if (!g_link)
        return;
    bpf_link__destroy(g_link);
    g_link = NULL;
    printf("GATE closed\n");
    fflush(stdout);
}

/* Traite une commande (stdin ou socket) ; rend 1 sur `quit`. */
static int traiter(char *buf, struct bpf_program *prog)
{
    if (!strncmp(buf, "target ", 7)) {
        __u64 dev = kernel_dev(buf + 7);
        if (!dev)
            printf("ERROR pas un device: %s\n", buf + 7);
        else
            add_target(dev, buf + 7);
    } else if (!strcmp(buf, "clear")) {
        clear_targets();
    } else if (!strncmp(buf, "exempt ", 7)) {
        __u64 pid = strtoull(buf + 7, NULL, 10);
        if (g_nexempt < MAX_EXEMPT) {
            set_u64(g_cfg, CFG_EXEMPT + g_nexempt, pid);
            g_nexempt++;
            printf("EXEMPT %llu\n", (unsigned long long)pid);
        } else {
            printf("ERROR trop d'exemptions\n");
        }
    } else if (!strcmp(buf, "open")) {
        do_open(prog);
    } else if (!strcmp(buf, "close")) {
        do_close();
    } else if (!strcmp(buf, "status")) {
        status();
    } else if (!strcmp(buf, "quit")) {
        return 1;
    } else {
        printf("ERROR commande inconnue\n");
    }
    fflush(stdout);
    return 0;
}

int main(int argc, char **argv)
{
    const char *sockpath = NULL;
    int keepalive = 120;
    const char *targets[MAX_TARGETS];
    int ntarget_arg = 0;

    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--socket") && i + 1 < argc)
            sockpath = argv[++i];
        else if (!strcmp(argv[i], "--target") && i + 1 < argc && ntarget_arg < MAX_TARGETS)
            targets[ntarget_arg++] = argv[++i];
        else if (!strcmp(argv[i], "--timeout") && i + 1 < argc)
            keepalive = atoi(argv[++i]);
    }

    /* .o à côté du binaire */
    char objpath[4096];
    snprintf(objpath, sizeof objpath, "%s", argv[0]);
    char *slash = strrchr(objpath, '/');
    if (slash)
        snprintf(slash + 1, sizeof objpath - (size_t)(slash - objpath) - 1, "gpad-gate.bpf.o");
    else
        snprintf(objpath, sizeof objpath, "gpad-gate.bpf.o");

    struct bpf_object *obj = bpf_object__open_file(objpath, NULL);
    if (!obj) {
        fprintf(stderr, "open %s: %s\n", objpath, strerror(errno));
        return 1;
    }
    int err = bpf_object__load(obj);
    if (err) {
        fprintf(stderr, "load: %s\n", strerror(-err));
        return 1;
    }
    struct bpf_program *prog = bpf_object__find_program_by_name(obj, "hid_gate");
    if (!prog) {
        fprintf(stderr, "programme hid_gate introuvable\n");
        return 1;
    }
    g_cfg = bpf_object__find_map_by_name(obj, "cfg");
    g_stats = bpf_object__find_map_by_name(obj, "stats");
    if (!g_cfg || !g_stats) {
        fprintf(stderr, "maps absentes\n");
        return 1;
    }

    for (int i = 0; i < ntarget_arg; i++) {
        __u64 dev = kernel_dev(targets[i]);
        if (!dev) {
            fprintf(stderr, "cible ignorée (pas un device): %s\n", targets[i]);
            continue;
        }
        add_target(dev, targets[i]);
    }
    printf("READY (chargé, non attaché)\n");
    fflush(stdout);

    signal(SIGTERM, on_term);
    signal(SIGINT, on_term);

    int srv = -1;
    if (sockpath) {
        unlink(sockpath);
        srv = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
        struct sockaddr_un a = {.sun_family = AF_UNIX};
        snprintf(a.sun_path, sizeof a.sun_path, "%s", sockpath);
        if (srv < 0 || bind(srv, (struct sockaddr *)&a, sizeof a) < 0 || listen(srv, 8) < 0) {
            perror("socket");
            return 1;
        }
        /* 0666 : tout compte de la session doit pouvoir ouvrir/fermer le gate.
         * Sur un poste personnel, le risque est qu'un autre compte local coupe
         * les lectures d'une manette — pas d'escalade. */
        chmod(sockpath, 0666);
        printf("SOCKET=%s\n", sockpath);
        fflush(stdout);
    }

    /* Un emplacement par client, avec un délai d'abandon : un client qui se
     * connecte sans envoyer sa commande bloquait le démon, qui ne relisait plus
     * la socket d'écoute. La file du noyau se remplissait alors et les clients
     * SUIVANTS restaient plantés dans connect() — l'overlay se figeait, manette
     * grabée en main, et Home ne répondait plus (vécu 18/11/2026). */
    struct client {
        int fd;
        time_t vu;
    } cl[MAX_CLIENTS];
    for (int i = 0; i < MAX_CLIENTS; i++)
        cl[i].fd = -1;
    int stdin_open = 1;
    time_t last = time(NULL);

    while (!stop) {
        if (keepalive > 0 && time(NULL) - last > keepalive) {
            printf("TIMEOUT (auto-arrêt)\n");
            break;
        }

        struct pollfd p[2 + MAX_CLIENTS];
        int nfd = 0;
        p[nfd++] = (struct pollfd){.fd = stdin_open ? STDIN_FILENO : -1, .events = POLLIN};
        p[nfd++] = (struct pollfd){.fd = srv, .events = POLLIN};
        for (int i = 0; i < MAX_CLIENTS; i++)
            if (cl[i].fd >= 0)
                p[nfd++] = (struct pollfd){.fd = cl[i].fd, .events = POLLIN};

        int pret = poll(p, (nfds_t)nfd, 200);
        time_t now = time(NULL);

        /* Connexion muette (client mort avant d'envoyer) : on libère. */
        for (int i = 0; i < MAX_CLIENTS; i++) {
            if (cl[i].fd >= 0 && now - cl[i].vu > CLIENT_IDLE) {
                close(cl[i].fd);
                cl[i].fd = -1;
            }
        }
        if (pret <= 0)
            continue;

        int quitter = 0;
        if (p[0].revents & (POLLIN | POLLHUP)) {
            char buf[256] = {0};
            ssize_t n = read(STDIN_FILENO, buf, sizeof buf - 1);
            if (n <= 0) {
                stdin_open = 0;
            } else {
                last = now;
                buf[strcspn(buf, "\n")] = 0;
                quitter = traiter(buf, prog);
            }
        }
        if (quitter)
            break;

        /* L'écoute est relue À CHAQUE tour, même avec des clients en cours. */
        if (p[1].revents & POLLIN) {
            int c = accept4(srv, NULL, NULL, SOCK_CLOEXEC);
            if (c >= 0) {
                int libre = -1;
                for (int i = 0; i < MAX_CLIENTS && libre < 0; i++)
                    if (cl[i].fd < 0)
                        libre = i;
                if (libre < 0) {
                    printf("ERROR trop de clients simultanés\n");
                    close(c);
                } else {
                    cl[libre].fd = c;
                    cl[libre].vu = now;
                }
            }
        }

        for (int i = 2; i < nfd && !quitter; i++) {
            if (!(p[i].revents & (POLLIN | POLLHUP | POLLERR)))
                continue;
            for (int j = 0; j < MAX_CLIENTS; j++) {
                if (cl[j].fd < 0 || cl[j].fd != p[i].fd)
                    continue;
                char buf[256] = {0};
                ssize_t n = read(cl[j].fd, buf, sizeof buf - 1);
                if (n > 0) {
                    last = now;
                    buf[strcspn(buf, "\n")] = 0;
                    quitter = traiter(buf, prog);
                }
                /* Protocole : une commande par connexion (le client rouvre). */
                close(cl[j].fd);
                cl[j].fd = -1;
                break;
            }
        }
        if (quitter)
            break;
    }

    for (int i = 0; i < MAX_CLIENTS; i++)
        if (cl[i].fd >= 0)
            close(cl[i].fd);

    do_close();
    if (srv >= 0) {
        if (sockpath)
            unlink(sockpath);
        close(srv);
    }
    bpf_object__close(obj);
    printf("CLOSED\n");
    return 0;
}
