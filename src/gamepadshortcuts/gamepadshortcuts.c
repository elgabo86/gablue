#define _POSIX_C_SOURCE 200809L
#define _DEFAULT_SOURCE

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <signal.h>
#include <dirent.h>
#include <stdbool.h>
#include <time.h>
#include <sys/ioctl.h>
#include <sys/inotify.h>
#include <sys/poll.h>
#include <sys/wait.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <pthread.h>
#include <dbus/dbus.h>
#include <linux/input.h>

#define SCRIPTS_DIR "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts"
#define VOLUME_COOLDOWN_NS 200000000L
#define HAT_COOLDOWN_NS    300000000L
#define MAX_KEY_BITS (KEY_MAX + 1)

static volatile bool running = true;

/* Remplace systemd-inhibit (6.2 Mo) par D-Bus via libdbus (+200 Ko PSS) */
static DBusConnection *dbus_conn = NULL;
static dbus_uint32_t inhibit_cookie = 0;
static bool inhibited = false;

static bool home_pressed = false;
static bool select_pressed = false;
static bool start_pressed = false;
static bool triangle_pressed = false;
static bool square_pressed = false;
static bool circle_pressed = false;
static bool l3_pressed = false;
static bool r3_pressed = false;

static int hat_x = 0;
static int hat_y = 0;
static int last_hat_x = 0;
static int last_hat_y = 0;
static double axis_y = 0.0;
static double last_axis_y = 0.0;

static bool mouse_running = false;
static pid_t mouse_pid = -1;
static bool menuvsr_running = false;
static pid_t menuvsr_pid = -1;
static struct timespec last_volume_time = {0, 0};
static struct timespec last_hat_time = {0, 0};

/* Plage reelle de ABS_Y de la manette connectee, relue a chaque connexion.
   DualSense USB/BT: 0-255. "Xbox 360 Controller" virtuel (ds2xbox): -32768..32767.
   Sans cette normalisation, la valeur de repos du device virtuel (0) etait
   interpretee comme -1.008 -> volume monte en boucle quand Home est presse. */
static int abs_y_minimum = 0;
static int abs_y_maximum = 255;

/* Normalise une valeur ABS_Y brute vers [-1.0, 1.0] selon la plage reelle
   du peripherique courant (0.0 au repos quel que soit le device) */
static double normalize_axis_y(int value)
{
    if (abs_y_maximum <= abs_y_minimum)
        return 0.0;

    double norm = 2.0 * (value - abs_y_minimum)
                  / (abs_y_maximum - abs_y_minimum) - 1.0;
    if (norm < -1.0)
        norm = -1.0;
    if (norm > 1.0)
        norm = 1.0;
    return norm;
}

/* =========================================================================
 * SUIVI DU VT ACTIF (inotify sur /sys/class/tty/tty0/active)
 * Permet a deux sessions Wayland simultanees de ne pas entrer en conflit
 * ========================================================================= */
static int my_vt = -1;
static bool vt_active = true;
static int inotify_vt_fd = -1;
static int inotify_vt_wd = -1;
static int tty0_fd = -1;

static void signal_handler(int sig)
{
    (void)sig;
    running = false;
}

/* =================================================================
 * Inhibition ecran via org.freedesktop.ScreenSaver (libdbus-1)
 * Alternative legere a systemd-inhibit (6.2 Mo)
 * Surcout reel: ~200 Ko PSS (libs deja en RAM via KDE/systemd)
 * ================================================================= */

static void inhibit_screensaver(void)
{
    if (inhibited)
        return;

    if (!dbus_conn) {
        DBusError err;
        dbus_error_init(&err);
        dbus_conn = dbus_bus_get(DBUS_BUS_SESSION, &err);
        if (dbus_error_is_set(&err)) {
            fprintf(stderr, "[WARN] Connexion DBus echouee: %s\n", err.message);
            dbus_error_free(&err);
            dbus_conn = NULL;
            return;
        }
    }

    DBusMessage *msg = dbus_message_new_method_call(
        "org.freedesktop.ScreenSaver", "/ScreenSaver",
        "org.freedesktop.ScreenSaver", "Inhibit");
    if (!msg)
        return;

    const char *app = "gablue-gamepadshortcuts";
    const char *reason = "Manette connectee";
    dbus_message_append_args(msg,
        DBUS_TYPE_STRING, &app,
        DBUS_TYPE_STRING, &reason,
        DBUS_TYPE_INVALID);

    DBusError err;
    dbus_error_init(&err);
    DBusMessage *reply = dbus_connection_send_with_reply_and_block(
        dbus_conn, msg, 2000, &err);
    dbus_message_unref(msg);

    if (dbus_error_is_set(&err) || !reply) {
        fprintf(stderr, "[INFO] Inhibition ecran indisponible: %s\n",
                err.message ? err.message : "pas de reponse");
        if (dbus_error_is_set(&err))
            dbus_error_free(&err);
        if (reply)
            dbus_message_unref(reply);
        return;
    }

    if (dbus_message_get_args(reply, &err,
        DBUS_TYPE_UINT32, &inhibit_cookie, DBUS_TYPE_INVALID)) {
        inhibited = true;
        fprintf(stderr, "[INFO] Inhibition ecran activee (cookie: %u)\n",
                inhibit_cookie);
    } else {
        fprintf(stderr, "[WARN] Cookie d'inhibition non recu: %s\n",
                err.message);
        dbus_error_free(&err);
    }
    dbus_message_unref(reply);
}

static void uninhibit_screensaver(void)
{
    inhibited = false;

    if (!dbus_conn || inhibit_cookie == 0)
        return;

    DBusMessage *msg = dbus_message_new_method_call(
        "org.freedesktop.ScreenSaver", "/ScreenSaver",
        "org.freedesktop.ScreenSaver", "UnInhibit");
    if (!msg)
        return;

    dbus_message_append_args(msg,
        DBUS_TYPE_UINT32, &inhibit_cookie, DBUS_TYPE_INVALID);

    dbus_connection_send_with_reply_and_block(dbus_conn, msg, 2000, NULL);
    dbus_message_unref(msg);

    inhibit_cookie = 0;
    fprintf(stderr, "[INFO] Inhibition ecran desactivee\n");
}

#define VID_SONY 0x054c

/* Teste un candidat /dev/input/event* : retourne le fd ouvert si le device
   convient, -1 sinon.
   sony_only : ne retenir que les manettes Sony physiques (passe prioritaire,
   pour ne pas attraper la manette virtuelle "Xbox 360" de ds2xbox) */
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

    bool has_btn_a = key_bits[BTN_A / (8 * sizeof(unsigned long))]
        & (1UL << (BTN_A % (8 * sizeof(unsigned long))));
    bool has_btn_select = key_bits[BTN_SELECT / (8 * sizeof(unsigned long))]
        & (1UL << (BTN_SELECT % (8 * sizeof(unsigned long))));

    if (!has_btn_a && !has_btn_select) {
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

        /* Exclusion des peripheriques auxiliaires exposes par hid-playstation */
        if (strstr(name, "Touchpad") || strstr(name, "Motion")
            || strstr(name, "Headset") || strstr(name, "Jack")) {
            close(fd);
            return -1;
        }
    }

    /* Lecture de la plage reelle de ABS_Y pour la normalisation
       (evite les valeurs faussees sur les devices virtuels ex. ds2xbox) */
    struct input_absinfo absinfo;
    if (ioctl(fd, EVIOCGABS(ABS_Y), &absinfo) == 0) {
        abs_y_minimum = absinfo.minimum;
        abs_y_maximum = absinfo.maximum;
    } else {
        abs_y_minimum = 0;
        abs_y_maximum = 255;
    }

    fprintf(stderr, "[INFO] Manette trouvee: %s (%s, ABS_Y %d..%d)\n",
            name, path, abs_y_minimum, abs_y_maximum);
    return fd;
}

static int find_gamepad(void)
{
    /* Passe 1 : manette Sony physique (prioritaire, evite la virtuelle ds2xbox)
       Passe 2 : n'importe quel gamepad (Xbox, 8bitdo, etc.) */
    for (int pass = 0; pass < 2; pass++) {
        bool sony_only = (pass == 0);

        DIR *dir = opendir("/dev/input");
        if (!dir) {
            perror("opendir /dev/input");
            return -1;
        }

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

static pid_t launch_script(const char *script, bool background)
{
    pid_t pid = fork();
    if (pid < 0) {
        perror("fork launch_script");
        return -1;
    }
    if (pid == 0) {
        if (!background)
            close(STDIN_FILENO);
        execl("/bin/bash", "bash", script, (char *)NULL);
        _exit(1);
    }
    return pid;
}

static pid_t launch_python_script(const char *script)
{
    pid_t pid = fork();
    if (pid < 0) {
        perror("fork launch_python");
        return -1;
    }
    if (pid == 0) {
        execl("/usr/bin/python3", "python3", script, (char *)NULL);
        _exit(1);
    }
    return pid;
}

static void launch_shell_cmd(const char *cmd)
{
    pid_t pid = fork();
    if (pid < 0) {
        perror("fork shell_cmd");
        return;
    }
    if (pid == 0) {
        execl("/bin/sh", "sh", "-c", cmd, (char *)NULL);
        _exit(1);
    }
}

static void check_child(pid_t *pid, bool *running_flag, const char *name)
{
    if (*running_flag && *pid > 0) {
        int status;
        pid_t ret = waitpid(*pid, &status, WNOHANG);
        if (ret > 0) {
            fprintf(stderr, "[INFO] %s termine.\n", name);
            *running_flag = false;
            *pid = -1;
        }
    }
}

static void handle_combinations(void)
{
    if (!home_pressed)
        return;

    if (select_pressed) {
        fprintf(stderr, "[ACTION] KILL\n");
        launch_script(SCRIPTS_DIR "/killthemall", false);
        select_pressed = false;
        home_pressed = false;
    } else if (start_pressed) {
        fprintf(stderr, "[ACTION] ES\n");
        launch_script(SCRIPTS_DIR "/openes", true);
        start_pressed = false;
        home_pressed = false;
    } else if (r3_pressed && !mouse_running) {
        fprintf(stderr, "[ACTION] MOUSE\n");
        mouse_pid = launch_python_script(SCRIPTS_DIR "/mouse.py");
        mouse_running = true;
        r3_pressed = false;
    } else if (l3_pressed) {
        fprintf(stderr, "[ACTION] MUTE\n");
        launch_shell_cmd("pactl set-sink-mute @DEFAULT_SINK@ toggle");
        l3_pressed = false;
    } else if (triangle_pressed) {
        fprintf(stderr, "[ACTION] LAUNCHYT\n");
        launch_script(SCRIPTS_DIR "/launchyt", true);
        triangle_pressed = false;
    } else if (circle_pressed && !menuvsr_running) {
        fprintf(stderr, "[ACTION] MENUVR\n");
        menuvsr_pid = launch_python_script(SCRIPTS_DIR "/menuvsr.py");
        menuvsr_running = true;
        circle_pressed = false;
    }
}

static void handle_hat(void)
{
    if (!home_pressed)
        return;

    if (hat_x == last_hat_x && hat_y == last_hat_y)
        return;

    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);

    long elapsed_ns = (now.tv_sec - last_hat_time.tv_sec) * 1000000000L
                    + (now.tv_nsec - last_hat_time.tv_nsec);

    if (elapsed_ns < HAT_COOLDOWN_NS)
        goto update_hat;

    if (hat_x == -1 && hat_y == 0) {
        fprintf(stderr, "[ACTION] SCREEN\n");
        launch_script(SCRIPTS_DIR "/takescreenshot", true);
    } else if (hat_x == 1 && hat_y == 0) {
        fprintf(stderr, "[ACTION] RECORD\n");
        launch_script(SCRIPTS_DIR "/startstoprecord", true);
    } else if (hat_x == 0 && hat_y == 1) {
        fprintf(stderr, "[ACTION] FPS\n");
        launch_script(SCRIPTS_DIR "/changefps", false);
    } else if (hat_x == 0 && hat_y == -1) {
        fprintf(stderr, "[ACTION] MANGO\n");
        launch_script(SCRIPTS_DIR "/showhidemango", false);
    }

    last_hat_time = now;

update_hat:
    last_hat_x = hat_x;
    last_hat_y = hat_y;
}

static void handle_volume(void)
{
    if (!home_pressed)
        return;

    double diff_y = axis_y - last_axis_y;
    if (diff_y > -0.1 && diff_y < 0.1)
        return;

    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);

    long elapsed_ns = (now.tv_sec - last_volume_time.tv_sec) * 1000000000L
                    + (now.tv_nsec - last_volume_time.tv_nsec);

    if (elapsed_ns < VOLUME_COOLDOWN_NS)
        return;

    if (axis_y < -0.5) {
        fprintf(stderr, "[ACTION] VOLUME UP\n");
        launch_shell_cmd("pactl set-sink-volume @DEFAULT_SINK@ +10%");
        last_volume_time = now;
        last_axis_y = axis_y;
    } else if (axis_y > 0.5) {
        fprintf(stderr, "[ACTION] VOLUME DOWN\n");
        launch_shell_cmd("pactl set-sink-volume @DEFAULT_SINK@ -10%");
        last_volume_time = now;
        last_axis_y = axis_y;
    }
}

static void process_event(struct input_event *ev)
{
    if (ev->type == EV_KEY) {
        switch (ev->code) {
        case BTN_MODE:
            if (ev->value == 1) {
                home_pressed = true;
                last_axis_y = axis_y;
            } else {
                home_pressed = false;
            }
            break;
        case BTN_SELECT: select_pressed = ev->value == 1; break;
        case BTN_START:  start_pressed = ev->value == 1; break;
        case BTN_NORTH:  triangle_pressed = ev->value == 1; break;
        case BTN_WEST:   square_pressed = ev->value == 1; break;
        case BTN_EAST:   circle_pressed = ev->value == 1; break;
        case BTN_SOUTH:  break;
        case BTN_THUMBL: l3_pressed = ev->value == 1; break;
        case BTN_THUMBR: r3_pressed = ev->value == 1; break;
        default: break;
        }

        if (ev->value == 1 && home_pressed)
            handle_combinations();

    } else if (ev->type == EV_ABS) {
        switch (ev->code) {
        case ABS_HAT0X:
            hat_x = ev->value;
            handle_hat();
            break;
        case ABS_HAT0Y:
            hat_y = ev->value;
            handle_hat();
            break;
        case ABS_Y:
            axis_y = normalize_axis_y(ev->value);
            break;
        default:
            break;
        }
    }
}

/* =========================================================================
 * FONCTIONS DE SUIVI DU VT
 * ========================================================================= */

/* Lit le numero du VT actif depuis /sys/class/tty/tty0/active
   Retourne le numero (ex: 7 pour tty7), ou -1 en cas d'erreur */
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

/* Reinitialise les etats des boutons pour eviter les actions residuelles
   apres un switch VT */
static void reset_button_states(void)
{
    home_pressed = false;
    select_pressed = false;
    start_pressed = false;
    triangle_pressed = false;
    square_pressed = false;
    circle_pressed = false;
    l3_pressed = false;
    r3_pressed = false;
    hat_x = 0;
    hat_y = 0;
    last_hat_x = 0;
    last_hat_y = 0;
    axis_y = 0.0;
    last_axis_y = 0.0;
    last_hat_time = (struct timespec){0, 0};
    last_volume_time = (struct timespec){0, 0};
}

/* Verifie si notre VT est actif, met a jour vt_active et gere
   l'inhibition ecran en consequence */
static void check_vt_activity(void)
{
    if (my_vt < 0 || tty0_fd < 0)
        return;

    int active = read_active_vt();
    if (active < 0)
        return;

    bool was_active = vt_active;
    vt_active = (my_vt == active);

    if (!was_active && vt_active) {
        fprintf(stderr, "[INFO] VT %d devient actif, reprise du traitement\n", my_vt);
        inhibit_screensaver();
    } else if (was_active && !vt_active) {
        fprintf(stderr, "[INFO] VT %d devient inactif, pause du traitement\n", my_vt);
        uninhibit_screensaver();
        reset_button_states();
    }
}

/* Initialise le suivi inotify du VT.
   Retourne 0 si OK, -1 si XDG_VTNR absent (pas de filtrage) */
static int setup_vt_tracking(void)
{
    char *vt_str = getenv("XDG_VTNR");
    if (!vt_str) {
        fprintf(stderr, "[INFO] XDG_VTNR non defini, fonctionnement sans filtrage VT\n");
        return -1;
    }

    my_vt = atoi(vt_str);
    fprintf(stderr, "[INFO] Session demarree sur VT %d\n", my_vt);

    tty0_fd = open("/sys/class/tty/tty0/active", O_RDONLY | O_CLOEXEC);
    if (tty0_fd < 0) {
        perror("open /sys/class/tty/tty0/active");
        my_vt = -1;
        return -1;
    }

    inotify_vt_fd = inotify_init1(IN_CLOEXEC | IN_NONBLOCK);
    if (inotify_vt_fd < 0) {
        perror("inotify_init1");
        close(tty0_fd);
        tty0_fd = -1;
        my_vt = -1;
        return -1;
    }

    inotify_vt_wd = inotify_add_watch(inotify_vt_fd,
                                       "/sys/class/tty/tty0/active",
                                       IN_MODIFY);
    if (inotify_vt_wd < 0) {
        perror("inotify_add_watch");
        close(inotify_vt_fd);
        close(tty0_fd);
        inotify_vt_fd = -1;
        tty0_fd = -1;
        my_vt = -1;
        return -1;
    }

    /* Verification de l'etat initial */
    check_vt_activity();

    return 0;
}

/* Nettoie les ressources inotify */
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

/* =========================================================================
 * AUTO-PAIRING BLUETOOTH DES MANETTES
 *
 * Regle de fonctionnement :
 *   - Aucune manette BT connectee  -> mode pairing : decouverte active,
 *     pairable + discoverable, appairage automatique des manettes vues.
 *   - >= 1 manette BT connectee    -> decouverte et discoverable coupes
 *     (zero impact radio/latence pendant le jeu). Pairable reste actif :
 *     il ne fait qu'ecouter passivement (page scan) et est indispensable
 *     pour que les AUTRES manettes deja appairees puissent se reconnecter.
 *
 * Securite : l'agent D-Bus n'accepte un pairing que si le peripherique est
 * reconnu comme manette (classe Bluetooth gamepad/joystick ou nom connu).
 * On n'appelle PAS RequestDefaultAgent : les pairings manuels (KDE etc.)
 * gardent leur propre agent avec confirmation utilisateur.
 *
 * Multi-session : un verrou flock (/tmp/gablue-bt-autopair.lock) garantit
 * qu'une seule instance de gamepadshortcuts pilote le Bluetooth.
 * Le BT est eteint par l'utilisateur (Powered=false) -> on ne touche a rien.
 * ========================================================================= */

#define BT_LOCK_PATH       "/tmp/gablue-bt-autopair.lock"
#define BT_AGENT_PATH      "/org/gablue/btautopair"
#define BT_BLUEZ           "org.bluez"
#define BT_DEV_IFACE       "org.bluez.Device1"
#define BT_ADP_IFACE       "org.bluez.Adapter1"
#define BT_AGENT_IFACE     "org.bluez.Agent1"
#define BT_AGENT_MGR       "org.bluez.AgentManager1"
#define BT_OM_IFACE        "org.freedesktop.DBus.ObjectManager"
#define BT_PROPS_IFACE     "org.freedesktop.DBus.Properties"
#define BT_MAX_DEVICES     32
#define BT_MAX_ADAPTERS    4
#define BT_PAIR_MAX_FAILS  3
#define BT_PAIR_RETRY_S    20
#define BT_WATCHDOG_S      15

typedef struct {
    char path[192];
    char name[128];
    char alias[128];
    unsigned int cod;
    bool has_cod;
    bool paired;
    bool connected;
    bool trusted;
    time_t last_try;   /* dernier essai de pairing (anti-boucle) */
    int fails;         /* echecs consecutifs (abandon apres BT_PAIR_MAX_FAILS) */
} btdev_t;

typedef struct {
    char path[64];
    bool powered;
    bool pairable;
    bool discoverable;
    bool discovering;
    bool started_here; /* decouverte lancee par nous (pour StopDiscovery) */
} btadapt_t;

static pthread_mutex_t g_bt_lock = PTHREAD_MUTEX_INITIALIZER;
static btdev_t g_bt_devices[BT_MAX_DEVICES];
static int g_bt_ndevices;
static btadapt_t g_bt_adapters[BT_MAX_ADAPTERS];
static int g_bt_nadapters;
static volatile bool *g_bt_run;
static volatile bool g_bt_bluez_up;     /* bluetoothd present sur le bus */
static volatile bool g_bt_agent_ready;  /* agent enregistre aupres de bluez */
static volatile bool g_bt_dirty;        /* un signal demande un re-calcul */
static bool g_bt_pairing_mode = false; /* premier refresh force la transition */
static int g_bt_lock_fd = -1;
static pthread_t g_bt_manager_thread;

/* Motifs de noms Bluetooth identifies comme manettes (minuscules,
   comparaison par sous-chaine). Liste volontairement large. */
static const char *const bt_name_patterns[] = {
    "controller",      /* Wireless Controller (DS4/DS5), Pro Controller,
                          Xbox Wireless Controller, clones 8BitDo... */
    "dualsense",       /* DualSense / DualSense Edge */
    "dualshock",
    "xbox",
    "joy-con",
    "8bitdo",
    "gamesir",
    "gulikit",
    "nimbus",
    "gamepad",
};

/* Copie bornee sans warning (strncpy a ses propres warnings -Wstringop) */
static void bt_strlcpy(char *dst, size_t size, const char *src)
{
    size_t len = strlen(src);
    if (len >= size)
        len = size - 1;
    memcpy(dst, src, len);
    dst[len] = '\0';
}

/* Classe Bluetooth (CoD) : major Peripheral (0x05) + minor Joystick (0x01)
   ou Gamepad (0x02). Detecte les manettes sans nom connu. */
static bool bt_class_is_gamepad(unsigned int cod)
{
    unsigned int major = (cod >> 8) & 0x1f;
    unsigned int minor = (cod >> 2) & 0x3f;
    return major == 0x05 && (minor == 0x01 || minor == 0x02);
}

static bool bt_name_is_controller(const char *name)
{
    if (!name || !name[0])
        return false;

    char lower[160];
    size_t i;
    for (i = 0; name[i] && i < sizeof(lower) - 1; i++) {
        char cch = name[i];
        if (cch >= 'A' && cch <= 'Z')
            cch += 'a' - 'A';
        lower[i] = cch;
    }
    lower[i] = '\0';

    for (size_t p = 0; p < sizeof(bt_name_patterns) / sizeof(bt_name_patterns[0]); p++) {
        if (strstr(lower, bt_name_patterns[p]))
            return true;
    }
    return false;
}

static bool bt_dev_is_controller_locked(const btdev_t *d)
{
    if (d->has_cod && bt_class_is_gamepad(d->cod))
        return true;
    if (bt_name_is_controller(d->alias[0] ? d->alias : d->name))
        return true;
    return false;
}

/* ----- Appels D-Bus generiques (thread etat uniquement) ----- */

static DBusConnection *g_bt_conn; /* bus systeme prive du thread etat */

/* Envoie un appel bloquant, log l'erreur en mode non verbeux.
   Retourne la reponse (a libérer par l'appelant) ou NULL. */
static DBusMessage *bt_call(DBusMessage *msg, int timeout_ms, const char *what,
                            const char *path)
{
    DBusError err;
    dbus_error_init(&err);
    DBusMessage *reply = dbus_connection_send_with_reply_and_block(
        g_bt_conn, msg, timeout_ms, &err);
    dbus_message_unref(msg);
    if (dbus_error_is_set(&err)) {
        fprintf(stderr, "[BT] %s (%s): %s\n", what, path, err.message);
        dbus_error_free(&err);
        if (reply)
            dbus_message_unref(reply);
        return NULL;
    }
    return reply;
}

/* Properties.Set avec une valeur booléenne */
static void bt_props_set_bool(const char *path, const char *iface,
                              const char *prop, bool value, const char *what)
{
    DBusMessage *msg = dbus_message_new_method_call(BT_BLUEZ, path,
                                                    BT_PROPS_IFACE, "Set");
    if (!msg)
        return;

    DBusMessageIter it, var;
    dbus_bool_t v = value;
    dbus_message_iter_init_append(msg, &it);
    dbus_message_iter_append_basic(&it, DBUS_TYPE_STRING, &iface);
    dbus_message_iter_append_basic(&it, DBUS_TYPE_STRING, &prop);
    dbus_message_iter_open_container(&it, DBUS_TYPE_VARIANT, "b", &var);
    dbus_message_iter_append_basic(&var, DBUS_TYPE_BOOLEAN, &v);
    dbus_message_iter_close_container(&it, &var);

    DBusMessage *reply = bt_call(msg, 10000, what, path);
    if (reply)
        dbus_message_unref(reply);
}

/* Méthode sans argument sur un objet bluez */
static bool bt_call_void(const char *path, const char *iface,
                         const char *method, int timeout_ms, const char *what)
{
    DBusMessage *msg = dbus_message_new_method_call(BT_BLUEZ, path,
                                                    iface, method);
    if (!msg)
        return false;

    DBusMessage *reply = bt_call(msg, timeout_ms, what, path);
    if (reply) {
        dbus_message_unref(reply);
        return true;
    }
    return false;
}

/* ----- Parcours des arbres de proprietes D-Bus ----- */

typedef void (*bt_prop_cb)(const char *key, DBusMessageIter *val, void *ctx);

/* props_array pointe sur le ARRAY a{sv} (variant deja ouverte) */
static void bt_iter_props(DBusMessageIter *props_array, bt_prop_cb cb, void *ctx)
{
    if (dbus_message_iter_get_arg_type(props_array) != DBUS_TYPE_ARRAY)
        return;

    DBusMessageIter a;
    dbus_message_iter_recurse(props_array, &a);
    while (dbus_message_iter_get_arg_type(&a) == DBUS_TYPE_DICT_ENTRY) {
        DBusMessageIter de, var;
        const char *key = NULL;
        dbus_message_iter_recurse(&a, &de);
        if (dbus_message_iter_get_arg_type(&de) == DBUS_TYPE_STRING)
            dbus_message_iter_get_basic(&de, &key);
        dbus_message_iter_next(&de);
        dbus_message_iter_recurse(&de, &var); /* ouvre la variant valeur */
        if (key)
            cb(key, &var, ctx);
        dbus_message_iter_next(&a);
    }
}

static void bt_dev_prop_cb(const char *key, DBusMessageIter *val, void *ctx)
{
    btdev_t *d = ctx;
    int t = dbus_message_iter_get_arg_type(val);

    if (!strcmp(key, "Name") && t == DBUS_TYPE_STRING) {
        const char *s = NULL;
        dbus_message_iter_get_basic(val, &s);
        if (s)
            bt_strlcpy(d->name, sizeof(d->name), s);
    } else if (!strcmp(key, "Alias") && t == DBUS_TYPE_STRING) {
        const char *s = NULL;
        dbus_message_iter_get_basic(val, &s);
        if (s)
            bt_strlcpy(d->alias, sizeof(d->alias), s);
    } else if (!strcmp(key, "Class") && t == DBUS_TYPE_UINT32) {
        dbus_uint32_t u = 0;
        dbus_message_iter_get_basic(val, &u);
        d->cod = u;
        d->has_cod = true;
    } else if (!strcmp(key, "Paired") && t == DBUS_TYPE_BOOLEAN) {
        dbus_bool_t b = 0;
        dbus_message_iter_get_basic(val, &b);
        d->paired = b;
    } else if (!strcmp(key, "Connected") && t == DBUS_TYPE_BOOLEAN) {
        dbus_bool_t b = 0;
        dbus_message_iter_get_basic(val, &b);
        d->connected = b;
    } else if (!strcmp(key, "Trusted") && t == DBUS_TYPE_BOOLEAN) {
        dbus_bool_t b = 0;
        dbus_message_iter_get_basic(val, &b);
        d->trusted = b;
    }
}

static void bt_adp_prop_cb(const char *key, DBusMessageIter *val, void *ctx)
{
    btadapt_t *a = ctx;
    int t = dbus_message_iter_get_arg_type(val);

    if (t != DBUS_TYPE_BOOLEAN)
        return;

    dbus_bool_t b = 0;
    dbus_message_iter_get_basic(val, &b);

    if (!strcmp(key, "Powered"))
        a->powered = b;
    else if (!strcmp(key, "Pairable"))
        a->pairable = b;
    else if (!strcmp(key, "Discoverable"))
        a->discoverable = b;
    else if (!strcmp(key, "Discovering"))
        a->discovering = b;
}

/* Recherche par path dans la table courante (appelant doit tenir g_bt_lock) */
static btdev_t *bt_find_dev(const char *path)
{
    for (int i = 0; i < g_bt_ndevices; i++)
        if (!strcmp(g_bt_devices[i].path, path))
            return &g_bt_devices[i];
    return NULL;
}

/* ----- ObjectManager.GetManagedObjects : etat complet ----- */

static int bt_collect(void)
{
    /* Sauvegarde des infos transitoires (fails, started_here) par path */
    btdev_t old_dev[BT_MAX_DEVICES];
    int nold_dev = g_bt_ndevices;
    btadapt_t old_adp[BT_MAX_ADAPTERS];
    int nold_adp = g_bt_nadapters;
    pthread_mutex_lock(&g_bt_lock);
    memcpy(old_dev, g_bt_devices, sizeof(old_dev));
    memcpy(old_adp, g_bt_adapters, sizeof(old_adp));
    pthread_mutex_unlock(&g_bt_lock);

    DBusMessage *msg = dbus_message_new_method_call(BT_BLUEZ, "/",
                                                    BT_OM_IFACE,
                                                    "GetManagedObjects");
    if (!msg)
        return -1;

    DBusMessage *reply = bt_call(msg, 15000, "GetManagedObjects", "/");
    if (!reply)
        return -1;

    pthread_mutex_lock(&g_bt_lock);
    g_bt_ndevices = 0;
    g_bt_nadapters = 0;

    DBusMessageIter root, objs;
    bool ok = dbus_message_iter_init(reply, &root)
              && dbus_message_iter_get_arg_type(&root) == DBUS_TYPE_ARRAY;
    if (ok) {
        dbus_message_iter_recurse(&root, &objs);
        while (dbus_message_iter_get_arg_type(&objs) == DBUS_TYPE_DICT_ENTRY) {
            DBusMessageIter entry, ifaces;
            const char *opath = NULL;
            dbus_message_iter_recurse(&objs, &entry);
            if (dbus_message_iter_get_arg_type(&entry) == DBUS_TYPE_OBJECT_PATH)
                dbus_message_iter_get_basic(&entry, &opath);
            dbus_message_iter_next(&entry); /* ARRAY a{sa{sv}} (variant aplatie) */

            if (opath && dbus_message_iter_get_arg_type(&entry) == DBUS_TYPE_ARRAY) {
                dbus_message_iter_recurse(&entry, &ifaces);
                while (dbus_message_iter_get_arg_type(&ifaces)
                       == DBUS_TYPE_DICT_ENTRY) {
                    DBusMessageIter ie;
                    const char *iface = NULL;
                    dbus_message_iter_recurse(&ifaces, &ie);
                    if (dbus_message_iter_get_arg_type(&ie) == DBUS_TYPE_STRING)
                        dbus_message_iter_get_basic(&ie, &iface);
                    dbus_message_iter_next(&ie); /* ARRAY a{sv} (variant aplatie) */

                    if (iface && !strcmp(iface, BT_DEV_IFACE)
                        && g_bt_ndevices < BT_MAX_DEVICES) {
                        btdev_t *d = &g_bt_devices[g_bt_ndevices];
                        memset(d, 0, sizeof(*d));
                        bt_strlcpy(d->path, sizeof(d->path), opath);
                        bt_iter_props(&ie, bt_dev_prop_cb, d);
                        /* Restaure les compteurs d'echec du meme path */
                        for (int j = 0; j < nold_dev; j++) {
                            if (!strcmp(old_dev[j].path, d->path)) {
                                d->fails = old_dev[j].fails;
                                d->last_try = old_dev[j].last_try;
                                break;
                            }
                        }
                        g_bt_ndevices++;
                    } else if (iface && !strcmp(iface, BT_ADP_IFACE)
                               && g_bt_nadapters < BT_MAX_ADAPTERS) {
                        btadapt_t *a = &g_bt_adapters[g_bt_nadapters];
                        memset(a, 0, sizeof(*a));
                        bt_strlcpy(a->path, sizeof(a->path), opath);
                        bt_iter_props(&ie, bt_adp_prop_cb, a);
                        /* Restaure started_here du meme path */
                        for (int j = 0; j < nold_adp; j++) {
                            if (!strcmp(old_adp[j].path, a->path)) {
                                a->started_here = old_adp[j].started_here;
                                break;
                            }
                        }
                        g_bt_nadapters++;
                    }
                    dbus_message_iter_next(&ifaces);
                }
            }
            dbus_message_iter_next(&objs);
        }
    }

    pthread_mutex_unlock(&g_bt_lock);
    dbus_message_unref(reply);
    return ok ? 0 : -1;
}

/* ----- Appairage automatique d'une manette decouverte ----- */

static void bt_try_autopair(const char *path)
{
    btdev_t snapshot;
    bool doit = false;

    pthread_mutex_lock(&g_bt_lock);
    btdev_t *d = bt_find_dev(path);
    if (d && !d->paired && bt_dev_is_controller_locked(d)
        && d->fails < BT_PAIR_MAX_FAILS
        && time(NULL) - d->last_try >= BT_PAIR_RETRY_S) {
        d->last_try = time(NULL);
        snapshot = *d;
        doit = true;
    }
    pthread_mutex_unlock(&g_bt_lock);

    if (!doit || !g_bt_agent_ready)
        return;

    const char *label = snapshot.alias[0] ? snapshot.alias : snapshot.name;
    fprintf(stderr, "[BT] Manette a appairer detectee: %s (%s)\n",
            label[0] ? label : "nom inconnu", path);

    /* Couper la decouverte pendant le pairing (plus fiable) */
    pthread_mutex_lock(&g_bt_lock);
    for (int i = 0; i < g_bt_nadapters; i++) {
        if (g_bt_adapters[i].discovering && g_bt_adapters[i].started_here) {
            char adp_path[64];
            bt_strlcpy(adp_path, sizeof(adp_path), g_bt_adapters[i].path);
            g_bt_adapters[i].started_here = false;
            pthread_mutex_unlock(&g_bt_lock);
            bt_call_void(adp_path, BT_ADP_IFACE, "StopDiscovery", 10000,
                         "StopDiscovery");
            pthread_mutex_lock(&g_bt_lock);
        }
    }
    pthread_mutex_unlock(&g_bt_lock);

    /* Pair : l'agent enregistre sur l'autre connexion repond aux requetes */
    if (!bt_call_void(path, BT_DEV_IFACE, "Pair", 30000, "Pair")) {
        pthread_mutex_lock(&g_bt_lock);
        d = bt_find_dev(path);
        if (d)
            d->fails++;
        pthread_mutex_unlock(&g_bt_lock);
        return;
    }

    fprintf(stderr, "[BT] Appairage reussi: %s\n", label);

    /* Trusted : evite les futurs dialogues d'autorisation HID */
    bt_props_set_bool(path, BT_DEV_IFACE, "Trusted", true, "Trusted");

    /* Connect (certaines manettes attendent que le PC initie) */
    bt_call_void(path, BT_DEV_IFACE, "Connect", 30000, "Connect");

    pthread_mutex_lock(&g_bt_lock);
    d = bt_find_dev(path);
    if (d)
        d->fails = 0;
    pthread_mutex_unlock(&g_bt_lock);

    g_bt_dirty = true;
}

/* ----- Recalcul et application de l'etat ----- */

static void bt_refresh_state(void)
{
    if (!g_bt_bluez_up)
        return;
    if (bt_collect() < 0)
        return;

    pthread_mutex_lock(&g_bt_lock);

    int n_ctrl = 0;
    for (int i = 0; i < g_bt_ndevices; i++) {
        if (g_bt_devices[i].connected
            && bt_dev_is_controller_locked(&g_bt_devices[i]))
            n_ctrl++;
    }

    bool want_pairing = (n_ctrl == 0);
    if (want_pairing && !g_bt_pairing_mode) {
        /* Nouveau cycle : les manettes en echec ont droit a une nouvelle
           chance au prochain passage en mode pairing */
        for (int i = 0; i < g_bt_ndevices; i++)
            g_bt_devices[i].fails = 0;
        fprintf(stderr, "[BT] Aucune manette connectee: mode pairing actif\n");
    } else if (!want_pairing && g_bt_pairing_mode) {
        fprintf(stderr, "[BT] %d manette(s) connectee(s): pairing coupe\n",
                n_ctrl);
    }
    g_bt_pairing_mode = want_pairing;

    /* Copie les decisions prises sous verrou pour agir hors verrou */
    struct {
        char path[64];
        bool stop_disc;
        bool disc_off;
        bool pairable_on;
        bool disc_on;
        bool start_disc;
    } todo[BT_MAX_ADAPTERS];
    int ntodo = 0;

    struct {
        char path[192];
        bool trust_only; /* paired sans trusted: juste Trusted=true */
    } pair_candidates[BT_MAX_DEVICES];
    int ncandidates = 0;

    for (int i = 0; i < g_bt_nadapters && ntodo < BT_MAX_ADAPTERS; i++) {
        btadapt_t *a = &g_bt_adapters[i];
        if (!a->powered)
            continue; /* BT eteint par l'utilisateur: respect */

        memset(&todo[ntodo], 0, sizeof(todo[ntodo]));
        bt_strlcpy(todo[ntodo].path, sizeof(todo[ntodo].path), a->path);

        if (!want_pairing) {
            if (a->discovering && a->started_here) {
                todo[ntodo].stop_disc = true;
                a->started_here = false;
            }
            if (a->discoverable)
                todo[ntodo].disc_off = true;
        } else {
            if (!a->pairable)
                todo[ntodo].pairable_on = true;
            if (!a->discoverable)
                todo[ntodo].disc_on = true;
            if (!a->discovering)
                todo[ntodo].start_disc = true;
        }
        ntodo++;
    }

    if (want_pairing) {
        for (int i = 0; i < g_bt_ndevices; i++) {
            btdev_t *d = &g_bt_devices[i];
            if (d->paired && !d->trusted
                && bt_dev_is_controller_locked(d)) {
                /* Manette deja appairee mais non trusted (pairing manuel
                   ancien): evite les futurs dialogues d'autorisation */
                bt_strlcpy(pair_candidates[ncandidates].path,
                           sizeof(pair_candidates[ncandidates].path),
                           d->path);
                pair_candidates[ncandidates].trust_only = true;
                ncandidates++;
            } else if (!d->paired && bt_dev_is_controller_locked(d)) {
                bt_strlcpy(pair_candidates[ncandidates].path,
                           sizeof(pair_candidates[ncandidates].path),
                           d->path);
                pair_candidates[ncandidates].trust_only = false;
                ncandidates++;
            }
        }
    }

    pthread_mutex_unlock(&g_bt_lock);

    for (int i = 0; i < ntodo; i++) {
        if (todo[i].stop_disc)
            bt_call_void(todo[i].path, BT_ADP_IFACE, "StopDiscovery", 10000,
                         "StopDiscovery");
        if (todo[i].disc_off)
            bt_props_set_bool(todo[i].path, BT_ADP_IFACE, "Discoverable",
                              false, "Discoverable off");
        if (todo[i].pairable_on)
            bt_props_set_bool(todo[i].path, BT_ADP_IFACE, "Pairable", true,
                              "Pairable on");
        if (todo[i].disc_on)
            bt_props_set_bool(todo[i].path, BT_ADP_IFACE, "Discoverable",
                              true, "Discoverable on");
        if (todo[i].start_disc) {
            if (bt_call_void(todo[i].path, BT_ADP_IFACE, "StartDiscovery",
                             10000, "StartDiscovery silencieux")) {
                pthread_mutex_lock(&g_bt_lock);
                for (int j = 0; j < g_bt_nadapters; j++)
                    if (!strcmp(g_bt_adapters[j].path, todo[i].path))
                        g_bt_adapters[j].started_here = true;
                pthread_mutex_unlock(&g_bt_lock);
            }
        }
    }

    for (int i = 0; i < ncandidates; i++) {
        if (pair_candidates[i].trust_only)
            bt_props_set_bool(pair_candidates[i].path, BT_DEV_IFACE,
                              "Trusted", true, "Trusted");
        else
            bt_try_autopair(pair_candidates[i].path);
    }
}

/* ----- Traitement des signaux bluez (thread etat) ----- */

static void bt_on_properties_changed(DBusMessage *m)
{
    DBusMessageIter it;
    const char *iface = NULL;

    if (!dbus_message_iter_init(m, &it)
        || dbus_message_iter_get_arg_type(&it) != DBUS_TYPE_STRING)
        return;
    dbus_message_iter_get_basic(&it, &iface);
    if (!iface)
        return;

    /* On ne reagit qu'aux proprietes utiles (les changements RSSI pendant
       la decouverte sont tres frequents et inutiles ici) */
    dbus_message_iter_next(&it); /* a{sv} changed */

    struct stmt {
        bool relevant;
    } st = { false };

    if (!strcmp(iface, BT_DEV_IFACE)) {
        DBusMessageIter arr, a;
        arr = it;
        if (dbus_message_iter_get_arg_type(&arr) == DBUS_TYPE_ARRAY) {
            dbus_message_iter_recurse(&arr, &a);
            while (!st.relevant
                   && dbus_message_iter_get_arg_type(&a) == DBUS_TYPE_DICT_ENTRY) {
                DBusMessageIter de;
                const char *key = NULL;
                dbus_message_iter_recurse(&a, &de);
                if (dbus_message_iter_get_arg_type(&de) == DBUS_TYPE_STRING)
                    dbus_message_iter_get_basic(&de, &key);
                if (key && (!strcmp(key, "Connected") || !strcmp(key, "Paired")
                            || !strcmp(key, "Name") || !strcmp(key, "Alias")
                            || !strcmp(key, "Class")))
                    st.relevant = true;
                dbus_message_iter_next(&a);
            }
        }
    } else if (!strcmp(iface, BT_ADP_IFACE)) {
        DBusMessageIter arr, a;
        arr = it;
        if (dbus_message_iter_get_arg_type(&arr) == DBUS_TYPE_ARRAY) {
            dbus_message_iter_recurse(&arr, &a);
            while (!st.relevant
                   && dbus_message_iter_get_arg_type(&a) == DBUS_TYPE_DICT_ENTRY) {
                DBusMessageIter de;
                const char *key = NULL;
                dbus_message_iter_recurse(&a, &de);
                if (dbus_message_iter_get_arg_type(&de) == DBUS_TYPE_STRING)
                    dbus_message_iter_get_basic(&de, &key);
                if (key && (!strcmp(key, "Powered")
                            || !strcmp(key, "Discovering")))
                    st.relevant = true;
                dbus_message_iter_next(&a);
            }
        }
    }

    if (st.relevant)
        g_bt_dirty = true;
}

static DBusHandlerResult bt_state_filter(DBusConnection *conn, DBusMessage *m,
                                         void *data)
{
    (void)conn;
    (void)data;

    if (dbus_message_get_type(m) != DBUS_MESSAGE_TYPE_SIGNAL)
        return DBUS_HANDLER_RESULT_NOT_YET_HANDLED;

    const char *iface = dbus_message_get_interface(m);
    const char *member = dbus_message_get_member(m);
    if (!iface || !member)
        return DBUS_HANDLER_RESULT_NOT_YET_HANDLED;

    if (!strcmp(iface, BT_OM_IFACE)
        && (!strcmp(member, "InterfacesAdded")
            || !strcmp(member, "InterfacesRemoved"))) {
        g_bt_dirty = true;
    } else if (!strcmp(iface, BT_PROPS_IFACE)
               && !strcmp(member, "PropertiesChanged")) {
        bt_on_properties_changed(m);
    }

    return DBUS_HANDLER_RESULT_NOT_YET_HANDLED;
}

/* ----- Agent de pairing (seconde connexion, thread dedie) -----
   Connexion separee obligatoire : les appels bloquants (Pair) du thread
   etat ne doivent pas bloquer les reponses aux requetes de l'agent. */

static DBusConnection *g_bt_agent_conn;

/* Verifie (table courante) si un path correspond a une manette */
static bool bt_agent_path_is_controller(const char *path)
{
    bool ok = false;
    pthread_mutex_lock(&g_bt_lock);
    btdev_t *d = bt_find_dev(path);
    if (d && bt_dev_is_controller_locked(d))
        ok = true;
    pthread_mutex_unlock(&g_bt_lock);
    return ok;
}

static DBusHandlerResult bt_agent_filter(DBusConnection *conn, DBusMessage *m,
                                         void *data)
{
    (void)data;

    if (dbus_message_get_type(m) != DBUS_MESSAGE_TYPE_METHOD_CALL)
        return DBUS_HANDLER_RESULT_NOT_YET_HANDLED;

    const char *path = dbus_message_get_path(m);
    const char *iface = dbus_message_get_interface(m);
    const char *member = dbus_message_get_member(m);

    if (!path || strcmp(path, BT_AGENT_PATH) || !iface
        || strcmp(iface, BT_AGENT_IFACE) || !member)
        return DBUS_HANDLER_RESULT_NOT_YET_HANDLED;

    DBusMessage *reply = NULL;

    if (!strcmp(member, "Release") || !strcmp(member, "Cancel")
        || !strcmp(member, "DisplayPinCode")
        || !strcmp(member, "DisplayPasskey")) {
        reply = dbus_message_new_method_return(m);
    } else if (!strcmp(member, "RequestPinCode")
               || !strcmp(member, "RequestPasskey")) {
        /* PIN legacy jamais utilise par les manettes modernes */
        reply = dbus_message_new_error(m, "org.bluez.Error.Rejected",
                                       "PIN non supporte");
    } else if (!strcmp(member, "RequestConfirmation")
               || !strcmp(member, "RequestAuthorization")
               || !strcmp(member, "AuthorizeService")) {
        /* 1er argument = object path du peripherique */
        const char *devpath = NULL;
        DBusMessageIter it;
        if (dbus_message_iter_init(m, &it)
            && dbus_message_iter_get_arg_type(&it) == DBUS_TYPE_OBJECT_PATH)
            dbus_message_iter_get_basic(&it, &devpath);

        if (devpath && bt_agent_path_is_controller(devpath)) {
            reply = dbus_message_new_method_return(m);
            fprintf(stderr, "[BT] Pairing accepte: %s\n", devpath);
        } else {
            reply = dbus_message_new_error(m, "org.bluez.Error.Rejected",
                                           "pas une manette");
            fprintf(stderr, "[BT] Pairing refuse (pas une manette): %s\n",
                    devpath ? devpath : "?");
        }
    }

    if (reply) {
        dbus_connection_send(conn, reply, NULL);
        dbus_connection_flush(conn);
        dbus_message_unref(reply);
        return DBUS_HANDLER_RESULT_HANDLED;
    }

    return DBUS_HANDLER_RESULT_NOT_YET_HANDLED;
}

static void bt_agent_register(void)
{
    DBusMessage *msg = dbus_message_new_method_call(BT_BLUEZ, "/org/bluez",
                                                    BT_AGENT_MGR,
                                                    "RegisterAgent");
    if (!msg)
        return;

    const char *apath = BT_AGENT_PATH;
    const char *cap = "NoInputNoOutput";
    dbus_message_append_args(msg,
        DBUS_TYPE_OBJECT_PATH, &apath,
        DBUS_TYPE_STRING, &cap,
        DBUS_TYPE_INVALID);

    DBusError err;
    dbus_error_init(&err);
    DBusMessage *reply = dbus_connection_send_with_reply_and_block(
        g_bt_agent_conn, msg, 10000, &err);
    dbus_message_unref(msg);
    if (dbus_error_is_set(&err)) {
        fprintf(stderr, "[BT] RegisterAgent: %s\n", err.message);
        dbus_error_free(&err);
        if (reply)
            dbus_message_unref(reply);
        return;
    }
    if (reply)
        dbus_message_unref(reply);

    /* PAS de RequestDefaultAgent volontairement : bluedevil (KDE) garde la
       main pour les pairings manuels avec confirmation utilisateur. Notre
       agent n'est utilise que pour les pairings que nous initions. */
    g_bt_agent_ready = true;
    fprintf(stderr, "[BT] Agent de pairing enregistre\n");
}

static void *bt_agent_thread(void *arg)
{
    (void)arg;

    DBusError err;
    dbus_error_init(&err);
    g_bt_agent_conn = dbus_bus_get_private(DBUS_BUS_SYSTEM, &err);
    if (!g_bt_agent_conn) {
        fprintf(stderr, "[BT] Bus systeme (agent) indisponible: %s\n",
                err.message);
        dbus_error_free(&err);
        return NULL;
    }
    dbus_connection_set_exit_on_disconnect(g_bt_agent_conn, FALSE);

    DBusObjectPathVTable vtable = {
        .unregister_function = NULL,
        .message_function = bt_agent_filter,
    };
    if (!dbus_connection_register_object_path(g_bt_agent_conn, BT_AGENT_PATH,
                                              &vtable, NULL)) {
        fprintf(stderr, "[BT] Enregistrement du path agent impossible\n");
        dbus_connection_close(g_bt_agent_conn);
        dbus_connection_unref(g_bt_agent_conn);
        g_bt_agent_conn = NULL;
        return NULL;
    }

    bool registered = false;

    while (*g_bt_run) {
        if (!g_bt_bluez_up) {
            if (registered) {
                registered = false;
                g_bt_agent_ready = false;
            }
            usleep(500000);
            continue;
        }
        if (!registered) {
            bt_agent_register();
            registered = g_bt_agent_ready;
        }

        dbus_connection_read_write(g_bt_agent_conn, 500);
        while (dbus_connection_get_dispatch_status(g_bt_agent_conn)
               == DBUS_DISPATCH_DATA_REMAINS)
            dbus_connection_dispatch(g_bt_agent_conn);
    }

    g_bt_agent_ready = false;
    dbus_connection_close(g_bt_agent_conn);
    dbus_connection_unref(g_bt_agent_conn);
    g_bt_agent_conn = NULL;
    return NULL;
}

/* ----- Verrou inter-sessions : une seule instance pilote le BT ----- */

static bool bt_lock_try_acquire(void)
{
    if (g_bt_lock_fd >= 0)
        return true;

    int fd = open(BT_LOCK_PATH, O_RDWR | O_CREAT | O_CLOEXEC, 0666);
    if (fd < 0)
        return false;
    fchmod(fd, 0666); /* un autre utilisateur doit pouvoir prendre le relais */

    struct flock fl = {
        .l_type = F_WRLCK,
        .l_whence = SEEK_SET,
        .l_start = 0,
        .l_len = 0,
    };
    if (fcntl(fd, F_SETLK, &fl) < 0) {
        close(fd);
        return false;
    }

    g_bt_lock_fd = fd;
    fprintf(stderr, "[BT] Verrou acquis: cette instance pilote le Bluetooth\n");
    return true;
}

/* ----- Thread etat : surveillance + application ----- */

static void *bt_state_thread(void *arg)
{
    (void)arg;

    DBusError err;
    dbus_error_init(&err);
    g_bt_conn = dbus_bus_get_private(DBUS_BUS_SYSTEM, &err);
    if (!g_bt_conn) {
        fprintf(stderr, "[BT] Bus systeme indisponible: %s\n", err.message);
        dbus_error_free(&err);
        return NULL;
    }
    dbus_connection_set_exit_on_disconnect(g_bt_conn, FALSE);

    dbus_connection_add_filter(g_bt_conn, bt_state_filter, NULL, NULL);
    dbus_bus_add_match(g_bt_conn,
        "type='signal',sender='org.bluez',"
        "interface='org.freedesktop.DBus.ObjectManager'", &err);
    dbus_error_free(&err);
    dbus_bus_add_match(g_bt_conn,
        "type='signal',sender='org.bluez',"
        "interface='org.freedesktop.DBus.Properties'", &err);
    dbus_error_free(&err);

    time_t last_refresh = 0;

    while (*g_bt_run) {
        /* Attente de bluetoothd */
        dbus_error_init(&err);
        bool up = dbus_bus_name_has_owner(g_bt_conn, BT_BLUEZ, &err);
        if (dbus_error_is_set(&err)) {
            dbus_error_free(&err);
            up = false;
        }
        if (!up) {
            if (g_bt_bluez_up)
                fprintf(stderr, "[BT] bluetoothd introuvable, pause\n");
            g_bt_bluez_up = false;
            sleep(2);
            continue;
        }
        if (!g_bt_bluez_up) {
            g_bt_bluez_up = true;
            g_bt_dirty = true; /* recalcul complet au retour de bluez */
        }

        dbus_connection_read_write(g_bt_conn, 500);
        while (dbus_connection_get_dispatch_status(g_bt_conn)
               == DBUS_DISPATCH_DATA_REMAINS)
            dbus_connection_dispatch(g_bt_conn);

        time_t now = time(NULL);
        if (g_bt_dirty || now - last_refresh >= BT_WATCHDOG_S) {
            g_bt_dirty = false;
            last_refresh = now;
            bt_refresh_state();
        }
    }

    /* Etat sur en sortant : coupe ce qu'on a active */
    if (g_bt_bluez_up && bt_collect() == 0) {
        for (int i = 0; i < g_bt_nadapters; i++) {
            if (g_bt_adapters[i].discovering && g_bt_adapters[i].started_here)
                bt_call_void(g_bt_adapters[i].path, BT_ADP_IFACE,
                             "StopDiscovery", 10000, "StopDiscovery sortie");
            if (g_bt_adapters[i].discoverable)
                bt_props_set_bool(g_bt_adapters[i].path, BT_ADP_IFACE,
                                  "Discoverable", false,
                                  "Discoverable off sortie");
        }
    }
    g_bt_bluez_up = false;

    dbus_connection_close(g_bt_conn);
    dbus_connection_unref(g_bt_conn);
    g_bt_conn = NULL;
    fprintf(stderr, "[BT] Autopair arrete, mode pairing desactive\n");
    return NULL;
}

/* ----- Thread manager : verrou puis lancement des 2 threads BT ----- */

static void *bt_manager(void *arg)
{
    (void)arg;

    while (*g_bt_run && !bt_lock_try_acquire())
        sleep(3);
    if (!*g_bt_run)
        return NULL;

    pthread_t t_state, t_agent;
    bool has_state = pthread_create(&t_state, NULL, bt_state_thread, NULL) == 0;
    bool has_agent = pthread_create(&t_agent, NULL, bt_agent_thread, NULL) == 0;

    if (has_state)
        pthread_join(t_state, NULL);
    if (has_agent)
        pthread_join(t_agent, NULL);

    if (g_bt_lock_fd >= 0) {
        close(g_bt_lock_fd); /* libere le flock */
        g_bt_lock_fd = -1;
    }
    return NULL;
}

static void bt_autopair_start(volatile bool *running_flag)
{
    g_bt_run = running_flag;
    *g_bt_run = true;

    /* Obligatoire avant TOUT usage multi-thread de libdbus
       (thread BT + thread principal avec le bus session) */
    dbus_threads_init_default();

    if (pthread_create(&g_bt_manager_thread, NULL, bt_manager, NULL) != 0)
        fprintf(stderr, "[WARN] Thread BT autopair non demarre\n");
}

static void bt_autopair_stop(void)
{
    pthread_join(g_bt_manager_thread, NULL);
}

/* =========================================================================
 * PROGRAMME PRINCIPAL
 * ========================================================================= */

int main(void)
{
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

    bt_autopair_start(&running);

    setup_vt_tracking();

    int gamepad_fd = find_gamepad();
    if (gamepad_fd >= 0) {
        if (vt_active)
            inhibit_screensaver();
    } else {
        fprintf(stderr, "[INFO] Aucune manette detectee au demarrage.\n");
    }

    struct pollfd pfds[2];
    struct input_event ev;

    while (running) {
        /* --- Reconnexion manette si perdue --- */
        if (gamepad_fd < 0) {
            sleep(1);
            gamepad_fd = find_gamepad();
            if (gamepad_fd >= 0) {
                if (vt_active)
                    inhibit_screensaver();
                reset_button_states();
            }
            continue;
        }

        /* --- Construction du set poll: gamepad + inotify VT --- */
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

        int ret = poll(pfds, nfds, 500);
        if (ret < 0) {
            if (errno == EINTR)
                continue;
            fprintf(stderr, "[WARN] Erreur poll, deconnexion manette.\n");
            uninhibit_screensaver();
            close(gamepad_fd);
            reset_button_states();
            gamepad_fd = find_gamepad();
            if (gamepad_fd >= 0 && vt_active)
                inhibit_screensaver();
            continue;
        }

        /* --- Evenement inotify: switch VT --- */
        if (inotify_vt_fd >= 0 && nfds >= 2
            && (pfds[1].revents & POLLIN)) {
            char ino_buf[4096];
            while (read(inotify_vt_fd, ino_buf, sizeof(ino_buf)) > 0) {}
            check_vt_activity();
        }

        /* --- Deconnexion manette --- */
        if (pfds[0].revents & (POLLERR | POLLHUP | POLLNVAL)) {
            fprintf(stderr, "[WARN] Manette deconnectee.\n");
            uninhibit_screensaver();
            close(gamepad_fd);
            reset_button_states();
            gamepad_fd = find_gamepad();
            if (gamepad_fd >= 0 && vt_active)
                inhibit_screensaver();
            continue;
        }

        /* --- Lecture evenements manette --- */
        if (ret > 0 && (pfds[0].revents & POLLIN)) {
            while (true) {
                ssize_t bytes = read(gamepad_fd, &ev, sizeof(ev));
                if (bytes < 0) {
                    if (errno == EAGAIN || errno == EWOULDBLOCK)
                        break;
                    fprintf(stderr, "[WARN] Erreur lecture manette, reconnexion...\n");
                    uninhibit_screensaver();
                    close(gamepad_fd);
                    reset_button_states();
                    gamepad_fd = find_gamepad();
                    if (gamepad_fd >= 0 && vt_active)
                        inhibit_screensaver();
                    break;
                }
                if (bytes == sizeof(ev)) {
                    if (vt_active)
                        process_event(&ev);
                }
            }
        }

        /* Volume : uniquement quand le VT est actif */
        if (vt_active)
            handle_volume();

        /* Surveillance des processus enfants (toujours actif pour
           nettoyer les pid) */
        check_child(&mouse_pid, &mouse_running, "mouse.py");
        check_child(&menuvsr_pid, &menuvsr_running, "menuvsr.py");
    }

    fprintf(stderr, "[INFO] Arret du script.\n");
    uninhibit_screensaver();

    if (mouse_pid > 0)
        kill(mouse_pid, SIGTERM);
    if (menuvsr_pid > 0)
        kill(menuvsr_pid, SIGTERM);

    if (gamepad_fd >= 0)
        close(gamepad_fd);

    cleanup_vt_tracking();

    if (dbus_conn) {
        dbus_connection_unref(dbus_conn);
        dbus_conn = NULL;
    }

    bt_autopair_stop();

    return 0;
}
