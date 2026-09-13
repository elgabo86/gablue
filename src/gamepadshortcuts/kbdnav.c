/*
 * kbdnav : pont manette -> clavier virtuel Plasma Keyboard
 *
 * Permet de taper au clavier visuel avec la manette de jeu (navigation
 * au D-pad/stick, validation boutons). Le clavier virtuel KDE officiel
 * (plasma-keyboard, input method Wayland) est piloté via son mode
 * "navigation clavier" (KCM, prévu pour Bigscreen/accessibilité) :
 * le pont injecte les touches flèches/Entrée/Echap via uinput.
 *
 * Cycle de vie (100 % auto-porté, lancé par gamepadshortcuts via
 * Home+Carré) :
 *   1. Grab exclusif de la manette (EVIOCGRAB) -> plus aucun input vers
 *      les jeux/applications pendant la frappe
 *   2. Affichage du clavier (KWin : mode AnyInput + forceActivate)
 *   3. Frappe -> flèches/Entrée/Echap/Backspace/Espace/Tab injectés
 *   4. Sortie (Home+Carré, Home+R3 -> mode souris, fermeture via l'UI du
 *      clavier, déconnexion) -> masquage + retour au mode défaut
 *      (NonMouseInput) + redémarrage de l'IM + tout relâché
 *
 * Quarantaine de frappe (poll D-Bus 250 ms) : les touches ne sont
 * injectées que si l'input method est active ET le panneau visible
 * (sinon elles seraient re-transmises à l'app focus au lieu du clavier).
 *
 * Multi-session Wayland : suivi du VT actif (inotify sur
 * /sys/class/tty/tty0/active, même mécanique que gamepadshortcuts).
 * VT inactif -> relâchement du grab et pause complète, pour que
 * l'instance gamepadshortcuts de l'autre session retrouve la manette.
 *
 * LIMITATION NOYAU (vérifiée dans les sources) : il n'existe aucun
 * mécanisme pour couper des lecteurs hidraw existants (HIDIOCGRAB n'a
 * jamais existé, HIDIOCREVOKE ne révoque que le fd appelant). Les apps
 * lisant la manette via /dev/hidraw (SDL avec driver hidapi : DualSense/
 * Switch Pro, Steam) continuent donc de recevoir le pad pendant la
 * frappe. Le grab evdev couvre tous les lecteurs evdev (ds2xbox,
 * gamepadshortcuts, SDL sans hidapi, Chromium gamepad).
 *
 * Build : voir Makefile (gcc, aucune dépendance externe)
 */

#define _POSIX_C_SOURCE 200809L
#define _DEFAULT_SOURCE

#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/inotify.h>
#include <sys/ioctl.h>
#include <sys/poll.h>
#include <time.h>
#include <unistd.h>

#include <linux/input.h>
#include <linux/uinput.h>

#define VID_SONY 0x054c

/* Répétition : 400 ms de délai puis 60 ms entre répétitions */
#define REPEAT_DELAY_MS 400
#define REPEAT_PERIOD_MS 60
#define POLL_TIMEOUT_MS 30

/* Fermeture du clavier via son UI : sortie du pont après ce délai de
   confirmation (filtre les micro-coupures transitoires du panneau ;
   le poll passe à 100 ms pendant la confirmation pour rester réactif) */
#define EXIT_AFTER_CLOSED_MS 500

static volatile bool running = true;

static int gamepad_fd = -1;
static int uinput_fd = -1;
static bool grabbed = false;
static bool opt_grab = true;

static bool home_held = false;
static bool keys_allowed = false;

/* Plage réelle des axes (normalisation, cf. gamepadshortcuts.c) */
static int abs_x_min = 0, abs_x_max = 255;
static int abs_y_min = 0, abs_y_max = 255;

/* Touches maintenues avec auto-repeat manuel */
struct held_key {
    int code;
    bool held;
    long next_repeat_ns;
};
static struct held_key held_keys[8] = {0};

static void signal_handler(int sig)
{
    (void)sig;
    running = false;
}

static long now_ns(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000000000L + ts.tv_nsec;
}

/* =========================================================================
 * UINPUT : création du clavier virtuel
 * ========================================================================= */

static const int mapped_keys[] = {
    KEY_LEFT, KEY_RIGHT, KEY_UP, KEY_DOWN,
    KEY_ENTER, KEY_ESC, KEY_BACKSPACE, KEY_SPACE, KEY_TAB,
};

static int uinput_create(void)
{
    int fd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);
    if (fd < 0) {
        perror("[ERREUR] open /dev/uinput");
        return -1;
    }

    if (ioctl(fd, UI_SET_EVBIT, EV_KEY) < 0 ||
        ioctl(fd, UI_SET_EVBIT, EV_SYN) < 0 ||
        ioctl(fd, UI_SET_EVBIT, EV_REP) < 0) {
        perror("[ERREUR] UI_SET_EVBIT");
        close(fd);
        return -1;
    }

    for (size_t i = 0; i < sizeof(mapped_keys) / sizeof(mapped_keys[0]); i++) {
        if (ioctl(fd, UI_SET_KEYBIT, mapped_keys[i]) < 0) {
            perror("[ERREUR] UI_SET_KEYBIT");
            close(fd);
            return -1;
        }
    }

    struct uinput_setup usetup = {0};
    usetup.id.bustype = BUS_USB;
    usetup.id.vendor = 0x1337;
    usetup.id.product = 0x4b44; /* "KD" */
    strncpy((char *)usetup.name, "kbdnav", UINPUT_MAX_NAME_SIZE - 1);

    if (ioctl(fd, UI_DEV_SETUP, &usetup) < 0 || ioctl(fd, UI_DEV_CREATE, 0) < 0) {
        perror("[ERREUR] création device uinput");
        close(fd);
        return -1;
    }

    fprintf(stderr, "[INFO] Clavier virtuel 'kbdnav' créé\n");
    return fd;
}

static void emit_event(int type, int code, int value)
{
    if (uinput_fd < 0)
        return;

    struct input_event ev = {0};
    ev.type = type;
    ev.code = code;
    ev.value = value;

    if (write(uinput_fd, &ev, sizeof(ev)) < 0)
        perror("[WARN] write uinput");
}

static void syn(void)
{
    emit_event(EV_SYN, SYN_REPORT, 0);
}

/* Appui simple : press + release immédiats */
static void tap_key(int code)
{
    emit_event(EV_KEY, code, 1);
    syn();
    emit_event(EV_KEY, code, 0);
    syn();
}

/* Maintien avec auto-repeat manuel */
static void hold_key(int code)
{
    for (size_t i = 0; i < sizeof(held_keys) / sizeof(held_keys[0]); i++) {
        if (!held_keys[i].held) {
            held_keys[i].code = code;
            held_keys[i].held = true;
            held_keys[i].next_repeat_ns = now_ns() + REPEAT_DELAY_MS * 1000000L;
            break;
        }
    }
    emit_event(EV_KEY, code, 1);
    syn();
}

static void unhold_key(int code)
{
    for (size_t i = 0; i < sizeof(held_keys) / sizeof(held_keys[0]); i++) {
        if (held_keys[i].held && held_keys[i].code == code) {
            held_keys[i].held = false;
            break;
        }
    }
    emit_event(EV_KEY, code, 0);
    syn();
}

/* Entrée pressée SANS auto-repeat : appui court = clic sur la touche
   surlignée, maintien = long-press (>= 600 ms, diacriticsHoldThresholdMs)
   qui déclenche la popup d'accents côté plasma-keyboard. Une fois la
   popup ouverte, flèches + Entrée sélectionnent, Échap ferme
   (OverlayController upstream redirige ces touches vers l'overlay). */
static bool enter_held = false;

static void press_enter(void)
{
    if (enter_held)
        return;
    enter_held = true;
    emit_event(EV_KEY, KEY_ENTER, 1);
    syn();
}

static void release_enter(void)
{
    if (!enter_held)
        return;
    enter_held = false;
    emit_event(EV_KEY, KEY_ENTER, 0);
    syn();
}

/* Répétition des touches maintenues (appelé dans la boucle poll) */
static void repeat_pass(void)
{
    if (!keys_allowed)
        return;

    long now = now_ns();
    for (size_t i = 0; i < sizeof(held_keys) / sizeof(held_keys[0]); i++) {
        if (held_keys[i].held && now >= held_keys[i].next_repeat_ns) {
            emit_event(EV_KEY, held_keys[i].code, 0);
            syn();
            emit_event(EV_KEY, held_keys[i].code, 1);
            syn();
            held_keys[i].next_repeat_ns = now + REPEAT_PERIOD_MS * 1000000L;
        }
    }
}

static void release_all(void)
{
    release_enter();
    for (size_t i = 0; i < sizeof(held_keys) / sizeof(held_keys[0]); i++) {
        if (held_keys[i].held) {
            emit_event(EV_KEY, held_keys[i].code, 0);
            held_keys[i].held = false;
        }
    }
    syn();
}

/* =========================================================================
 * MANETTE : détection (même logique que gamepadshortcuts.c)
 * ========================================================================= */

#define MAX_KEY_BITS (KEY_MAX + 1)

static bool has_key_bit(const unsigned long *bits, int code)
{
    return bits[code / (8 * sizeof(unsigned long))]
           & (1UL << (code % (8 * sizeof(unsigned long))));
}

static int test_gamepad_candidate(const char *path, bool sony_only)
{
    int fd = open(path, O_RDONLY | O_NONBLOCK);
    if (fd < 0)
        return -1;

    unsigned long key_bits[MAX_KEY_BITS / (8 * sizeof(unsigned long))] = {0};
    if (ioctl(fd, EVIOCGBIT(EV_KEY, sizeof(key_bits)), key_bits) < 0) {
        close(fd);
        return -1;
    }

    if (!has_key_bit(key_bits, BTN_A) && !has_key_bit(key_bits, BTN_SELECT)) {
        close(fd);
        return -1;
    }

    char name[256] = {0};
    ioctl(fd, EVIOCGNAME(sizeof(name)), name);

    if (sony_only) {
        struct input_id id;
        if (ioctl(fd, EVIOCGID, &id) < 0 || id.vendor != VID_SONY) {
            close(fd);
            return -1;
        }
        if (strstr(name, "Touchpad") || strstr(name, "Motion")
            || strstr(name, "Headset") || strstr(name, "Jack")) {
            close(fd);
            return -1;
        }
    }

    struct input_absinfo absinfo;
    if (ioctl(fd, EVIOCGABS(ABS_X), &absinfo) == 0) {
        abs_x_min = absinfo.minimum;
        abs_x_max = absinfo.maximum;
    } else {
        abs_x_min = 0;
        abs_x_max = 255;
    }
    if (ioctl(fd, EVIOCGABS(ABS_Y), &absinfo) == 0) {
        abs_y_min = absinfo.minimum;
        abs_y_max = absinfo.maximum;
    } else {
        abs_y_min = 0;
        abs_y_max = 255;
    }

    fprintf(stderr, "[INFO] Manette trouvée : %s (%s)\n", name, path);
    return fd;
}

static int find_gamepad(void)
{
    /* Passe 1 : manette Sony physique (prioritaire, évite la manette
       virtuelle "Xbox 360" de ds2xbox) — Passe 2 : n'importe quel pad */
    for (int pass = 0; pass < 2; pass++) {
        bool sony_only = (pass == 0);

        DIR *dir = opendir("/dev/input");
        if (!dir)
            return -1;

        struct dirent *ent;
        char path[512];
        while ((ent = readdir(dir)) != NULL) {
            if (strncmp(ent->d_name, "event", 5) != 0)
                continue;
            snprintf(path, sizeof(path), "/dev/input/%s", ent->d_name);
            int fd = test_gamepad_candidate(path, sony_only);
            if (fd >= 0) {
                closedir(dir);
                return fd;
            }
        }
        closedir(dir);
    }
    return -1;
}

/* Grab evdev exclusif : coupe tous les lecteurs evdev (ds2xbox,
   gamepadshortcuts, jeux SDL sans hidapi, Chromium gamepad). */
static void set_grab(bool on)
{
    if (gamepad_fd < 0 || !opt_grab)
        return;

    if (on && !grabbed) {
        if (ioctl(gamepad_fd, EVIOCGRAB, 1) == 0) {
            grabbed = true;
        } else {
            perror("[WARN] grab manette impossible (déjà grabbée ?)");
        }
    } else if (!on && grabbed) {
        ioctl(gamepad_fd, EVIOCGRAB, 0);
        grabbed = false;
    }
}

/* =========================================================================
 * SUIVI DU VT ACTIF (inotify sur /sys/class/tty/tty0/active)
 * Pendant que le pont grabbe la manette, l'instance gamepadshortcuts
 * d'une AUTRE session doit pouvoir l'utiliser : VT inactif -> relâchement
 * du grab et pause complète du pont.
 * ========================================================================= */

static int my_vt = -1;
static bool vt_active = true;
static int inotify_vt_fd = -1;
static int inotify_vt_wd = -1;
static int tty0_fd = -1;

static int read_active_vt(void)
{
    char buf[16];
    ssize_t n = pread(tty0_fd, buf, sizeof(buf) - 1, 0);
    if (n < 3)
        return -1;
    buf[n] = '\0';
    if (strncmp(buf, "tty", 3) != 0)
        return -1;
    return atoi(buf + 3);
}

static int setup_vt_tracking(void)
{
    char *vt_str = getenv("XDG_VTNR");
    if (!vt_str) {
        fprintf(stderr, "[INFO] XDG_VTNR non défini, fonctionnement sans filtrage VT\n");
        return -1;
    }

    my_vt = atoi(vt_str);
    fprintf(stderr, "[INFO] Session démarrée sur VT %d\n", my_vt);

    tty0_fd = open("/sys/class/tty/tty0/active", O_RDONLY | O_CLOEXEC);
    if (tty0_fd < 0) {
        my_vt = -1;
        return -1;
    }

    inotify_vt_fd = inotify_init1(IN_CLOEXEC | IN_NONBLOCK);
    if (inotify_vt_fd < 0) {
        close(tty0_fd);
        tty0_fd = -1;
        my_vt = -1;
        return -1;
    }

    inotify_vt_wd = inotify_add_watch(inotify_vt_fd,
                                       "/sys/class/tty/tty0/active",
                                       IN_MODIFY);
    if (inotify_vt_wd < 0) {
        close(inotify_vt_fd);
        close(tty0_fd);
        inotify_vt_fd = -1;
        tty0_fd = -1;
        my_vt = -1;
        return -1;
    }

    return 0;
}

static void cleanup_vt_tracking(void)
{
    if (inotify_vt_wd >= 0) {
        inotify_rm_watch(inotify_vt_fd, inotify_vt_wd);
        inotify_vt_wd = -1;
    }
    if (inotify_vt_fd >= 0) {
        close(inotify_vt_fd);
        inotify_vt_fd = -1;
    }
    if (tty0_fd >= 0) {
        close(tty0_fd);
        tty0_fd = -1;
    }
}

static void check_vt_activity(void)
{
    if (my_vt < 0 || tty0_fd < 0)
        return;

    int active = read_active_vt();
    if (active < 0)
        return;

    bool was = vt_active;
    vt_active = (my_vt == active);

    if (!was && vt_active) {
        fprintf(stderr, "[INFO] VT %d actif : reprise du pont\n", my_vt);
        release_all();
        home_held = false;
        set_grab(true);
    } else if (was && !vt_active) {
        fprintf(stderr, "[INFO] VT %d inactif : pause du pont (manette relâchée)\n", my_vt);
        release_all();
        home_held = false;
        set_grab(false);
    }
}

/* =========================================================================
 * ÉTAT IM (poll D-Bus 250 ms) : quarantaine + détection de fermeture
 *
 * active  : contexte IM actif (un champ texte a le focus)
 * visible : panneau OSK affiché
 *
 * - Frappe autorisée seulement si active ET visible (sinon les touches
 *   seraient re-transmises à l'app focus au lieu du clavier).
 * - active ET !visible de façon persistante = l'utilisateur a fermé le
 *   clavier via son UI -> sortie du pont (sinon il resterait grabé).
 * - !active + !visible = focus hors champ texte -> quarantaine simple,
 *   le pont reste (pad grabé muet), re-focus -> tout reprend.
 * ========================================================================= */

static long invisible_since = 0;
static bool ever_visible = false;

static void poll_im_state(void)
{
    static long last_check = 0;
    long now = now_ns();
    /* 250 ms en veille, 100 ms pendant la confirmation de fermeture
       (invisible_since actif) pour une sortie réactive */
    long interval = (invisible_since != 0) ? 100000000L : 250000000L;
    if (now - last_check < interval)
        return;
    last_check = now;

    bool act = false, vis = false;
    FILE *fp = popen("busctl --user get-property org.kde.KWin /VirtualKeyboard "
                     "org.kde.kwin.VirtualKeyboard active visible 2>/dev/null", "r");
    if (fp) {
        char line[64];
        int idx = 0;
        while (fgets(line, sizeof(line), fp) && idx < 2) {
            bool is_true = strstr(line, "true") != NULL;
            if (idx == 0)
                act = is_true;
            else
                vis = is_true;
            idx++;
        }
        pclose(fp);
    } else {
        act = true;
        vis = true; /* en cas de doute, injecter */
    }

    /* Quarantaine de frappe */
    bool allowed = act && vis;
    if (allowed != keys_allowed) {
        keys_allowed = allowed;
        if (!allowed) {
            release_all();
            fprintf(stderr, "[INFO] Frappe suspendue (panneau caché ou focus hors champ)\n");
        } else {
            fprintf(stderr, "[INFO] Frappe autorisée\n");
        }
    }

    /* Détection de fermeture par l'UI du clavier */
    if (vis) {
        ever_visible = true;
        invisible_since = 0;
    } else if (ever_visible && act) {
        if (invisible_since == 0) {
            invisible_since = now;
        } else if (now - invisible_since >= EXIT_AFTER_CLOSED_MS * 1000000L) {
            fprintf(stderr, "[INFO] Clavier fermé (UI) : sortie du pont\n");
            running = false;
            return;
        }
    }
    /* act=false + vis=false : focus hors champ texte -> le pont reste,
       pad grabé muet (quarantaine), re-focus sur un champ -> tout reprend */
}

/* Prévient gamepadshortcuts (si c'est notre parent) qu'il doit lancer le
   mode souris après notre sortie — il est aveugle pendant notre grab et ne
   verrait jamais le Home+R3. Le handler côté gamepadshortcuts pose un
   flag, le lancement est différé dans sa boucle principale. */
static void notify_parent_mouse(void)
{
    pid_t ppid = getppid();
    char path[64], comm[64] = {0};
    snprintf(path, sizeof(path), "/proc/%d/comm", ppid);
    FILE *f = fopen(path, "r");
    if (!f)
        return;
    size_t n = fread(comm, 1, sizeof(comm) - 1, f);
    fclose(f);
    if (n > 0 && strncmp(comm, "gamepadshortc", 13) == 0) {
        kill(ppid, SIGUSR1);
        fprintf(stderr, "[INFO] SIGUSR1 -> gamepadshortcuts (lancement souris)\n");
    }
}

/* =========================================================================
 * NAVIGATION : stick + D-pad -> flèches
 * ========================================================================= */

static int hat_x = 0, hat_y = 0;
static double stick_x = 0.0, stick_y = 0.0;
static int cur_dx = 0, cur_dy = 0; /* direction courante */

static double normalize_axis(int value, int vmin, int vmax)
{
    if (vmax <= vmin)
        return 0.0;
    double norm = 2.0 * (value - vmin) / (vmax - vmin) - 1.0;
    if (norm < -1.0)
        norm = -1.0;
    if (norm > 1.0)
        norm = 1.0;
    return norm;
}

/* Met à jour les flèches pressées selon la direction voulue */
static void update_arrows(void)
{
    /* Quarantaine : relâcher les directions maintenues et ne rien presser */
    if (!keys_allowed) {
        if (cur_dx || cur_dy) {
            if (cur_dx == -1) unhold_key(KEY_LEFT);
            if (cur_dx == 1) unhold_key(KEY_RIGHT);
            if (cur_dy == -1) unhold_key(KEY_UP);
            if (cur_dy == 1) unhold_key(KEY_DOWN);
            cur_dx = cur_dy = 0;
        }
        return;
    }

    /* Le D-pad est prioritaire sur le stick */
    int dx = (hat_x != 0) ? hat_x : (stick_x > 0.5 ? 1 : (stick_x < -0.5 ? -1 : 0));
    int dy = (hat_y != 0) ? hat_y : (stick_y > 0.5 ? 1 : (stick_y < -0.5 ? -1 : 0));

    if (dx == cur_dx && dy == cur_dy)
        return;

    /* Relâcher les directions qui changent */
    if ((dx == 0 || dx == 1) && cur_dx == -1)
        unhold_key(KEY_LEFT);
    if ((dx == 0 || dx == -1) && cur_dx == 1)
        unhold_key(KEY_RIGHT);
    if ((dy == 0 || dy == 1) && cur_dy == -1)
        unhold_key(KEY_UP);
    if ((dy == 0 || dy == -1) && cur_dy == 1)
        unhold_key(KEY_DOWN);

    /* Presser les nouvelles */
    if (dx == -1 && cur_dx != -1)
        hold_key(KEY_LEFT);
    if (dx == 1 && cur_dx != 1)
        hold_key(KEY_RIGHT);
    if (dy == -1 && cur_dy != -1)
        hold_key(KEY_UP);
    if (dy == 1 && cur_dy != 1)
        hold_key(KEY_DOWN);

    cur_dx = dx;
    cur_dy = dy;
}

/* =========================================================================
 * ÉVÉNEMENTS MANETTE
 * ========================================================================= */

static void handle_event(struct input_event *ev)
{
    if (!vt_active)
        return;

    /* Home et Start restent trackés même en quarantaine (raccourcis de sortie) */
    if (ev->type == EV_KEY && ev->code == BTN_MODE) {
        home_held = (ev->value == 1);
        return;
    }

    if (ev->type == EV_KEY && ev->code == BTN_START) {
        if (ev->value == 1) {
            fprintf(stderr, "[INFO] Start : sortie du pont\n");
            running = false;
        }
        return;
    }

    /* Quarantaine : on n'injecte rien hors frappe autorisée */
    if (!keys_allowed)
        return;

    if (ev->type == EV_KEY) {
        switch (ev->code) {
        case BTN_SOUTH: /* A / Croix : Entrée — appui court = valider la
                           touche surlignée, maintien = popup d'accents */
            if (ev->value == 1) {
                if (!home_held)
                    press_enter();
            } else {
                release_enter();
            }
            break;

        case BTN_EAST: /* B / Rond : Échap */
            if (ev->value == 1 && !home_held)
                tap_key(KEY_ESC);
            break;

        case BTN_WEST: /* Carré : Home+Carré = quitter, sinon Backspace */
            if (ev->value == 1) {
                if (home_held) {
                    fprintf(stderr, "[INFO] Home+Carré : sortie du pont\n");
                    running = false;
                } else {
                    hold_key(KEY_BACKSPACE);
                }
            } else {
                unhold_key(KEY_BACKSPACE);
            }
            break;

        case BTN_NORTH: /* Triangle : Espace */
            if (ev->value == 1 && !home_held) {
                hold_key(KEY_SPACE);
            } else {
                unhold_key(KEY_SPACE);
            }
            break;

        case BTN_TL: /* L1 : Tab */
        case BTN_TR: /* R1 : Tab */
            if (ev->value == 1 && !home_held)
                tap_key(KEY_TAB);
            break;

        case BTN_THUMBR: /* R3 : Home+R3 = sortir et passer en mode souris */
            if (ev->value == 1 && home_held) {
                fprintf(stderr, "[INFO] Home+R3 : sortie du pont -> mode souris\n");
                notify_parent_mouse();
                running = false;
            }
            break;

        default:
            break;
        }

    } else if (ev->type == EV_ABS) {
        switch (ev->code) {
        case ABS_HAT0X:
            hat_x = ev->value;
            update_arrows();
            break;
        case ABS_HAT0Y:
            hat_y = ev->value;
            update_arrows();
            break;
        case ABS_X:
            stick_x = normalize_axis(ev->value, abs_x_min, abs_x_max);
            update_arrows();
            break;
        case ABS_Y:
            stick_y = normalize_axis(ev->value, abs_y_min, abs_y_max);
            update_arrows();
            break;
        default:
            break;
        }
    }
}

/* =========================================================================
 * PROGRAMME PRINCIPAL
 * ========================================================================= */

static void cleanup(void)
{
    release_all();

    if (gamepad_fd >= 0 && grabbed) {
        ioctl(gamepad_fd, EVIOCGRAB, 0);
        fprintf(stderr, "[INFO] Manette relâchée (evdev)\n");
    }
    if (uinput_fd >= 0) {
        ioctl(uinput_fd, UI_DEV_DESTROY, 0);
        close(uinput_fd);
    }
    if (gamepad_fd >= 0)
        close(gamepad_fd);

    cleanup_vt_tracking();

    /* Masquer le clavier à la sortie du pont + remise au mode par défaut.
       Deux setMode obligatoires, dans cet ordre :
       - setMode(Never 0) : KWin appelle hide() -> masque le panneau ET
         reset m_showRequested/m_forceShowRequested ; sans ce reset, le
         show() inconditionnel de setPanel() ré-afficherait le panneau
         re-mappé après le redémarrage de l'IM.
       - setMode(NonMouseInput 1, "Touch and Tablet", défaut Kinoite) :
         état final. Contrairement à Never, l'applet OSK du system tray
         (manage-inputmethod) reste cachée (en Never elle passe en
         ActiveStatus : icône en permanence dans la barre des tâches),
         et la config persistée par KWin dans kwinrc
         ([Wayland] VirtualKeyboardMode) revient au défaut : le panneau
         ne peut réapparaître qu'au toucher/tablette, jamais au
         clavier/souris/manette.
       Redémarrage de l'IM (kill) : repart d'un panneau non mappé,
       incapable de réapparaître spontanément. KWin relance
       plasma-keyboard automatiquement sur crash. */
    int rc = system("busctl --user set-property org.kde.KWin /VirtualKeyboard "
                    "org.kde.kwin.VirtualKeyboard mode i 0 2>/dev/null && "
                    "busctl --user set-property org.kde.KWin /VirtualKeyboard "
                    "org.kde.kwin.VirtualKeyboard mode i 1 2>/dev/null");
    (void)rc;
    rc = system("pkill -9 -x plasma-keyboard 2>/dev/null");
    (void)rc;
}

int main(int argc, char **argv)
{
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--no-grab") == 0)
            opt_grab = false;
        else if (strcmp(argv[i], "--find") == 0) {
            int fd = find_gamepad();
            if (fd >= 0) {
                close(fd);
                return 0;
            }
            fprintf(stderr, "[ERREUR] Aucune manette détectée\n");
            return 1;
        } else {
            fprintf(stderr, "Usage : %s [--no-grab] [--find]\n", argv[0]);
            return 1;
        }
    }

    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

    setup_vt_tracking();
    check_vt_activity();

    gamepad_fd = find_gamepad();
    if (gamepad_fd < 0) {
        fprintf(stderr, "[ERREUR] Aucune manette détectée — relancer après branchement\n");
        cleanup_vt_tracking();
        return 1;
    }

    if (vt_active)
        set_grab(true);

    uinput_fd = uinput_create();
    if (uinput_fd < 0) {
        cleanup();
        return 1;
    }

    fprintf(stderr, "[INFO] Pont actif — mappings : D-pad/stick=flèches  A=Entrée (maintien=accents)  "
                    "B=Échap  Carré=Backspace  Triangle=Espace  L1/R1=Tab  Start=quitter  "
                    "Home+Carré=quitter  Home+R3=mode souris  fermeture UI du clavier=quitter\n");

    /* Afficher le clavier (mode AnyInput + forceActivate). Le masquage et
       la remise au mode défaut (NonMouseInput) se font dans cleanup() —
       cycle de vie 100 % auto-porté : gamepadshortcuts lance juste le
       binaire via Home+Carré. */
    int rc = system("busctl --user set-property org.kde.KWin /VirtualKeyboard "
                    "org.kde.kwin.VirtualKeyboard mode i 2 2>/dev/null && "
                    "qdbus org.kde.KWin /VirtualKeyboard "
                    "org.kde.kwin.VirtualKeyboard.forceActivate 2>/dev/null");
    (void)rc;

    struct pollfd pfds[2];
    struct input_event ev;

    while (running) {
        int nfds = 0;
        pfds[nfds].fd = gamepad_fd;
        pfds[nfds].events = POLLIN;
        pfds[nfds].revents = 0;
        nfds++;

        if (inotify_vt_fd >= 0) {
            pfds[nfds].fd = inotify_vt_fd;
            pfds[nfds].events = POLLIN;
            pfds[nfds].revents = 0;
            nfds++;
        }

        int ret = poll(pfds, nfds, POLL_TIMEOUT_MS);
        if (ret < 0) {
            if (errno == EINTR)
                continue;
            fprintf(stderr, "[ERREUR] poll : %s\n", strerror(errno));
            break;
        }

        /* Switch VT */
        if (nfds >= 2 && (pfds[1].revents & POLLIN)) {
            char ino_buf[4096];
            while (read(inotify_vt_fd, ino_buf, sizeof(ino_buf)) > 0) {}
            check_vt_activity();
        }

        /* Déconnexion manette -> sortie propre (l'utilisateur relancera) */
        if (pfds[0].revents & (POLLERR | POLLHUP | POLLNVAL)) {
            fprintf(stderr, "[INFO] Manette déconnectée : sortie du pont\n");
            break;
        }

        /* Événements manette */
        if (ret > 0 && (pfds[0].revents & POLLIN)) {
            while (true) {
                ssize_t bytes = read(gamepad_fd, &ev, sizeof(ev));
                if (bytes < 0) {
                    if (errno == EAGAIN || errno == EWOULDBLOCK)
                        break;
                    fprintf(stderr, "[ERREUR] lecture manette : %s\n", strerror(errno));
                    running = false;
                    break;
                }
                if (bytes == sizeof(ev))
                    handle_event(&ev);
                if (bytes <= 0)
                    break;
            }
        }

        if (vt_active)
            poll_im_state();

        if (running && vt_active)
            repeat_pass();
    }

    cleanup();
    fprintf(stderr, "[INFO] Pont terminé\n");
    return 0;
}
