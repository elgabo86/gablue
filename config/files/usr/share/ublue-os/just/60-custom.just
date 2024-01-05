import '100-bling.just'
# Include some of your custom scripts here!

tailscale-up:
    sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent
    sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent
    sudo tailscale up --login-server https://headscale.gabserv.duckdns.org

distrobox-archgab:
    podman kill --all
    distrobox-rm -f archgab
    distrobox-create --pull latest --name archgab --home ~/.archgab --nvidia --image ghcr.io/elgabo86/archgab:latest

gablue-rebase-main:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main:latest

gablue-rebase-nvidia:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest

flatpak-custom-overrides:
    flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
    flatpak override --user --filesystem=xdg-config/MangoHud:ro
    flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
    flatpak override --user --env=OBS_VKCAPTURE=1
    flatpak override com.usebottles.bottles --user --filesystem=xdg-data/applications
    flatpak override com.usebottles.bottles --user --filesystem=/run/media
    flatpak override com.usebottles.bottles --user --filesystem=xdg-download

amd-corectrl-set-kargs:
    echo 'Setting needed kargs for CoreCtrl...'
    rpm-ostree kargs --append="amdgpu.ppfeaturemask=0xffffffff"

mitigations-off:
    rpm-ostree kargs --append-if-missing=mitigations=off

mitigations-on:
    rpm-ostree kargs --delete-if-present=mitigations=off

gablue-setup:
    flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
    flatpak override --user --filesystem=xdg-config/MangoHud:ro
    flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
    flatpak override --user --env=OBS_VKCAPTURE=1
    flatpak override com.usebottles.bottles --user --filesystem=xdg-data/applications
    flatpak override com.usebottles.bottles --user --filesystem=/run/media
    flatpak override com.usebottles.bottles --user --filesystem=xdg-download
    rpm-ostree kargs --append-if-missing=mitigations=off

miniconda-install:
    mkdir -p ~/.miniconda
    wget https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-x86_64.sh -O ~/.miniconda/miniconda.sh
    bash ~/.miniconda/miniconda.sh -b -u -p ~/.miniconda
    rm -rf ~/.miniconda/miniconda.sh
    ~/.miniconda/bin/conda init bash
    ~/.miniconda/bin/conda config --set auto_activate_base false
    ~/.miniconda/bin/conda config --add channels conda-forge
    ~/.miniconda/bin/conda config --set channel_priority strict

atuin-setup:
    wget -O - https://github.com/akinomyoga/ble.sh/releases/download/nightly/ble-nightly.tar.xz | tar xJf -
    mkdir -p ~/.local/share/blesh
    cp -Rf ble-nightly/* ~/.local/share/blesh/
    rm -rf ble-nightly
    echo 'source ~/.local/share/blesh/ble.sh' >> ~/.bashrc
    echo 'eval "$(atuin init bash --disable-up-arrow)"' >> ~/.bashrc
