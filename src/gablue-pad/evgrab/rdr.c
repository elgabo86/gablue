/* rdr - lecteur evdev court pour banc de test EVIOCGRAB.
 *
 *   rdr [-g] <node> <secondes> <label>
 *
 * -g : tente un grab exclusif (EVIOCGRAB) sur le node.
 * Affiche le nombre d'événements reçus et le résultat du grab.
 */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <errno.h>
#include <fcntl.h>
#include <linux/input.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    int grab = 0, i = 1;
    if (argc > 1 && strcmp(argv[1], "-g") == 0) {
        grab = 1;
        i++;
    }
    if (argc < i + 3) {
        fprintf(stderr, "usage: %s [-g] <node> <secondes> <label>\n", argv[0]);
        return 2;
    }
    const char *node = argv[i];
    double secs = atof(argv[i + 1]);
    const char *label = argv[i + 2];

    int fd = open(node, O_RDONLY | O_NONBLOCK);
    if (fd < 0) {
        printf("%-28s OPEN_FAIL %s\n", label, strerror(errno));
        return 1;
    }

    if (grab) {
        if (ioctl(fd, EVIOCGRAB, 1) < 0) {
            printf("%-28s GRAB_FAIL %s\n", label, strerror(errno));
            close(fd);
            return 1;
        }
        printf("%-28s GRAB_OK\n", label);
    } else {
        printf("%-28s (sans grab)\n", label);
    }
    fflush(stdout);

    /* Horloge CLOCK_MONOTONIC, pas de sleep d'attente active : poll(). */
    struct timespec t0, now;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    int n = 0, last_code = -1;
    for (;;) {
        struct pollfd p = {.fd = fd, .events = POLLIN};
        int r = poll(&p, 1, 100);
        clock_gettime(CLOCK_MONOTONIC, &now);
        double elapsed = (now.tv_sec - t0.tv_sec) + (now.tv_nsec - t0.tv_nsec) / 1e9;
        if (elapsed >= secs)
            break;
        if (r > 0 && (p.revents & POLLIN)) {
            struct input_event ev[32];
            ssize_t got = read(fd, ev, sizeof ev);
            if (got > 0) {
                int cnt = got / sizeof ev[0];
                for (int k = 0; k < cnt; k++)
                    if (ev[k].type == EV_KEY || ev[k].type == EV_ABS)
                        last_code = ev[k].code;
                n += cnt;
            }
        }
    }

    printf("%-28s evenements=%-4d dernier_code=%d\n", label, n, last_code);
    if (grab)
        ioctl(fd, EVIOCGRAB, 0);
    close(fd);
    return n > 0 ? 0 : 3; /* 3 = rien reçu */
}
