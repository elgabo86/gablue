/* vpad - manette virtuelle uinput pour banc de test EVIOCGRAB.
 *
 * Crée un pad (BTN_MODE, BTN_SOUTH, ABS_HAT0X/Y, ABS_X/Y) nommé
 * "OpenCode Virtual Gamepad", imprime le chemin de son /dev/input/eventN puis
 * émet une rafale d'événements à chaque SIGUSR1. Sort sur SIGTERM.
 *
 * Aucune manette physique n'est nécessaire : les événements sont
 * déterministes, le test est rejouable.
 *
 * Les nodes sont résolus par sysfs (/sys/class/input/inputN/{name,eventM,jsM})
 * et non par ouverture de tous les /dev/input/event*, pour ne pas dépendre
 * des ACL uaccess au moment où le node vient d'être créé.
 */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <linux/uinput.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>

#define PAD_NAME "OpenCode Virtual Gamepad"
/* VPAD_NOM : nom du périphérique. Utile au banc quand un gpad-trigger de
 * l'utilisateur tourne : son scan filtre les noms contenant « gamepad »,
 * « controller »… Un nom neutre le rend aveugle à notre manette de test. */
static const char *g_nom_pad = PAD_NAME;

static int ufd = -1;
static volatile sig_atomic_t pending_burst;
static volatile sig_atomic_t stop;

static volatile sig_atomic_t pending_nav;
static volatile sig_atomic_t pending_accept;
static volatile sig_atomic_t pending_home;
static volatile sig_atomic_t pending_cancel;
static volatile sig_atomic_t pending_home_left;
static volatile sig_atomic_t pending_home_right;
static volatile sig_atomic_t pending_stick;   /* -1 gauche, 1 droite, -2 haut, 2 bas */
static volatile sig_atomic_t pending_home_up;
static volatile sig_atomic_t pending_home_down;
static volatile sig_atomic_t pending_hat;    /* -1 D-pad gauche, 1 droite */
static volatile sig_atomic_t pending_combo;  /* 1 SELECT, 2 START, 3 TRIANGLE */
static volatile sig_atomic_t pending_page;   /* 1 = L1, 2 = R1 */
static volatile sig_atomic_t pending_combo2; /* 1 = Home+R3, 2 = Home+carré */
static volatile sig_atomic_t pending_combo_l3; /* Home+L3 (BTN_THUMBL) */
static volatile sig_atomic_t pending_gachette; /* -1 = L2, 1 = R2 */
static volatile sig_atomic_t pending_home_state; /* 1 = appui, 2 = relâche */
static volatile sig_atomic_t pending_up_key;  /* BTN_NORTH : un pas vers le haut */
static volatile sig_atomic_t stream;   /* SIGQUIT : flux continu 20 Hz */
static void on_usr1(int s) { (void)s; pending_accept = 1; }
static void on_usr2(int s) { (void)s; pending_nav = 1; }
static void on_hup(int s) { (void)s; pending_home = 1; }
static void on_alrm(int s) { (void)s; pending_cancel = 1; }
static void on_rtmin1(int s) { (void)s; pending_home_left = 1; }
static void on_rtmin2(int s) { (void)s; pending_home_right = 1; }
static void on_rtmin3(int s) { (void)s; pending_stick = -1; }
static void on_rtmin4(int s) { (void)s; pending_stick = 1; }
static void on_rtmin5(int s) { (void)s; pending_stick = -2; }
static void on_rtmin6(int s) { (void)s; pending_stick = 2; }
static void on_rtmin7(int s) { (void)s; pending_home_up = 1; }
static void on_rtmin8(int s) { (void)s; pending_home_down = 1; }
static void on_rtmin9(int s) { (void)s; pending_hat = -1; }
static void on_rtmin10(int s) { (void)s; pending_hat = 1; }
static void on_rtmin11(int s) { (void)s; pending_combo = 1; }
static void on_rtmin12(int s) { (void)s; pending_combo = 2; }
static void on_rtmin13(int s) { (void)s; pending_combo = 3; }
static void on_rtmin14(int s) { (void)s; pending_page = 1; }
static void on_rtmin15(int s) { (void)s; pending_page = 2; }
static void on_rtmin16(int s) { (void)s; pending_combo2 = 1; }
static void on_rtmin17(int s) { (void)s; pending_combo2 = 2; }
static void on_rtmin18(int s) { (void)s; pending_gachette = -1; }
static void on_rtmin19(int s) { (void)s; pending_gachette = 1; }
static void on_rtmin20(int s) { (void)s; pending_home_state = 1; }
static void on_rtmin21(int s) { (void)s; pending_home_state = 2; }
/* un seul pas « haut » fiable : le stick (RTMIN+5) émet deux pas d'un coup,
 * ce qui interdit de se poser sur l'en-tête depuis le premier choix */
static void on_rtmin22(int s) { (void)s; pending_up_key = 1; }
/* Home+L3 : Home part au jeu, le nôtre ne doit rien ouvrir */
static void on_rtmin23(int s) { (void)s; pending_combo_l3 = 1; }
static void on_quit(int s) { (void)s; stream = !stream; }
static void on_term(int s) { (void)s; stop = 1; }

static void emit(int type, int code, int val)
{
    struct input_event ev = {0};
    ev.type = type;
    ev.code = code;
    ev.value = val;
    if (write(ufd, &ev, sizeof ev) != (ssize_t)sizeof ev)
        perror("write uinput");
}

static void burst_nav(void)
{
    /* D-pad bas : ce que fait un appui "descendre" dans le menu */
    emit(EV_ABS, ABS_HAT0Y, 1);
    emit(EV_SYN, SYN_REPORT, 0);
    emit(EV_ABS, ABS_HAT0Y, 0);
    emit(EV_SYN, SYN_REPORT, 0);
}

static void burst_accept(void)
{
    emit(EV_KEY, BTN_SOUTH, 1);
    emit(EV_SYN, SYN_REPORT, 0);
    emit(EV_KEY, BTN_SOUTH, 0);
    emit(EV_SYN, SYN_REPORT, 0);
}

/* combo tel qu'une vraie manette l'envoie : Home maintenu + D-pad */
static void burst_combo(int hat_x)
{
    emit(EV_KEY, BTN_MODE, 1);
    emit(EV_SYN, SYN_REPORT, 0);
    emit(EV_ABS, ABS_HAT0X, hat_x);
    emit(EV_SYN, SYN_REPORT, 0);
    emit(EV_ABS, ABS_HAT0X, 0);
    emit(EV_SYN, SYN_REPORT, 0);
    emit(EV_KEY, BTN_MODE, 0);
    emit(EV_SYN, SYN_REPORT, 0);
}

/* stick gauche : valeur pleine échelle, puis retour au centre */
static void burst_stick(int which)
{
    int code = (which == -1 || which == 1) ? ABS_X : ABS_Y;
    int val = which < 0 ? -30000 : 30000;
    emit(EV_ABS, code, val);
    emit(EV_SYN, SYN_REPORT, 0);
    emit(EV_ABS, code, 0);
    emit(EV_SYN, SYN_REPORT, 0);
}

static void burst_cancel(void)
{
    /* bouton B / Est : "fermer" dans l'overlay */
    emit(EV_KEY, BTN_EAST, 1);
    emit(EV_SYN, SYN_REPORT, 0);
    emit(EV_KEY, BTN_EAST, 0);
    emit(EV_SYN, SYN_REPORT, 0);
}
static void burst_home(void)
{
    /* Home = BTN_MODE, ce que gpad-trigger attend */
    emit(EV_KEY, BTN_MODE, 1);
    emit(EV_SYN, SYN_REPORT, 0);
    emit(EV_KEY, BTN_MODE, 0);
    emit(EV_SYN, SYN_REPORT, 0);
}

static void burst(void)
{
    emit(EV_KEY, BTN_MODE, 1);
    emit(EV_KEY, BTN_SOUTH, 1);
    emit(EV_ABS, ABS_HAT0X, 1);
    emit(EV_SYN, SYN_REPORT, 0);

    emit(EV_KEY, BTN_MODE, 0);
    emit(EV_KEY, BTN_SOUTH, 0);
    emit(EV_ABS, ABS_HAT0X, 0);
    emit(EV_SYN, SYN_REPORT, 0);
}

/* Résout /dev/input/eventN et /dev/input/jsN via sysfs, par nom. */
static int resolve_nodes(char *evout, size_t evlen, char *jsout, size_t jslen)
{
    DIR *d = opendir("/sys/class/input");
    if (!d)
        return -1;
    int found = -1;
    struct dirent *de;
    while ((de = readdir(d))) {
        if (strncmp(de->d_name, "input", 5) != 0)
            continue;
        char path[300], name[128] = {0};
        snprintf(path, sizeof path, "/sys/class/input/%s/name", de->d_name);
        FILE *f = fopen(path, "r");
        if (!f)
            continue;
        if (!fgets(name, sizeof name, f)) {
            fclose(f);
            continue;
        }
        fclose(f);
        name[strcspn(name, "\n")] = '\0';
        if (strcmp(name, g_nom_pad) != 0)
            continue;

        char dirpath[300];
        snprintf(dirpath, sizeof dirpath, "/sys/class/input/%s", de->d_name);
        DIR *d2 = opendir(dirpath);
        if (!d2)
            continue;
        struct dirent *e2;
        while ((e2 = readdir(d2))) {
            if (strncmp(e2->d_name, "event", 5) == 0) {
                snprintf(evout, evlen, "/dev/input/%s", e2->d_name);
                found = 0;
            } else if (strncmp(e2->d_name, "js", 2) == 0) {
                snprintf(jsout, jslen, "/dev/input/%s", e2->d_name);
            }
        }
        closedir(d2);
        if (found == 0)
            break;
    }
    closedir(d);
    return found;
}

int main(void)
{
    const char *nom = getenv("VPAD_NOM");
    if (nom && *nom)
        g_nom_pad = nom;

    ufd = open("/dev/uinput", O_WRONLY | O_NONBLOCK);
    if (ufd < 0) {
        perror("open /dev/uinput");
        return 1;
    }

    ioctl(ufd, UI_SET_EVBIT, EV_KEY);
    ioctl(ufd, UI_SET_EVBIT, EV_ABS);
    ioctl(ufd, UI_SET_EVBIT, EV_SYN);
    ioctl(ufd, UI_SET_KEYBIT, BTN_MODE);
    ioctl(ufd, UI_SET_KEYBIT, BTN_SOUTH);
    ioctl(ufd, UI_SET_KEYBIT, BTN_EAST);
    ioctl(ufd, UI_SET_KEYBIT, BTN_START);
    ioctl(ufd, UI_SET_KEYBIT, BTN_SELECT);
    ioctl(ufd, UI_SET_KEYBIT, BTN_NORTH);
    ioctl(ufd, UI_SET_KEYBIT, BTN_WEST);
    ioctl(ufd, UI_SET_KEYBIT, BTN_TL);
    ioctl(ufd, UI_SET_KEYBIT, BTN_THUMBR);
    ioctl(ufd, UI_SET_KEYBIT, BTN_THUMBL);
    ioctl(ufd, UI_SET_KEYBIT, BTN_TR);
    ioctl(ufd, UI_SET_KEYBIT, BTN_TL2);
    ioctl(ufd, UI_SET_KEYBIT, BTN_TR2);

    struct uinput_abs_setup absx = {.code = ABS_X,
                                    .absinfo = {.minimum = -32768,
                                                .maximum = 32767,
                                                .flat = 128,
                                                .fuzz = 16}};
    ioctl(ufd, UI_ABS_SETUP, &absx);
    /* ABS_Y était absent : le noyau jetait silencieusement chaque émission,
     * d'où un stick gauche/droit qui marchait et un haut/bas muet au banc. */
    struct uinput_abs_setup absy = {.code = ABS_Y,
                                    .absinfo = {.minimum = -32768,
                                                .maximum = 32767,
                                                .flat = 128,
                                                .fuzz = 16}};
    ioctl(ufd, UI_ABS_SETUP, &absy);
    struct uinput_abs_setup habsx = {.code = ABS_HAT0X,
                                     .absinfo = {.minimum = -1, .maximum = 1}};
    ioctl(ufd, UI_ABS_SETUP, &habsx);
    struct uinput_abs_setup habsy = {.code = ABS_HAT0Y,
                                     .absinfo = {.minimum = -1, .maximum = 1}};
    ioctl(ufd, UI_ABS_SETUP, &habsy);

    struct uinput_setup usetup = {0};
    snprintf(usetup.name, UINPUT_MAX_NAME_SIZE, "%s", g_nom_pad);
    usetup.id.bustype = BUS_VIRTUAL;
    usetup.id.vendor = 0x0f0f;
    usetup.id.product = 0x0f0f;
    if (ioctl(ufd, UI_DEV_SETUP, &usetup) < 0) {
        perror("UI_DEV_SETUP");
        return 1;
    }
    if (ioctl(ufd, UI_DEV_CREATE) < 0) {
        perror("UI_DEV_CREATE");
        return 1;
    }

    struct sigaction sa = {0};
    sa.sa_handler = on_usr1;
    sigaction(SIGUSR1, &sa, NULL);
    sa.sa_handler = on_usr2;
    sigaction(SIGUSR2, &sa, NULL);
    sa.sa_handler = on_hup;
    sigaction(SIGHUP, &sa, NULL);
    sa.sa_handler = on_quit;
    sigaction(SIGQUIT, &sa, NULL);
    sa.sa_handler = on_alrm;
    sigaction(SIGALRM, &sa, NULL);
    sa.sa_handler = on_rtmin1;
    sigaction(SIGRTMIN + 1, &sa, NULL);
    sa.sa_handler = on_rtmin2;
    sigaction(SIGRTMIN + 2, &sa, NULL);
    sa.sa_handler = on_rtmin3;
    sigaction(SIGRTMIN + 3, &sa, NULL);
    sa.sa_handler = on_rtmin4;
    sigaction(SIGRTMIN + 4, &sa, NULL);
    sa.sa_handler = on_rtmin5;
    sigaction(SIGRTMIN + 5, &sa, NULL);
    sa.sa_handler = on_rtmin6;
    sigaction(SIGRTMIN + 6, &sa, NULL);
    sa.sa_handler = on_rtmin7;
    sigaction(SIGRTMIN + 7, &sa, NULL);
    sa.sa_handler = on_rtmin8;
    sigaction(SIGRTMIN + 8, &sa, NULL);
    sa.sa_handler = on_rtmin9;
    sigaction(SIGRTMIN + 9, &sa, NULL);
    sa.sa_handler = on_rtmin10;
    sigaction(SIGRTMIN + 10, &sa, NULL);
    sa.sa_handler = on_rtmin11;
    sigaction(SIGRTMIN + 11, &sa, NULL);
    sa.sa_handler = on_rtmin12;
    sigaction(SIGRTMIN + 12, &sa, NULL);
    sa.sa_handler = on_rtmin13;
    sigaction(SIGRTMIN + 13, &sa, NULL);
    sa.sa_handler = on_rtmin14;
    sigaction(SIGRTMIN + 14, &sa, NULL);
    sa.sa_handler = on_rtmin15;
    sigaction(SIGRTMIN + 15, &sa, NULL);
    sa.sa_handler = on_rtmin16;
    sigaction(SIGRTMIN + 16, &sa, NULL);
    sa.sa_handler = on_rtmin17;
    sigaction(SIGRTMIN + 17, &sa, NULL);
    sa.sa_handler = on_rtmin18;
    sigaction(SIGRTMIN + 18, &sa, NULL);
    sa.sa_handler = on_rtmin19;
    sigaction(SIGRTMIN + 19, &sa, NULL);
    sa.sa_handler = on_rtmin20;
    sigaction(SIGRTMIN + 20, &sa, NULL);
    sa.sa_handler = on_rtmin21;
    sigaction(SIGRTMIN + 21, &sa, NULL);
    sa.sa_handler = on_rtmin22;
    sigaction(SIGRTMIN + 22, &sa, NULL);
    sa.sa_handler = on_rtmin23;
    sigaction(SIGRTMIN + 23, &sa, NULL);
    sa.sa_handler = on_term;
    sigaction(SIGTERM, &sa, NULL);
    sigaction(SIGINT, &sa, NULL);

    char evnode[256] = {0}, jsnode[256] = {0};
    for (int i = 0; i < 50; i++) {
        if (resolve_nodes(evnode, sizeof evnode, jsnode, sizeof jsnode) == 0)
            break;
        usleep(100000);
    }
    if (evnode[0] == '\0') {
        fprintf(stderr, "vpad: node event introuvable\n");
        goto cleanup;
    }
    printf("PAD_EVENT=%s\nPAD_JS=%s\n", evnode, jsnode[0] ? jsnode : "(aucun)");
    fflush(stdout);

    while (!stop) {
        struct timespec ts = {0, 50 * 1000 * 1000};
        nanosleep(&ts, NULL);
        if (stream) {
            static int flip;
            flip = !flip;
            emit(EV_ABS, ABS_X, flip ? 100 : -100);
            emit(EV_SYN, SYN_REPORT, 0);
        }
        if (pending_accept) {
            pending_accept = 0;
            burst_accept();
            printf("ACCEPT_EMIS\n");
            fflush(stdout);
        }
        if (pending_home_state) {
            int v = pending_home_state == 1 ? 1 : 0;
            pending_home_state = 0;
            emit(EV_KEY, BTN_MODE, v);
            emit(EV_SYN, SYN_REPORT, 0);
            printf("HOME_STATE %d\n", v);
            fflush(stdout);
        }
        if (pending_home) {
            pending_home = 0;
            burst_home();
            printf("HOME_EMIS\n");
            fflush(stdout);
        }
        if (pending_nav) {
            pending_nav = 0;
            burst_nav();
            printf("NAV_EMIS\n");
            fflush(stdout);
        }
        if (pending_home_left) {
            pending_home_left = 0;
            burst_combo(-1);
            printf("HOME_LEFT_EMIS\n");
            fflush(stdout);
        }
        if (pending_home_up) {
            pending_home_up = 0;
            emit(EV_KEY, BTN_MODE, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_ABS, ABS_HAT0Y, -1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_ABS, ABS_HAT0Y, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, BTN_MODE, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            printf("HOME_UP_EMIS\n");
            fflush(stdout);
        }
        if (pending_home_down) {
            pending_home_down = 0;
            emit(EV_KEY, BTN_MODE, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_ABS, ABS_HAT0Y, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_ABS, ABS_HAT0Y, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, BTN_MODE, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            printf("HOME_DOWN_EMIS\n");
            fflush(stdout);
        }
        if (pending_combo_l3) {
            /* Home+L3 tel que la manette l'envoie : Home tenu, L3 pressé */
            pending_combo_l3 = 0;
            emit(EV_KEY, BTN_MODE, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, BTN_THUMBL, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, BTN_THUMBL, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, BTN_MODE, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            printf("COMBO_L3_EMIS\n");
            fflush(stdout);
        }
        if (pending_combo2) {
            int code = pending_combo2 == 1 ? BTN_THUMBR : BTN_WEST;
            pending_combo2 = 0;
            emit(EV_KEY, BTN_MODE, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, code, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, code, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, BTN_MODE, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            printf("COMBO2_EMIS %d\n", code);
            fflush(stdout);
        }
        if (pending_page) {
            int code = pending_page == 1 ? BTN_TL : BTN_TR;
            pending_page = 0;
            emit(EV_KEY, code, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, code, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            printf("PAGE_EMIS %d\n", code);
            fflush(stdout);
        }
        if (pending_gachette) {
            int code = pending_gachette < 0 ? BTN_TL2 : BTN_TR2;
            pending_gachette = 0;
            emit(EV_KEY, code, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, code, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            printf("GACHETTE_EMIS %d\n", code);
            fflush(stdout);
        }
        if (pending_hat) {
            int v = pending_hat;
            pending_hat = 0;
            emit(EV_ABS, ABS_HAT0X, v);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_ABS, ABS_HAT0X, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            printf("HAT_EMIS %d\n", v);
            fflush(stdout);
        }
        if (pending_combo) {
            int code = pending_combo == 1 ? BTN_SELECT
                       : pending_combo == 2 ? BTN_START : BTN_NORTH;
            pending_combo = 0;
            emit(EV_KEY, BTN_MODE, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, code, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, code, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, BTN_MODE, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            printf("COMBO_EMIS %d\n", code);
            fflush(stdout);
        }
        if (pending_up_key) {
            pending_up_key = 0;
            emit(EV_KEY, BTN_NORTH, 1);
            emit(EV_SYN, SYN_REPORT, 0);
            emit(EV_KEY, BTN_NORTH, 0);
            emit(EV_SYN, SYN_REPORT, 0);
            printf("UP_EMIS\n");
            fflush(stdout);
        }
        if (pending_stick) {
            burst_stick(pending_stick);
            printf("STICK_EMIS %d\n", pending_stick);
            pending_stick = 0;
            fflush(stdout);
        }
        if (pending_home_right) {
            pending_home_right = 0;
            burst_combo(1);
            printf("HOME_RIGHT_EMIS\n");
            fflush(stdout);
        }
        if (pending_cancel) {
            pending_cancel = 0;
            burst_cancel();
            printf("CANCEL_EMIS\n");
            fflush(stdout);
        }
    }

cleanup:
    ioctl(ufd, UI_DEV_DESTROY);
    close(ufd);
    return 0;
}
