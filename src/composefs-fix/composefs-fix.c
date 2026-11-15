#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>
#include <sys/statvfs.h>

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
static int (*real_statvfs)(const char *, struct statvfs *) = NULL;

__attribute__((constructor))
static void init(void)
{
    real_statfs = (int (*)(const char *, struct statfs *))dlsym(RTLD_NEXT, "statfs");
    real_statvfs = (int (*)(const char *, struct statvfs *))dlsym(RTLD_NEXT, "statvfs");
}

/* Chemin de substitution quand la requete porte sur l'overlay composefs de / :
   /, /home et /home/x sont rediriges vers le btrfs /var/home */
static const char *redirect_target(const char *path, char *alt, size_t alt_size)
{
    if (!path)
        return NULL;
    if (strcmp(path, "/") == 0 || strcmp(path, "/home") == 0)
        return "/var/home";
    if (strncmp(path, "/home/", 6) == 0) {
        snprintf(alt, alt_size, "/var/home/%s", path + 6);
        return alt;
    }
    return NULL;
}

/* Interception statfs/statfs64 : f_type dit directement si on est sur
   l'overlay composefs (f_bavail toujours a 0 dessus) */
static int redirect_statfs(const char *path, struct statfs *buf, int ret)
{
    if (ret != 0 || !path || !buf)
        return ret;
    if (buf->f_type != OVERLAYFS_SUPER_MAGIC || buf->f_bavail > 0)
        return ret;

    char alt[4096];
    const char *target = redirect_target(path, alt, sizeof(alt));
    return target ? real_statfs(target, buf) : ret;
}

/* Interception statvfs/statvfs64 : struct statvfs n'expose pas f_type, on
   interroge donc statfs sur le meme chemin pour ne rediriger que l'overlay
   composefs (et pas un btrfs reellement plein).
   Necessaire depuis Dolphin 26.08 : la barre d'espace libre du status bar
   passe par KIO::fileSystemFreeSpace -> FileProtocol::fileSystemFreeSpace
   (kio_file.so) -> statvfs, qui n'etait pas intercepte : l'overlay de /
   rapporte 0 bloc disponible et Dolphin affichait "0 o libre". */
static int redirect_statvfs(const char *path, struct statvfs *buf, int ret)
{
    if (ret != 0 || !path || !buf)
        return ret;

    char alt[4096];
    const char *target = redirect_target(path, alt, sizeof(alt));
    if (!target)
        return ret;

    struct statfs sfs;
    if (real_statfs(path, &sfs) != 0)
        return ret;
    if (sfs.f_type != OVERLAYFS_SUPER_MAGIC || sfs.f_bavail > 0)
        return ret;

    return real_statvfs(target, buf);
}

int statfs(const char *path, struct statfs *buf)
{
    if (!real_statfs)
        init();
    return redirect_statfs(path, buf, real_statfs(path, buf));
}

int statfs64(const char *path, struct statfs *buf)
{
    if (!real_statfs)
        init();
    return redirect_statfs(path, buf, real_statfs(path, buf));
}

int statvfs(const char *path, struct statvfs *buf)
{
    if (!real_statvfs)
        init();
    return redirect_statvfs(path, buf, real_statvfs(path, buf));
}

int statvfs64(const char *path, struct statvfs64 *buf)
{
    if (!real_statvfs)
        init();
    /* struct statvfs64 a le meme agencement que statvfs en 64 bits */
    return redirect_statvfs(path, (struct statvfs *)buf,
                            real_statvfs(path, (struct statvfs *)buf));
}
