%post --nochroot --erroronfail --log=/tmp/anaconda_custom_logs/install-flatpaks.log
# Installer les Flatpaks pré-cachés depuis l'ISO live
# --nochroot car on doit accéder au repo flatpak du live ISO (pas celui du sysroot vide)
set -euo pipefail

LIVE_FLATPAK="/var/lib/flatpak/repo"
SYSROOT_FLATPAK="/mnt/sysroot/var/lib/flatpak"

if [ -d "$LIVE_FLATPAK" ] && [ -d "$SYSROOT_FLATPAK" ]; then
    # Copier le dépôt Flatpak depuis le live vers le système installé
    rsync -av "$LIVE_FLATPAK/" "$SYSROOT_FLATPAK/repo/"
    # Installer les flatpaks depuis le dépôt local dans le sysroot
    flatpak install --system --no-pull --noninteractive \
        --ostree-dir="$SYSROOT_FLATPAK" \
        org.mozilla.firefox \
        org.videolan.VLC \
        org.atheme.audacious || :
fi
%end
