// gpad-gate.bpf.c - coupe les lectures des nodes d'une manette pendant le menu.
//
// Hook LSM `file_permission` : appelé à CHAQUE read(), donc il coupe aussi un
// processus qui avait DÉJÀ ouvert le node (ce que EVIOCGRAB, chmod et
// HIDIOCREVOKE ne savent pas faire). Aucun relais de données, aucun rebind :
// en jeu, les applications lisent la manette physique directement.
//
// Coût hors menu : NUL, littéralement. Le programme n'est pas attaché tant que
// le menu est fermé (le démon fait attach/detach) : le hook n'existe pas dans
// le noyau, il n'y a donc rien à payer par read().
//
// Encodage CRITIQUE : inode->i_rdev est un dev_t noyau = (major << 20) | minor,
// alors que st_rdev côté user-space vaut (minor | major << 8 | ...). Le loader
// fait la conversion ; sans elle la comparaison ne matche jamais (mesuré).
#include "vmlinux.h"

#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>

#define MAY_READ 4
#define FILE_OFF_INODE 32 // BTF: file.f_inode @ bits_offset=256
#define INODE_OFF_RDEV 76 // BTF: inode.i_rdev @ bits_offset=608

#define MAX_TARGETS 8
#define MAX_EXEMPT 4

#define CFG_NTARGET 0
#define CFG_EXEMPT 1 // pid exemptés : CFG_EXEMPT .. CFG_EXEMPT+MAX_EXEMPT-1
#define CFG_TARGETS 8 // dev_t noyau : CFG_TARGETS .. CFG_TARGETS+MAX_TARGETS-1

struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, CFG_TARGETS + MAX_TARGETS);
    __type(key, __u32);
    __type(value, __u64);
} cfg SEC(".maps");

struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 2);
    __type(key, __u32);
    __type(value, __u64);
} stats SEC(".maps");

SEC("lsm/file_permission")
int BPF_PROG(hid_gate, struct file *file, int mask)
{
    if (!(mask & MAY_READ))
        return 0;

    /* exemption : le pid du menu (il doit continuer à lire la manette) */
    __u64 pid_tgid = bpf_get_current_pid_tgid();
    __u32 pid = pid_tgid >> 32;
    for (__u32 i = 0; i < MAX_EXEMPT; i++) {
        __u32 k = CFG_EXEMPT + i;
        __u64 *allowed = bpf_map_lookup_elem(&cfg, &k);
        if (allowed && *allowed == (__u64)pid)
            return 0;
    }
    /* root garde l'accès : c'est le démon de contrôle */
    if ((__u32)bpf_get_current_uid_gid() == 0)
        return 0;

    struct inode *ino = NULL;
    if (bpf_probe_read_kernel(&ino, sizeof(ino), (void *)file + FILE_OFF_INODE))
        return 0;
    if (!ino)
        return 0;

    __u32 rdev = 0;
    if (bpf_probe_read_kernel(&rdev, sizeof(rdev), (void *)ino + INODE_OFF_RDEV))
        return 0;
    if (!rdev)
        return 0; // fichier ordinaire : hors sujet

    __u32 kn = CFG_NTARGET;
    __u64 *ntarget = bpf_map_lookup_elem(&cfg, &kn);
    if (!ntarget)
        return 0;
    __u32 n = (__u32)*ntarget;
    if (n > MAX_TARGETS)
        n = MAX_TARGETS;

    for (__u32 i = 0; i < MAX_TARGETS; i++) {
        if (i >= n)
            break;
        __u32 k = CFG_TARGETS + i;
        __u64 *want = bpf_map_lookup_elem(&cfg, &k);
        if (!want || (__u32)*want != rdev)
            continue;

        __u32 kd = 1;
        __u64 *deny = bpf_map_lookup_elem(&stats, &kd);
        if (deny)
            *deny += 1;
        return -11; // -EAGAIN : « pas de données », pas une erreur dure
    }

    return 0;
}

char LICENSE[] SEC("license") = "GPL";
