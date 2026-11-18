/* hidrev - test de sémantique HIDIOCREVOKE sur /dev/hidrawN.
 *
 *   hidrev open   <node> <secondes>   : ouvre, attend, tente de lire
 *   hidrev revoke <node>              : ouvre et appelle HIDIOCREVOKE
 *
 * But : quand un client appelle HIDIOCREVOKE, les AUTRES clients déjà
 * ouverts sont-ils invalidés (règle globale) ou seulement l'appelant ?
 * C'est ce qui décide si un overlay peut couper les lecteurs hidraw
 * (SDL hidapi, Steam) comme EVIOCGRAB coupe les lecteurs evdev.
 */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <errno.h>
#include <fcntl.h>
#include <linux/hidraw.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <unistd.h>

int main(int argc, char **argv)
{
    if (argc < 3) {
        fprintf(stderr, "usage: %s open|revoke <node> [secondes]\n", argv[0]);
        return 2;
    }
    const char *mode = argv[1];
    const char *node = argv[2];

    int fd = open(node, O_RDONLY);
    if (fd < 0) {
        printf("[%d] OPEN_FAIL %s: %s\n", getpid(), node, strerror(errno));
        return 1;
    }
    printf("[%d] OPEN_OK %s\n", getpid(), node);
    fflush(stdout);

    if (strcmp(mode, "revoke") == 0) {
        int rc = ioctl(fd, HIDIOCREVOKE, 0);
        printf("[%d] HIDIOCREVOKE rc=%d %s\n", getpid(), rc, rc < 0 ? strerror(errno) : "OK");
        fflush(stdout);
        return rc < 0 ? 1 : 0;
    }

    double secs = argc > 3 ? atof(argv[3]) : 3.0;
    sleep((unsigned)secs);

    /* La révocation doit se manifester ici, sur un fd ouvert AVANT. */
    unsigned char buf[64];
    ssize_t n = read(fd, buf, sizeof buf);
    if (n < 0)
        printf("[%d] LECTURE_ECHEC errno=%d (%s)  <- fd invalidé\n", getpid(), errno,
               strerror(errno));
    else
        printf("[%d] LECTURE_OK %zd octets  <- toujours valide\n", getpid(), n);

    /* Un nouvel open est-il encore permis après révocation ? */
    int fd2 = open(node, O_RDONLY);
    printf("[%d] REOPEN %s\n", getpid(),
           fd2 < 0 ? strerror(errno) : "OK");
    if (fd2 >= 0)
        close(fd2);
    close(fd);
    return 0;
}
