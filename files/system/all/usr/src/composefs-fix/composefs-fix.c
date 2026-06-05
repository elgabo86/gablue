#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

/* On evite sys/statfs.h pour contourner le conflit de type statfs64 */
struct statfs {
    long f_type;
    long f_bsize;
    unsigned long f_blocks;
    unsigned long f_bfree;
    unsigned long f_bavail;
    unsigned long f_files;
    unsigned long f_ffree;
    int f_fsid[2];
    long f_namelen;
    long f_frsize;
    long f_flags;
    long f_spare[4];
};

#define OVERLAYFS_SUPER_MAGIC 0x794C7630

static int (*real_statfs)(const char *, struct statfs *) = NULL;

__attribute__((constructor))
static void init(void)
{
    real_statfs = (int (*)(const char *, struct statfs *))dlsym(RTLD_NEXT, "statfs");
}

static int redirect(const char *path, struct statfs *buf, int ret)
{
    if (ret != 0 || !path || !buf)
        return ret;
    if (buf->f_type != OVERLAYFS_SUPER_MAGIC || buf->f_bavail > 0)
        return ret;

    const char *target = NULL;
    char alt[4096];

    if (strcmp(path, "/") == 0 || strcmp(path, "/home") == 0) {
        target = "/var/home";
    } else if (strncmp(path, "/home/", 6) == 0) {
        snprintf(alt, sizeof(alt), "/var/home/%s", path + 6);
        target = alt;
    }

    return target ? real_statfs(target, buf) : ret;
}

int statfs(const char *path, struct statfs *buf)
{
    if (!real_statfs)
        init();
    return redirect(path, buf, real_statfs(path, buf));
}

int statfs64(const char *path, struct statfs *buf)
{
    if (!real_statfs)
        init();
    return redirect(path, buf, real_statfs(path, buf));
}
