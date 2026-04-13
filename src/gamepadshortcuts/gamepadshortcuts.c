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
#include <sys/poll.h>
#include <sys/wait.h>
#include <sys/types.h>
#include <linux/input.h>

#define SCRIPTS_DIR "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts"
#define VOLUME_COOLDOWN_NS 200000000L
#define MAX_KEY_BITS (KEY_MAX + 1)

static volatile bool running = true;
static pid_t inhibit_pid = -1;

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

static bool mouse_running = false;
static pid_t mouse_pid = -1;
static bool menuvsr_running = false;
static pid_t menuvsr_pid = -1;
static struct timespec last_volume_time = {0, 0};

static void signal_handler(int sig)
{
    (void)sig;
    running = false;
}

static void inhibit_screensaver(void)
{
    if (inhibit_pid > 0)
        return;

    pid_t pid = fork();
    if (pid < 0) {
        perror("fork inhibit");
        return;
    }
    if (pid == 0) {
        execlp("systemd-inhibit", "systemd-inhibit",
               "--what=idle",
               "--who=gablue-gamepadshortcuts",
               "--why=Manette connectee",
               "sleep", "infinity", (char *)NULL);
        _exit(1);
    }
    inhibit_pid = pid;
    fprintf(stderr, "[INFO] Inhibition ecran activee (pid: %d)\n", inhibit_pid);
}

static void uninhibit_screensaver(void)
{
    if (inhibit_pid > 0) {
        kill(inhibit_pid, SIGTERM);
        waitpid(inhibit_pid, NULL, 0);
        fprintf(stderr, "[INFO] Inhibition ecran desactivee\n");
        inhibit_pid = -1;
    }
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

    if (hat_x != last_hat_x || hat_y != last_hat_y) {
        if (hat_x == -1 && hat_y == 0) {
            fprintf(stderr, "[ACTION] SCREEN\n");
            launch_script(SCRIPTS_DIR "/takescreenshot", true);
            usleep(100000);
        } else if (hat_x == 1 && hat_y == 0) {
            fprintf(stderr, "[ACTION] RECORD\n");
            launch_script(SCRIPTS_DIR "/startstoprecord", true);
            usleep(2000000);
        } else if (hat_x == 0 && hat_y == 1) {
            fprintf(stderr, "[ACTION] FPS\n");
            launch_script(SCRIPTS_DIR "/changefps", false);
            usleep(100000);
        } else if (hat_x == 0 && hat_y == -1) {
            fprintf(stderr, "[ACTION] MANGO\n");
            launch_script(SCRIPTS_DIR "/showhidemango", false);
            usleep(100000);
        }
        last_hat_x = hat_x;
        last_hat_y = hat_y;
    }
}

static void handle_volume(void)
{
    if (!home_pressed)
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
    } else if (axis_y > 0.5) {
        fprintf(stderr, "[ACTION] VOLUME DOWN\n");
        launch_shell_cmd("pactl set-sink-volume @DEFAULT_SINK@ -10%");
        last_volume_time = now;
    }
}

static void process_event(struct input_event *ev)
{
    if (ev->type == EV_KEY) {
        switch (ev->code) {
        case BTN_MODE:   home_pressed = ev->value == 1; break;
        case BTN_SELECT: select_pressed = ev->value == 1; break;
        case BTN_START:  start_pressed = ev->value == 1; break;
        case BTN_C:      triangle_pressed = ev->value == 1; break;
        case BTN_X:      square_pressed = ev->value == 1; break;
        case BTN_B:      circle_pressed = ev->value == 1; break;
        case BTN_A:      break;
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

int main(void)
{
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

    int gamepad_fd = find_gamepad();
    if (gamepad_fd >= 0) {
        inhibit_screensaver();
    } else {
        fprintf(stderr, "[INFO] Aucune manette detectee au demarrage.\n");
    }

    struct pollfd pfd;
    struct input_event ev;

    while (running) {
        if (gamepad_fd < 0) {
            sleep(1);
            gamepad_fd = find_gamepad();
            if (gamepad_fd >= 0) {
                inhibit_screensaver();
                home_pressed = false;
                select_pressed = false;
                start_pressed = false;
                triangle_pressed = false;
                square_pressed = false;
                circle_pressed = false;
                l3_pressed = false;
                r3_pressed = false;
                hat_x = 0; hat_y = 0;
                last_hat_x = 0; last_hat_y = 0;
                axis_y = 0.0;
            }
            continue;
        }

        pfd.fd = gamepad_fd;
        pfd.events = POLLIN;
        pfd.revents = 0;

        int ret = poll(&pfd, 1, 100);
        if (ret < 0) {
            if (errno == EINTR)
                continue;
            fprintf(stderr, "[WARN] Erreur poll, deconnexion manette.\n");
            uninhibit_screensaver();
            close(gamepad_fd);
            gamepad_fd = find_gamepad();
            if (gamepad_fd >= 0)
                inhibit_screensaver();
            continue;
        }

        if (ret > 0 && (pfd.revents & POLLIN)) {
            while (true) {
                ssize_t bytes = read(gamepad_fd, &ev, sizeof(ev));
                if (bytes < 0) {
                    if (errno == EAGAIN || errno == EWOULDBLOCK)
                        break;
                    fprintf(stderr, "[WARN] Erreur lecture manette, reconnexion...\n");
                    uninhibit_screensaver();
                    close(gamepad_fd);
                    gamepad_fd = find_gamepad();
                    if (gamepad_fd >= 0)
                        inhibit_screensaver();
                    break;
                }
                if (bytes == sizeof(ev)) {
                    process_event(&ev);
                }
            }
        }

        if (pfd.revents & (POLLERR | POLLHUP | POLLNVAL)) {
            fprintf(stderr, "[WARN] Manette deconnectee.\n");
            uninhibit_screensaver();
            close(gamepad_fd);
            gamepad_fd = find_gamepad();
            if (gamepad_fd >= 0)
                inhibit_screensaver();
            continue;
        }

        handle_volume();

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

    return 0;
}
