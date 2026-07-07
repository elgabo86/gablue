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

static int find_gamepad(void)
{
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

        int fd = open(path, O_RDWR | O_NONBLOCK);
        if (fd < 0)
            continue;

        unsigned long key_bits[MAX_KEY_BITS / (8 * sizeof(unsigned long))] = {0};
        if (ioctl(fd, EVIOCGBIT(EV_KEY, sizeof(key_bits)), key_bits) < 0) {
            close(fd);
            continue;
        }

        bool has_btn_a = key_bits[BTN_A / (8 * sizeof(unsigned long))]
            & (1UL << (BTN_A % (8 * sizeof(unsigned long))));
        bool has_btn_select = key_bits[BTN_SELECT / (8 * sizeof(unsigned long))]
            & (1UL << (BTN_SELECT % (8 * sizeof(unsigned long))));

        if (has_btn_a || has_btn_select) {
            char name[256] = {0};
            ioctl(fd, EVIOCGNAME(sizeof(name)), name);
            fprintf(stderr, "[INFO] Manette trouvee: %s (%s)\n", name, path);
            closedir(dir);
            return fd;
        }

        close(fd);
    }

    closedir(dir);
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
            axis_y = (ev->value - 128) / 127.0;
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
 * PROGRAMME PRINCIPAL
 * ========================================================================= */

int main(void)
{
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

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

    return 0;
}
