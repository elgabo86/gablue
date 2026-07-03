%post --erroronfail --log=/tmp/anaconda_custom_logs/disable-fedora-flatpak.log
# Désactiver le remote Flatpak Fedora (on utilise Flathub uniquement)
set -euo pipefail
if flatpak remotes --system | grep -q fedora; then
    flatpak remote-delete --system fedora || :
fi
%end
