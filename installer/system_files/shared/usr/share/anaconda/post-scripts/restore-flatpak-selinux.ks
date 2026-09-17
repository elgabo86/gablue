%post --erroronfail --log=/tmp/anaconda_custom_logs/restore-flatpak-selinux.log
# Rétablit les contextes SELinux de /var/lib/flatpak dans le système installé.
#
# Les flatpaks sont copiés du live vers le déploiement ostree SANS les xattrs
# SELinux (rsync --filter="-x security.selinux", cf. install-flatpaks.ks) : le
# kernel leur assigne donc un contexte à la création, à partir du contexte du
# répertoire parent. Ce restorecon explicite garantit des labels conformes à la
# policy même si ce labeling implicite n'a pas joué (aligné Bazzite
# restore-selinux-labels.ks, commit f0bafa6).
#
# %post chrooté (pas de --nochroot) : s'exécute dans le déploiement ostree,
# restorecon cible donc bien /var/lib/flatpak du système installé et non celui,
# en lecture seule, du live.
#
# setenforce 0 : évite tout refus relabelto (le live tourne déjà en enforcing=0
# via la cmdline GRUB ; dans le chroot, selinuxfs n'est pas monté, l'échec de
# setenforce est donc normal et ignoré).
set -euo pipefail

setenforce 0 2>/dev/null || true

# Correctif non critique (le labeling implicite couvre le cas nominal) :
# un échec ne doit pas faire échouer l'installation.
restorecon -R /var/lib/flatpak 2>/dev/null || :
%end
