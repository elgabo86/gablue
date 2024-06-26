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
    curl -L -o ble-nightly.tar.xz https://github.com/akinomyoga/ble.sh/releases/download/nightly/ble-nightly.tar.xz
    tar xJf ble-nightly.tar.xz
    bash ble-nightly/ble.sh --install ~/.local/share
    rm -rf ble-nightly
    rm -f ble-nightly.tar.xz
    echo 'source ~/.local/share/blesh/ble.sh' >> ~/.bashrc
    echo 'eval "$(atuin init bash)"' >> ~/.bashrc

# Enable ssh remote connection
ssh-on:
    sudo systemctl enable sshd.service --now

# Disable ssh remote connection
ssh-off:
    sudo systemctl disable sshd.service --now

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
    sudo podman run --rm --rmi --privileged --volume $(xdg-user-dir DOWNLOAD):/build-container-installer/build --security-opt label=disable ghcr.io/jasonn3/build-container-installer:latest VERSION=40 IMAGE_REPO=ghcr.io/elgabo86 IMAGE_NAME=gablue-main IMAGE_TAG=latest VARIANT=Kinoite FLATPAK_REMOTE_NAME=flathub ISO_NAME=build/gablue-main.iso FLATPAK_REMOTE_REFS="app/com.usebottles.bottles/x86_64/stable app/io.freetubeapp.FreeTube/x86_64/stable app/org.mozilla.firefox/x86_64/stable app/org.atheme.audacious/x86_64/stable app/org.libreoffice.LibreOffice/x86_64/stable app/org.kde.haruna/x86_64/stable app/org.kde.haruna/x86_64/stable app/io.github.flattool.Warehouse/x86_64/stable app/io.missioncenter.MissionCenter/x86_64/stable app/io.github.dvlv.boxbuddyrs/x86_64/stable runtime/org.gnome.Platform/x86_64/45 runtime/org.freedesktop.Platform/x86_64/23.08 runtime/org.kde.Platform/x86_64/6.6 runtime/org.kde.Platform/x86_64/5.15-23.08 runtime/org.gnome.Platform/x86_64/46"

# Create gablue install iso (nvidia)
create-iso-nvidia:
    sudo podman run --rm --rmi --privileged --volume $(xdg-user-dir DOWNLOAD):/build-container-installer/build --security-opt label=disable ghcr.io/jasonn3/build-container-installer:latest VERSION=40 IMAGE_REPO=ghcr.io/elgabo86 IMAGE_NAME=gablue-nvidia IMAGE_TAG=latest VARIANT=Kinoite FLATPAK_REMOTE_NAME=flathub ISO_NAME=build/gablue-nvidia.iso FLATPAK_REMOTE_REFS="app/com.usebottles.bottles/x86_64/stable app/io.freetubeapp.FreeTube/x86_64/stable app/org.mozilla.firefox/x86_64/stable app/org.atheme.audacious/x86_64/stable app/org.libreoffice.LibreOffice/x86_64/stable app/org.kde.haruna/x86_64/stable app/org.kde.haruna/x86_64/stable app/io.github.flattool.Warehouse/x86_64/stable app/io.missioncenter.MissionCenter/x86_64/stable app/io.github.dvlv.boxbuddyrs/x86_64/stable runtime/org.gnome.Platform/x86_64/45 runtime/org.freedesktop.Platform/x86_64/23.08 runtime/org.kde.Platform/x86_64/6.6 runtime/org.kde.Platform/x86_64/5.15-23.08 runtime/org.gnome.Platform/x86_64/46"

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

# Install pyenv
pyenv-setup:
    git clone https://github.com/pyenv/pyenv.git ~/.pyenv
    cd ~/.pyenv && src/configure && make -C src

# Update pyenv
pyenv-update:
    git -C ~/.pyenv pull

# Remove pyenv
pyenv-remove:
    rm -rf ~/.pyenv

# Install waydroid
waydroid-install:
    sudo systemctl start waydroid-container
    sudo waydroid init -c 'https://ota.waydro.id/system' -v 'https://ota.waydro.id/vendor'
    mkdir -p ~/.local/share/applications/
    cp /usr/share/ublue-os/gablue/desktops/WaydroidKiller.desktop ~/.local/share/applications/
    update-desktop-database ~/.local/share/applications
    sudo systemctl stop waydroid-container

# Configure waydroid for intel cpu
waydroid-configure-intel:
    sudo systemctl stop waydroid-container
    waydroid session start &
    sleep 20
    sudo mkdir -p /var/lib/waydroid/overlay/system/etc/init
    sudo cp /usr/share/ublue-os/gablue/waydroid/audio.rc /var/lib/waydroid/overlay/system/etc/init/
    sudo mkdir -p /var/lib/waydroid/overlay/system/etc
    sudo cp /usr/share/ublue-os/gablue/waydroid/hosts /var/lib/waydroid/overlay/system/etc
    waydroid prop set persist.waydroid.udev true
    waydroid prop set persist.waydroid.uevent true
    sudo rm -rf /tmp/waydroid_script
    git clone https://github.com/ublue-os/waydroid_script.git --depth 1 /tmp/waydroid_script
    python -m venv /tmp/waydroid_script/venv
    source /tmp/waydroid_script/venv/bin/activate
    /tmp/waydroid_script/venv/bin/pip install -r /tmp/waydroid_script/requirements.txt
    sudo /tmp/waydroid_script/venv/bin/python /tmp/waydroid_script/main.py remove libndk
    sudo /tmp/waydroid_script/venv/bin/python /tmp/waydroid_script/main.py install {libhoudini,widevine}
    sudo rm -rf /tmp/waydroid_script
    sudo sed -i "s/ro.hardware.gralloc=.*/ro.hardware.gralloc=${WAYDROID_GRALLOC:-minigbm_gbm_mesa}/g" /var/lib/waydroid/waydroid_base.prop
    waydroid session stop

# Configure waydroid for amd cpu
waydroid-configure-amd:
    sudo systemctl stop waydroid-container
    waydroid session start &
    sleep 20
    sudo mkdir -p /var/lib/waydroid/overlay/system/etc/init
    sudo cp /usr/share/ublue-os/gablue/waydroid/audio.rc /var/lib/waydroid/overlay/system/etc/init/
    sudo mkdir -p /var/lib/waydroid/overlay/system/etc
    sudo cp /usr/share/ublue-os/gablue/waydroid/hosts /var/lib/waydroid/overlay/system/etc
    waydroid prop set persist.waydroid.udev true
    waydroid prop set persist.waydroid.uevent true
    sudo rm -rf /tmp/waydroid_script
    git clone https://github.com/ublue-os/waydroid_script.git --depth 1 /tmp/waydroid_script
    python -m venv /tmp/waydroid_script/venv
    source /tmp/waydroid_script/venv/bin/activate
    /tmp/waydroid_script/venv/bin/pip install -r /tmp/waydroid_script/requirements.txt
    sudo /tmp/waydroid_script/venv/bin/python /tmp/waydroid_script/main.py remove libhoudini
    sudo /tmp/waydroid_script/venv/bin/python /tmp/waydroid_script/main.py install {libndk,widevine}
    sudo rm -rf /tmp/waydroid_script
    sudo mkdir -p /var/lib/waydroid/overlay/system/lib64
    sudo cp /usr/share/ublue-os/gablue/waydroid/libndk_fixer.so /var/lib/waydroid/overlay/system/lib64
    sudo sed -i 's/ro.dalvik.vm.native.bridge=libndk_translation.so/ro.dalvik.vm.native.bridge=libndk_fixer.so/g' /var/lib/waydroid/waydroid_base.prop
    sudo sed -i "s/ro.hardware.gralloc=.*/ro.hardware.gralloc=${WAYDROID_GRALLOC:-minigbm_gbm_mesa}/g" /var/lib/waydroid/waydroid_base.prop
    waydroid session stop

# Configure waydroid manualy
waydroid-configure-custom:
    sudo systemctl stop waydroid-container
    waydroid session start &
    sleep 20
    sudo mkdir -p /var/lib/waydroid/overlay/system/etc/init
    sudo cp /usr/share/ublue-os/gablue/waydroid/audio.rc /var/lib/waydroid/overlay/system/etc/init/
    sudo mkdir -p /var/lib/waydroid/overlay/system/etc
    sudo cp /usr/share/ublue-os/gablue/waydroid/hosts /var/lib/waydroid/overlay/system/etc
    waydroid prop set persist.waydroid.udev true
    waydroid prop set persist.waydroid.uevent true
    sudo rm -rf /tmp/waydroid_script
    git clone https://github.com/ublue-os/waydroid_script.git --depth 1 /tmp/waydroid_script
    python -m venv /tmp/waydroid_script/venv
    source /tmp/waydroid_script/venv/bin/activate
    /tmp/waydroid_script/venv/bin/pip install -r /tmp/waydroid_script/requirements.txt
    sudo /tmp/waydroid_script/venv/bin/python /tmp/waydroid_script/main.py
    sudo rm -rf /tmp/waydroid_script
    sudo sed -i "s/ro.hardware.gralloc=.*/ro.hardware.gralloc=${WAYDROID_GRALLOC:-minigbm_gbm_mesa}/g" /var/lib/waydroid/waydroid_base.prop
    waydroid session stop

# Remove waydroid
waydroid-remove:
    sudo systemctl stop waydroid-container.service
    bash -c 'sudo rm -rf /var/lib/waydroid /home/.waydroid ~/waydroid ~/.share/waydroid ~/.local/share/applications/*aydroid* ~/.local/share/waydroid'
    update-desktop-database ~/.local/share/applications

# Set waydroid multi windows mode
waydroid-multiwin:
    sudo systemctl stop waydroid-container
    sudo systemctl start waydroid-container
    waydroid session stop
    waydroid session start &
    sleep 20
    waydroid prop set persist.waydroid.multi_windows true
    waydroid session stop
    sudo systemctl stop waydroid-container
    cp /usr/share/ublue-os/gablue/desktops/WaydroidDesktop.desktop ~/.local/share/applications/
    update-desktop-database ~/.local/share/applications

# Set waydroid only one window mode
waydroid-onewin:
    sudo systemctl stop waydroid-container
    sudo systemctl start waydroid-container
    waydroid session stop
    waydroid session start &
    sleep 20
    waydroid prop set persist.waydroid.multi_windows false
    waydroid session stop
    sudo systemctl stop waydroid-container
    rm -f ~/.local/share/applications/WaydroidDesktop.desktop
    update-desktop-database ~/.local/share/applications

