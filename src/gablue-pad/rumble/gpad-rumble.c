// rumble - impulsion de vibration sur une manette (force feedback evdev).
//
//   rumble [node] [ms] [force]
//
//   node   /dev/input/eventN ; vide, "-" ou absent = détection automatique
//   ms     durée en millisecondes (défaut 140)
//   force  intensité en % (défaut 60)
//
// Pourquoi ce binaire existe : c'est le seul retour utilisateur qui ne passe
// NI par l'écran NI par le son, donc qui ne peut pas se retrouver dans un
// replay ou un enregistrement (l'audio du bureau, lui, est capturé).
//
// Le kernel arrête l'effet tout seul (replay.length) ; on attend la fin, on
// envoie l'arrêt explicite puis on efface l'effet, sinon un effet laissé
// enregistré occupe un slot du périphérique.
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <linux/input.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define BITS_PER_LONG (8 * sizeof(unsigned long))

static int bit_set(int nr, const unsigned long *addr)
{
    return (addr[nr / BITS_PER_LONG] >> (nr % BITS_PER_LONG)) & 1UL;
}

/// La manette sait-elle vibrer ? (bitset des effets, FF_RUMBLE demandé)
static int sait_vibrer(int fd)
{
    unsigned long bits[(FF_MAX / BITS_PER_LONG) + 1];
    memset(bits, 0, sizeof bits);
    if (ioctl(fd, EVIOCGBIT(EV_FF, sizeof bits), bits) < 0)
        return 0;
    return bit_set(FF_RUMBLE, bits);
}

/// Nom d'affichage du périphérique (pour le message d'erreur).
static const char *nom_sysfs(const char *node)
{
    static char chemin[256], buf[128];
    const char *base = strrchr(node, '/');
    snprintf(chemin, sizeof chemin, "/sys/class/input/%s/device/name",
             base ? base + 1 : node);
    FILE *f = fopen(chemin, "re");
    if (!f)
        return "?";
    if (!fgets(buf, sizeof buf, f)) {
        fclose(f);
        return "?";
    }
    fclose(f);
    buf[strcspn(buf, "\n")] = '\0';
    return buf;
}

/// Une manette avec FF_RUMBLE (hors capteurs/touchpad), ou NULL.
static const char *detecter(void)
{
    static char trouve[64];
    for (int i = 0; i < 32; i++) {
        char node[32];
        snprintf(node, sizeof node, "/dev/input/event%d", i);
        int fd = open(node, O_RDWR | O_CLOEXEC);
        if (fd < 0)
            continue;
        char nom[128] = "";
        ioctl(fd, EVIOCGNAME(sizeof nom), nom);
        int bon = sait_vibrer(fd) && !strstr(nom, "Motion") && !strstr(nom, "Touchpad");
        int est_manette = strcasestr(nom, "controller") || strcasestr(nom, "dualsense") ||
                          strcasestr(nom, "dualshock") || strcasestr(nom, "gamepad") ||
                          strcasestr(nom, "xbox");
        close(fd);
        if (bon && est_manette) {
            snprintf(trouve, sizeof trouve, "%s", node);
            return trouve;
        }
    }
    return NULL;
}

int main(int argc, char **argv)
{
    const char *node = NULL;
    int ms = 140, force = 60;

    int argi = 1;
    if (argi < argc && argv[argi][0] != '\0' && strcmp(argv[argi], "-") != 0 &&
        strncmp(argv[argi], "/dev/", 5) == 0)
        node = argv[argi++];
    else if (argi < argc && strcmp(argv[argi], "-") == 0)
        argi++;
    if (argi < argc)
        ms = atoi(argv[argi++]);
    if (argi < argc)
        force = atoi(argv[argi++]);

    if (ms <= 0)
        ms = 140;
    if (force <= 0)
        force = 60;
    if (force > 100)
        force = 100;

    if (!node) {
        node = detecter();
        if (!node) {
            fprintf(stderr, "[rumble] aucune manette avec FF_RUMBLE\n");
            return 1;
        }
    }

    int fd = open(node, O_RDWR | O_CLOEXEC);
    if (fd < 0) {
        fprintf(stderr, "[rumble] %s (%s): %s\n", node, nom_sysfs(node), strerror(errno));
        return 1;
    }
    if (!sait_vibrer(fd)) {
        fprintf(stderr, "[rumble] %s (%s): pas de FF_RUMBLE\n", node, nom_sysfs(node));
        close(fd);
        return 1;
    }

    struct ff_effect effet;
    memset(&effet, 0, sizeof effet);
    effet.type = FF_RUMBLE;
    effet.id = -1;
    effet.u.rumble.strong_magnitude = (uint16_t)(force * 0xffff / 100);
    effet.u.rumble.weak_magnitude = (uint16_t)(force * 0xffff / 100);
    effet.replay.length = (uint16_t)ms;
    effet.replay.delay = 0;

    if (ioctl(fd, EVIOCSFF, &effet) < 0) {
        fprintf(stderr, "[rumble] %s: envoi refusé: %s\n", node, strerror(errno));
        close(fd);
        return 1;
    }

    struct input_event ev;
    memset(&ev, 0, sizeof ev);
    ev.type = EV_FF;
    ev.code = (uint16_t)effet.id;
    ev.value = 1;
    if (write(fd, &ev, sizeof ev) != (ssize_t)sizeof ev) {
        fprintf(stderr, "[rumble] %s: démarrage impossible: %s\n", node, strerror(errno));
        ioctl(fd, EVIOCRMFF, effet.id);
        close(fd);
        return 1;
    }

    // le kernel coupe à replay.length ; on laisse une marge puis on nettoie
    usleep((useconds_t)ms * 1000 + 30000);
    ev.value = 0;
    write(fd, &ev, sizeof ev);
    ioctl(fd, EVIOCRMFF, effet.id);
    close(fd);

    printf("RUMBLE %s %dms %d%%\n", node, ms, force);
    fflush(stdout);
    return 0;
}
