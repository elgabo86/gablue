import '100-bling.just'
# Include some of your custom scripts here!

# Enable tailscale + firewall rules & connect to private network
tailscale-up:
    sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent
    sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent
    sudo tailscale up --login-server https://headscale.gabserv.duckdns.org

# Install/Reinstall custom Archlinux
distrobox-archgab:
    podman kill --all
    distrobox-rm -f archgab
    distrobox-create --pull latest --name archgab --home ~/.archgab --nvidia --image ghcr.io/elgabo86/archgab:latest

# Rebase Gablue to main non nvidia
gablue-rebase-main:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main:latest

# Rebase Gablue to nvidia variant
gablue-rebase-nvidia:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest

# Add custom flatpak rules for theme, vk capture, mangohud config and bottles permissions
flatpak-custom-overrides:
    flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
    flatpak override --user --filesystem=xdg-config/MangoHud:ro
    flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
    flatpak override --user --env=OBS_VKCAPTURE=1
    flatpak override com.usebottles.bottles --user --filesystem=xdg-data/applications
    flatpak override com.usebottles.bottles --user --filesystem=/run/media
    flatpak override com.usebottles.bottles --user --filesystem=xdg-download

# Add amd kargs to have advanced settings in CoreCtrl
amd-corectrl-set-kargs:
    echo 'Setting needed kargs for CoreCtrl...'
    rpm-ostree kargs --append="amdgpu.ppfeaturemask=0xffffffff"

# Remove cpu security, boost performance
mitigations-off:
    rpm-ostree kargs --append-if-missing=mitigations=off

# Enable cpu security, more secure
mitigations-on:
    rpm-ostree kargs --delete-if-present=mitigations=off

# Add flatpak custom rules and remove cpu security
gablue-setup:
    flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
    flatpak override --user --filesystem=xdg-config/MangoHud:ro
    flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
    flatpak override --user --env=OBS_VKCAPTURE=1
    flatpak override com.usebottles.bottles --user --filesystem=xdg-data/applications
    flatpak override com.usebottles.bottles --user --filesystem=/run/media
    flatpak override com.usebottles.bottles --user --filesystem=xdg-download
    rpm-ostree kargs --append-if-missing=mitigations=off

# Install miniconda, useful to make build python environment
miniconda-install:
    mkdir -p ~/.miniconda
    wget https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-x86_64.sh -O ~/.miniconda/miniconda.sh
    bash ~/.miniconda/miniconda.sh -b -u -p ~/.miniconda
    rm -rf ~/.miniconda/miniconda.sh
    ~/.miniconda/bin/conda init bash
    ~/.miniconda/bin/conda config --set auto_activate_base false

# Install ble and atuin, for history bash sync and more
atuin-setup:
    wget -O - https://github.com/akinomyoga/ble.sh/releases/download/nightly/ble-nightly.tar.xz | tar xJf -
    mkdir -p ~/.local/share/blesh
    cp -Rf ble-nightly/* ~/.local/share/blesh/
    rm -rf ble-nightly
    echo 'source ~/.local/share/blesh/ble.sh' >> ~/.bashrc
    echo 'eval "$(atuin init bash --disable-up-arrow)"' >> ~/.bashrc
