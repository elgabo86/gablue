# Enable tailscale + firewall rules & connect to private network
tailscale-up:
    sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent
    sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent
    sudo tailscale up --login-server https://headscale.gabserv.duckdns.org

# Install/Reinstall custom Archlinux
distrobox-archgab:
    podman kill --all
    distrobox-rm -f archgab
    distrobox-create --pull latest --name archgab --nvidia --image ghcr.io/elgabo86/archgab:latest

# Rebase Gablue to main non nvidia
gablue-rebase-main:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main:latest

# Rebase Gablue to nvidia variant
gablue-rebase-nvidia:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest

# Add custom flatpak rules for theme, vk capture, mangohud config and bottles permissions
flatpak-custom-overrides:
    flatpak override --user --reset
    flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
    flatpak override --user --filesystem=xdg-config/MangoHud:ro
    flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
    flatpak override --user --env=OBS_VKCAPTURE=1
    flatpak override --user --filesystem=/run/media
    flatpak override --user --filesystem=xdg-download
    flatpak override com.usebottles.bottles --user --filesystem=xdg-data/applications
    flatpak override com.usebottles.bottles --user --env=DXVK_ASYNC=1
    flatpak override com.usebottles.bottles --user --env=DXVK_GPLASYNCCACHE=1

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
    flatpak override --user --reset
    flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
    flatpak override --user --filesystem=xdg-config/MangoHud:ro
    flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
    flatpak override --user --env=OBS_VKCAPTURE=1
    flatpak override --user --filesystem=/run/media
    flatpak override --user --filesystem=xdg-download
    flatpak override com.usebottles.bottles --user --filesystem=xdg-data/applications
    flatpak override com.usebottles.bottles --user --env=DXVK_ASYNC=1
    flatpak override com.usebottles.bottles --user --env=DXVK_GPLASYNCCACHE=1
    rpm-ostree kargs --append-if-missing=mitigations=off

# Install ble and atuin, for history bash sync and more
atuin-setup:
    curl -L https://github.com/akinomyoga/ble.sh/releases/download/nightly/ble-nightly.tar.xz --output ble-nightly.tar.xz
    tar xJf ble-nightly.tar.xz
    bash ble-nightly/ble.sh --install ~/.local/share
    rm -rf ble-nightly
    rm -f ble-nightly.tar.xz
    echo 'source ~/.local/share/blesh/ble.sh' >> ~/.bashrc
    echo 'eval "$(atuin init bash)"' >> ~/.bashrc

# Enable ssh remote connection
ssh-on:
    sudo systemctl enable sshd.socket --now

# Disable ssh remote connection
ssh-off:
    sudo systemctl disable sshd.socket --now

# Enable update notification
show-update:
    sudo sed -i 's/dbus_notify = true/dbus_notify = false/g' /etc/ublue-update/ublue-update.toml

# Disable update notification
hide-update:
    sudo sed -i 's/dbus_notify = false/dbus_notify = true/g' /etc/ublue-update/ublue-update.toml

# Hide grub menu on boot
hide-grub:
    #!/usr/bin/bash
    sudo sed -i 's/GRUB_TIMEOUT=5/GRUB_TIMEOUT=0/g' /etc/default/grub
    echo 'GRUB_TIMEOUT_STYLE=hidden' | sudo tee -a /etc/default/grub 1>/dev/null
    echo 'GRUB_HIDDEN_TIMEOUT=1' | sudo tee -a /etc/default/grub 1>/dev/null
    if [ -d /sys/firmware/efi ]; then
        sudo grub2-mkconfig -o /etc/grub2-efi.cfg
    else
        sudo grub2-mkconfig -o /etc/grub2.cfg
    fi

# Show grub menu on boot
show-grub:
    #!/usr/bin/bash
    sudo sed -i '/GRUB_HIDDEN_TIMEOUT=1/d' /etc/default/grub
    sudo sed -i '/GRUB_TIMEOUT_STYLE=hidden/d' /etc/default/grub
    sudo sed -i 's/GRUB_TIMEOUT=0/GRUB_TIMEOUT=5/g' /etc/default/grub
    if [ -d /sys/firmware/efi ]; then
        sudo grub2-mkconfig -o /etc/grub2-efi.cfg
    else
        sudo grub2-mkconfig -o /etc/grub2.cfg
    fi

# Set sound in 432hz
sound-432:
    cp -rf /usr/share/pipewire ~/.config
    sed -i 's/#default.clock.rate          = 48000/default.clock.rate          = 43200/g' ~/.config/pipewire/pipewire.conf
    sed -i 's/#default.clock.allowed-rates = \[ 48000 \]/default.clock.allowed-rates = [ 43200 ]/g' ~/.config/pipewire/pipewire.conf
    sed -i 's\#node.latency       = 1024/48000\node.latency       = 2048/48000\g' ~/.config/pipewire/jack.conf
    systemctl --user restart pipewire

# Reset sound config
sound-reset:
    rm -rf ~/.config/pipewire
    systemctl --user restart pipewire

# Create gablue install iso
create-iso-main:
    sudo podman run --rm --rmi --privileged --volume $(xdg-user-dir DOWNLOAD):/isogenerator/output -e VERSION=40 -e IMAGE_VARIANT=Kinoite -e IMAGE_REPO=ghcr.io/elgabo86 -e IMAGE_NAME=gablue-main -e IMAGE_TAG=latest  ghcr.io/ublue-os/isogenerator:40

# Create gablue install iso (nvidia)
create-iso-nvidia:
    sudo podman run --rm --rmi --privileged --volume $(xdg-user-dir DOWNLOAD):/isogenerator/output -e VERSION=40 -e IMAGE_VARIANT=Kinoite -e IMAGE_REPO=ghcr.io/elgabo86 -e IMAGE_NAME=gablue-nvidia -e IMAGE_TAG=latest  ghcr.io/ublue-os/isogenerator:40

# Install/Update Ollama+openwebui (cpu only)
ollama-cpu:
    podman kill --all
    podman create --pull always --replace --name ollama -p 11434:11434 -v ollama:/root/.ollama docker.io/ollama/ollama:latest
    podman create -p 127.0.0.1:3000:8080 --network=pasta:-T,11434 --add-host=ollama.local:127.0.0.1 --env 'OLLAMA_BASE_URL=http://ollama.local:11434' --env 'ANONYMIZED_TELEMETRY=False' -v open-webui:/app/backend/data --label io.containers.autoupdate=registry --replace --name open-webui --pull always ghcr.io/open-webui/open-webui:main
    podman start ollama open-webui

# Install/Update Ollama+openwebui (nvidia)
ollama-nvidia:
    podman kill --all
    podman create --pull always --replace --name ollama -p 11434:11434 -v ollama:/root/.ollama --security-opt label=disable --device nvidia.com/gpu=all docker.io/ollama/ollama:latest
    podman create -p 127.0.0.1:3000:8080 --network=pasta:-T,11434 --add-host=ollama.local:127.0.0.1 --env 'OLLAMA_BASE_URL=http://ollama.local:11434' --env 'ANONYMIZED_TELEMETRY=False' -v open-webui:/app/backend/data --label io.containers.autoupdate=registry --replace --name open-webui --pull always ghcr.io/open-webui/open-webui:main
    podman start ollama open-webui

# Install/Update Ollama+openwebui (nvidia)
ollama-amd:
    podman kill --all
    podman create --pull always --name ollama -p 11434:11434 -v ollama:/root/.ollama --security-opt label=disable  --device /dev/dri --device /dev/kfd  --replace --env HSA_OVERRIDE_GFX_VERSION=10.3.0 docker.io/ollama/ollama:rocm
    podman create -p 127.0.0.1:3000:8080 --network=pasta:-T,11434 --add-host=ollama.local:127.0.0.1 --env 'OLLAMA_BASE_URL=http://ollama.local:11434' --env 'ANONYMIZED_TELEMETRY=False' -v open-webui:/app/backend/data --label io.containers.autoupdate=registry --replace --name open-webui --pull always ghcr.io/open-webui/open-webui:main
    podman start ollama open-webui

# Remove Ollama+openwebui and models data
ollama-remove:
    podman kill --all
    podman rm ollama open-webui
    podman volume rm ollama open-webui
    podman rmi --all
