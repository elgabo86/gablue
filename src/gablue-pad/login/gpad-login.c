// gpad-login : mode souris pour l'écran de login.
//
// Mouvement : EXACTEMENT la courbe de gpad-mouse (zone morte 0.08,
// sensibilité 2.0 → 12.0, courbe quadratique par paliers, accumulateur
// sub-pixel, 120 Hz). Au login, le stick GAUCHE ou DROIT commande le pointeur
// (le plus écarté des deux gagne).
//
//   X / Croix, R1 .......... clic gauche
//   Carré .................. Retour arrière
//   Start .................. Entrée
//
// À chaque vrai mouvement (stick hors zone morte, D-pad, appui de touche, ou
// pointeur qui avance), SIGUSR1 est envoyé au parent — mais uniquement si
// GPAD_LOGIN_NOTIFY_PID vaut bien son PID. C'est ce signal qui ouvre le clavier
// visuel du greeter (jamais le simple branchement) et qui le rouvre après sa
// fermeture pour inactivité. Lancé à la main (variable absente), il ne signale
// personne.
//
// La manette n'est PAS grabée. Sortie nette : SIGTERM/SIGINT détruit le
// périphérique virtuel.
#define _POSIX_C_SOURCE 200809L
#define _DEFAULT_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <linux/uinput.h>
#include <math.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/poll.h>
#include <time.h>
#include <unistd.h>

// Repris tel quel de src/gablue-pad/mouse/gpad-mouse.c
#define DEAD_ZONE 0.08
#define BASE_SENSITIVITY 2.0
#define MAX_SENSITIVITY 12.0
#define UPDATE_RATE 120

static int ufd = -1;
static volatile sig_atomic_t running = 1;

static void on_signal(int sig) { (void)sig; running = 0; }

static void emit(int type, int code, int val) {
    struct input_event ev = { 0 };
    ev.type = type; ev.code = code; ev.value = val;
    if (write(ufd, &ev, sizeof ev) < 0) perror("[WARN] write uinput");
}
static void sync_ev(void) { emit(EV_SYN, SYN_REPORT, 0); }

static double now_seconds(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec / 1e9;
}

static double apply_easing_quadratic_curve(double v) {
    double a = fabs(v);
    double sign = v > 0 ? 1.0 : -1.0;
    double m;
    if (a <= 0.35) {
        m = 0.2;
    } else if (a <= 0.75) {
        m = 0.2 + 0.5 * ((a - 0.35) / 0.4);
    } else {
        double t = (a - 0.75) / 0.25;
        m = 0.7 + 0.3 * (t * t);
    }
    return sign * m * a;
}

static double normalize_axis(int value, int min_val, int max_val) {
    if (max_val <= min_val) return 0.0;
    return ((double)(value - min_val) / (double)(max_val - min_val)) * 2.0 - 1.0;
}

static bool test_bit(int fd, int type, int code) {
    unsigned long bits[(KEY_MAX / (8 * sizeof(unsigned long))) + 2] = { 0 };
    if (ioctl(fd, EVIOCGBIT(type, sizeof bits), bits) < 0) return false;
    return (bits[code / (8 * sizeof(unsigned long))] >>
            (code % (8 * sizeof(unsigned long)))) & 1UL;
}

struct axis { bool present; struct input_absinfo info; int value; };

static void axis_read(int fd, int code, struct axis *a) {
    if (ioctl(fd, EVIOCGABS(code), &a->info) < 0) { a->present = false; return; }
    a->present = true;
    a->value = a->info.value;
}

/* Notification « la manette est utilisée », par SIGUSR1 au parent.
 * GPAD_LOGIN_NOTIFY_PID doit valoir le PID du parent : sans elle (lancement à
 * la main) on ne signale rien, et si le parent a changé (orphelin) non plus —
 * on ne risque donc pas de tuer un processus qui aurait récupéré le PID.
 *
 * Plusieurs signaux, pas un seul : le pont referme le clavier après une
 * inactivité, et le moindre mouvement doit le rouvrir. Débit limité à
 * 2 signaux/s — un stick maintenu déplace le pointeur en continu, sans quoi
 * le réveil du shell à 120 Hz serait du gâchis pur. */
static char notify_pid[32];
static double last_notify;          /* CLOCK_MONOTONIC, 0 = jamais */
static double opened_at;            /* idem, au démarrage du pointeur */
#define NOTIFY_MIN_INTERVAL 0.5
/* Délai de stabilisation : à l'ouverture du node, une manette peut renvoyer son
 * état complet (axes hors zone morte, pavé tactile, boutons). Ces évènements ne
 * sont pas un geste de l'utilisateur : sans cette garde, le clavier visuel du
 * greeter s'ouvrait au branchement d'une manette (signalé le 24/11/2026). */
#define NOTIFY_SETTLE 0.6

static void notify_input(void) {
    if (!notify_pid[0]) return;
    double now = now_seconds();
    if (opened_at > 0.0 && now - opened_at < NOTIFY_SETTLE) return;
    if (last_notify > 0.0 && now - last_notify < NOTIFY_MIN_INTERVAL) return;
    last_notify = now;
    pid_t parent = getppid();
    if (parent <= 1 || (long)parent != atol(notify_pid)) return;
    if (kill(parent, SIGUSR1) < 0)
        fprintf(stderr, "[WARN] notification du pont (pid %ld) : %s\n",
                (long)parent, strerror(errno));
}

/* Amplitude du stick le plus écarté, comme la boucle de pointeur : sert à
 * ignorer l'état initial relu à l'ouverture (manette au repos). */
static double stick_raw_mag(const struct axis *lx, const struct axis *ly,
                            const struct axis *rx, const struct axis *ry) {
    double lnx = lx->present ? normalize_axis(lx->value, lx->info.minimum, lx->info.maximum) : 0.0;
    double lny = ly->present ? normalize_axis(ly->value, ly->info.minimum, ly->info.maximum) : 0.0;
    double rnx = rx->present ? normalize_axis(rx->value, rx->info.minimum, rx->info.maximum) : 0.0;
    double rny = ry->present ? normalize_axis(ry->value, ry->info.minimum, ry->info.maximum) : 0.0;
    double lmag = sqrt(lnx * lnx + lny * lny);
    double rmag = sqrt(rnx * rnx + rny * rny);
    return (lmag >= rmag) ? lmag : rmag;
}

static int uinput_create(void) {
    int fd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);
    if (fd < 0) { perror("[ERREUR] open /dev/uinput"); return -1; }
    if (ioctl(fd, UI_SET_EVBIT, EV_KEY) < 0 || ioctl(fd, UI_SET_EVBIT, EV_SYN) < 0 ||
        ioctl(fd, UI_SET_EVBIT, EV_REL) < 0) { perror("[ERREUR] evbit"); return -1; }
    ioctl(fd, UI_SET_KEYBIT, BTN_LEFT);
    ioctl(fd, UI_SET_KEYBIT, KEY_BACKSPACE);
    ioctl(fd, UI_SET_KEYBIT, KEY_ENTER);
    ioctl(fd, UI_SET_RELBIT, REL_X);
    ioctl(fd, UI_SET_RELBIT, REL_Y);
    struct uinput_setup us = { 0 };
    us.id.bustype = BUS_USB;
    us.id.vendor = 0x1d6b;
    us.id.product = 0x0114;
    snprintf(us.name, sizeof us.name, "gpad-login");
    if (ioctl(fd, UI_DEV_SETUP, &us) < 0 || ioctl(fd, UI_DEV_CREATE) < 0) {
        perror("[ERREUR] création uinput");
        return -1;
    }
    return fd;
}

static int find_pad(char *out, size_t len, bool *has_right) {
    DIR *d = opendir("/dev/input");
    if (!d) { perror("[ERREUR] /dev/input"); return -1; }
    int found = -1;
    struct dirent *e;
    while ((e = readdir(d))) {
        if (strncmp(e->d_name, "event", 5) != 0) continue;
        char path[512];   // 512 : un d_name fait au plus 255, jamais tronqué
        snprintf(path, sizeof path, "/dev/input/%s", e->d_name);
        int fd = open(path, O_RDONLY | O_NONBLOCK);
        if (fd < 0) continue;
        char name[64] = { 0 };
        ioctl(fd, EVIOCGNAME(sizeof name - 1), name);
        if (strstr(name, "gpad-login")) { close(fd); continue; }   // pas nous-mêmes
        bool left  = test_bit(fd, EV_ABS, ABS_X) && test_bit(fd, EV_ABS, ABS_Y);
        bool right = test_bit(fd, EV_ABS, ABS_RX) && test_bit(fd, EV_ABS, ABS_RY);
        bool ok = (left || right) && test_bit(fd, EV_KEY, BTN_SOUTH) &&
                  test_bit(fd, EV_KEY, BTN_START);
        if (ok) {
            printf("[INFO] Manette : %s - %s (sticks : %s%s)\n", path, name,
                   left ? "gauche" : "", right ? " droit" : "");
            snprintf(out, len, "%s", path);
            *has_right = right;
            found = fd;
            break;
        }
        close(fd);
    }
    closedir(d);
    return found;
}

int main(void) {
    signal(SIGINT, on_signal);
    signal(SIGTERM, on_signal);

    const char *env_pid = getenv("GPAD_LOGIN_NOTIFY_PID");
    if (env_pid) snprintf(notify_pid, sizeof notify_pid, "%s", env_pid);

    char path[512];
    bool has_right = false;
    int pad = find_pad(path, sizeof path, &has_right);
    if (pad < 0) { fprintf(stderr, "[ERREUR] aucune manette\n"); return 1; }

    struct axis lx = { 0 }, ly = { 0 }, rx = { 0 }, ry = { 0 };
    axis_read(pad, ABS_X, &lx);
    axis_read(pad, ABS_Y, &ly);
    if (has_right) { axis_read(pad, ABS_RX, &rx); axis_read(pad, ABS_RY, &ry); }

    ufd = uinput_create();
    if (ufd < 0) return 1;

    fprintf(stderr, "[INFO] Mode login : stick gauche/droit=pointeur (courbe gpad-mouse), "
                    "X/R1=clic, Carré=retour arrière, Start=Entrée\n");

    double carry_x = 0.0, carry_y = 0.0;
    double last_time = now_seconds();
    // la fenêtre de stabilisation part de l'ouverture du périphérique
    opened_at = last_time;

    while (running) {
        struct timespec tick = { 0, 1000000000L / UPDATE_RATE };
        nanosleep(&tick, NULL);
        if (!running) break;

        struct input_event ev;
        while (true) {
            ssize_t bytes = read(pad, &ev, sizeof ev);
            if (bytes < 0) {
                if (errno == EAGAIN || errno == EWOULDBLOCK) break;
                fprintf(stderr, "[ERREUR] manette déconnectée\n");
                goto out;
            }
            if (bytes != (ssize_t)sizeof ev) break;

            if (ev.type == EV_ABS) {
                switch (ev.code) {
                case ABS_X:  lx.value = ev.value; break;
                case ABS_Y:  ly.value = ev.value; break;
                case ABS_RX: rx.value = ev.value; break;
                case ABS_RY: ry.value = ev.value; break;
                default: break;
                }
            } else if (ev.type == EV_KEY) {
                int code = 0;
                if (ev.code == BTN_SOUTH || ev.code == BTN_TR) code = BTN_LEFT;
                else if (ev.code == BTN_WEST)  code = KEY_BACKSPACE;
                else if (ev.code == BTN_START) code = KEY_ENTER;
                if (code) { emit(EV_KEY, code, ev.value); sync_ev(); }
            }

            if (ev.type == EV_KEY && ev.value == 1) {
                notify_input();                   // appui : la manette est tenue
            } else if (ev.type == EV_ABS) {
                bool hat = (ev.code == ABS_HAT0X || ev.code == ABS_HAT0Y);
                // Stick : il doit SORTIR de la zone morte — l'état relu à
                // l'ouverture (repos) ne déclenche donc pas le clavier.
                if (hat ? ev.value != 0
                        : stick_raw_mag(&lx, &ly, &rx, &ry) >= DEAD_ZONE)
                    notify_input();
            }
        }

        double current = now_seconds();
        double dt = current - last_time;
        last_time = current;
        if (dt > 0.1) dt = 0.1;

        double lnx = lx.present ? normalize_axis(lx.value, lx.info.minimum, lx.info.maximum) : 0.0;
        double lny = ly.present ? normalize_axis(ly.value, ly.info.minimum, ly.info.maximum) : 0.0;
        double rnx = rx.present ? normalize_axis(rx.value, rx.info.minimum, rx.info.maximum) : 0.0;
        double rny = ry.present ? normalize_axis(ry.value, ry.info.minimum, ry.info.maximum) : 0.0;

        double lmag = sqrt(lnx * lnx + lny * lny);
        double rmag = sqrt(rnx * rnx + rny * rny);
        double norm_x = (lmag >= rmag) ? lnx : rnx;
        double norm_y = (lmag >= rmag) ? lny : rny;
        double raw = (lmag >= rmag) ? lmag : rmag;

        double magnitude = 0.0, dir_x = 0.0, dir_y = 0.0;
        if (raw >= DEAD_ZONE) {
            if (raw > 0.001) { dir_x = norm_x / raw; dir_y = norm_y / raw; }
            magnitude = (raw - DEAD_ZONE) / (1.0 - DEAD_ZONE);
        }

        if (magnitude > 0.001) {
            double curve = apply_easing_quadratic_curve(magnitude);
            double speed = BASE_SENSITIVITY + (MAX_SENSITIVITY - BASE_SENSITIVITY) * curve;
            carry_x += dir_x * speed * dt * 60.0;
            carry_y += dir_y * speed * dt * 60.0;
            int ix = (int)llround(carry_x);
            int iy = (int)llround(carry_y);
            carry_x -= ix;
            carry_y -= iy;
            if (ix) emit(EV_REL, REL_X, ix);
            if (iy) emit(EV_REL, REL_Y, iy);
            /* Le pointeur avance : le stick est maintenu, même sans nouvel
             * événement evdev — c'est de l'activité pour le pont. */
            if (ix || iy) { sync_ev(); notify_input(); }
        }
    }

out:
    ioctl(ufd, UI_DEV_DESTROY, 0);
    close(ufd);
    close(pad);
    return 0;
}
