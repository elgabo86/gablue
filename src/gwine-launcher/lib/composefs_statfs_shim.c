#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <sys/vfs.h>
#include <sys/statvfs.h>

#define OVERLAYFS_MAGIC 0x794c7630

static void patch_statfs_if_overlay(struct statfs *buf) {
    if (buf->f_type == OVERLAYFS_MAGIC && buf->f_bavail == 0) {
        buf->f_blocks = 100000000;
        buf->f_bfree = 50000000;
        buf->f_bavail = 50000000;
    }
}

static void patch_statfs64_if_overlay(struct statfs64 *buf) {
    if (buf->f_type == OVERLAYFS_MAGIC && buf->f_bavail == 0) {
        buf->f_blocks = 100000000;
        buf->f_bfree = 50000000;
        buf->f_bavail = 50000000;
    }
}

static void patch_statvfs_if_zero(struct statvfs *buf) {
    if (buf->f_bavail == 0) {
        buf->f_blocks = 100000000;
        buf->f_bfree = 50000000;
        buf->f_bavail = 50000000;
    }
}

static void patch_statvfs64_if_zero(struct statvfs64 *buf) {
    if (buf->f_bavail == 0) {
        buf->f_blocks = 100000000;
        buf->f_bfree = 50000000;
        buf->f_bavail = 50000000;
    }
}

int statfs(const char *path, struct statfs *buf) {
    typedef int (*fn_t)(const char *, struct statfs *);
    fn_t real = (fn_t)dlsym(RTLD_NEXT, "statfs");
    if (!real) { errno = ENOSYS; return -1; }
    int ret = real(path, buf);
    if (ret == 0) patch_statfs_if_overlay(buf);
    return ret;
}

int fstatfs(int fd, struct statfs *buf) {
    typedef int (*fn_t)(int, struct statfs *);
    fn_t real = (fn_t)dlsym(RTLD_NEXT, "fstatfs");
    if (!real) { errno = ENOSYS; return -1; }
    int ret = real(fd, buf);
    if (ret == 0) patch_statfs_if_overlay(buf);
    return ret;
}

int fstatfs64(int fd, struct statfs64 *buf) {
    typedef int (*fn_t)(int, struct statfs64 *);
    fn_t real = (fn_t)dlsym(RTLD_NEXT, "fstatfs64");
    if (!real) { errno = ENOSYS; return -1; }
    int ret = real(fd, buf);
    if (ret == 0) patch_statfs64_if_overlay(buf);
    return ret;
}

int statvfs(const char *path, struct statvfs *buf) {
    typedef int (*fn_t)(const char *, struct statvfs *);
    fn_t real = (fn_t)dlsym(RTLD_NEXT, "statvfs");
    if (!real) { errno = ENOSYS; return -1; }
    int ret = real(path, buf);
    if (ret == 0) patch_statvfs_if_zero(buf);
    return ret;
}

int fstatvfs(int fd, struct statvfs *buf) {
    typedef int (*fn_t)(int, struct statvfs *);
    fn_t real = (fn_t)dlsym(RTLD_NEXT, "fstatvfs");
    if (!real) { errno = ENOSYS; return -1; }
    int ret = real(fd, buf);
    if (ret == 0) patch_statvfs_if_zero(buf);
    return ret;
}

int statvfs64(const char *path, struct statvfs64 *buf) {
    typedef int (*fn_t)(const char *, struct statvfs64 *);
    fn_t real = (fn_t)dlsym(RTLD_NEXT, "statvfs64");
    if (!real) { errno = ENOSYS; return -1; }
    int ret = real(path, buf);
    if (ret == 0) patch_statvfs64_if_zero(buf);
    return ret;
}

int fstatvfs64(int fd, struct statvfs64 *buf) {
    typedef int (*fn_t)(int, struct statvfs64 *);
    fn_t real = (fn_t)dlsym(RTLD_NEXT, "fstatvfs64");
    if (!real) { errno = ENOSYS; return -1; }
    int ret = real(fd, buf);
    if (ret == 0) patch_statvfs64_if_zero(buf);
    return ret;
}
