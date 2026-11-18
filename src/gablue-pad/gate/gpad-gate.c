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

    int client = -1, stdin_open = 1;
    time_t last = time(NULL);

    while (!stop) {
        if (keepalive > 0 && time(NULL) - last > keepalive) {
            printf("TIMEOUT (auto-arrêt)\n");
            break;
        }
        struct pollfd p[2] = {
            {.fd = stdin_open ? STDIN_FILENO : -1, .events = POLLIN},
            {.fd = (client >= 0) ? client : srv, .events = POLLIN},
        };
        if (poll(p, 2, 200) <= 0)
            continue;

        int fd = -1;
        if (p[0].revents & POLLIN) {
            fd = STDIN_FILENO;
        } else if (p[1].revents & POLLIN) {
            if (client < 0) {
                client = accept4(srv, NULL, NULL, SOCK_CLOEXEC);
                last = time(NULL);
                continue;
            }
            fd = client;
        }
        if (fd < 0)
            continue;

        char buf[256] = {0};
        ssize_t n = read(fd, buf, sizeof buf - 1);
        if (n <= 0) {
            if (fd == STDIN_FILENO) {
                stdin_open = 0;
            } else {
                close(client);
                client = -1;
            }
            continue;
        }
        last = time(NULL);
        buf[strcspn(buf, "\n")] = 0;

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
            break;
        } else {
            printf("ERROR commande inconnue\n");
        }
        fflush(stdout);
    }

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
