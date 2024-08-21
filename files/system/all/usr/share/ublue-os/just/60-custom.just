# Enable tailscale + firewall rules & connect to private network
tailscale-up:
    #!/usr/bin/bash
    sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent
    sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent
    sudo tailscale up --operator=$USER --login-server https://headscale.gabserv.duckdns.org
    clear
    echo "ALL DONE!!!"

# Install/Reinstall custom Archlinux
distrobox-archgab:
    #!/usr/bin/bash
    podman kill --all
    distrobox-rm -f archgab
    distrobox-create --pull latest --name archgab --nvidia --image ghcr.io/elgabo86/archgab:latest
    clear
    echo "ALL DONE!!!"

# Rebase Gablue to main non nvidia
gablue-rebase-main:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main:latest

# Rebase Gablue to nvidia variant
gablue-rebase-nvidia:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest

# Add custom flatpak rules for theme, vk capture, mangohud config and bottles permissions
flatpak-custom-overrides:
    #!/usr/bin/bash
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
    clear
    echo "ALL DONE!!!"

# Add amd kargs to have advanced settings in CoreCtrl
amd-corectrl-set-kargs:
    #!/usr/bin/bash
    echo 'Setting needed kargs for CoreCtrl...'
    rpm-ostree kargs --append="amdgpu.ppfeaturemask=0xffffffff"
    clear
    echo "ALL DONE!!!"

# Remove cpu security,systemctl enable scx-lavd.service boost performance
mitigations-off:
    #!/usr/bin/bash
    rpm-ostree kargs --append-if-missing=mitigations=off
    clear
    echo "ALL DONE!!!"

# Enable cpu security, more secure
mitigations-on:
    #!/usr/bin/bash
    rpm-ostree kargs --delete-if-present=mitigations=off
    clear
    echo "ALL DONE!!!"

# Add flatpak custom rules and remove cpu security
gablue-setup:
    #!/usr/bin/bash
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
    clear
    echo "ALL DONE!!!"

# Install ble and atuin, for history bash sync and more
atuin-setup:
    #!/usr/bin/bash
    curl -L -o ble-nightly.tar.xz https://github.com/akinomyoga/ble.sh/releases/download/nightly/ble-nightly.tar.xz
    tar xJf ble-nightly.tar.xz
    bash ble-nightly/ble.sh --install ~/.local/share
    rm -rf ble-nightly
    rm -f ble-nightly.tar.xz
    echo 'source ~/.local/share/blesh/ble.sh' >> ~/.bashrc
    echo 'eval "$(atuin init bash)"' >> ~/.bashrc
    clear
    echo "ALL DONE!!!"

# Enable ssh remote connection
ssh-on:
    #!/usr/bin/bash
    sudo systemctl enable sshd.service --now
    clear
    echo "ALL DONE!!!"

# Disable ssh remote connection
ssh-off:
    #!/usr/bin/bash
    sudo systemctl disable sshd.service --now
    clear
    echo "ALL DONE!!!"

# Enable update notification
show-update:
    #!/usr/bin/bash
    sudo sed -i 's/dbus_notify = false/dbus_notify = true/g' /etc/ublue-update/ublue-update.toml
    clear
    echo "ALL DONE!!!"

# Disable update notification
hide-update:
    #!/usr/bin/bash
    sudo sed -i 's/dbus_notify = true/dbus_notify = false/g' /etc/ublue-update/ublue-update.toml
    clear
    echo "ALL DONE!!!"

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
    clear
    echo "ALL DONE!!!"

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
    clear
    echo "ALL DONE!!!"

# Set sound in 432hz
sound-432:
    #!/usr/bin/env bash
    cp -rf /usr/share/pipewire ~/.config
    sed -i 's/#default.clock.rate          = 48000/default.clock.rate          = 43200/g' ~/.config/pipewire/pipewire.conf
    sed -i 's/#default.clock.allowed-rates = \[ 48000 \]/default.clock.allowed-rates = [ 43200 ]/g' ~/.config/pipewire/pipewire.conf
    sed -i 's\#node.latency       = 1024/48000\node.latency       = 2048/48000\g' ~/.config/pipewire/jack.conf
    systemctl --user restart pipewire
    clear
    echo "The sound is now set to 432hz"

# Reset sound config
sound-reset:
    #!/usr/bin/env bash
    rm -rf ~/.config/pipewire
    systemctl --user restart pipewire
    clear
    echo "The sound is now at the default frequency"

# Create gablue install iso
create-iso-main:
    #!/usr/bin/env bash
    sudo podman run --rm --rmi --privileged --volume $(xdg-user-dir DOWNLOAD):/build-container-installer/build --security-opt label=disable ghcr.io/jasonn3/build-container-installer:latest VERSION=40 IMAGE_REPO=ghcr.io/elgabo86 IMAGE_NAME=gablue-main IMAGE_TAG=latest VARIANT=Kinoite FLATPAK_REMOTE_NAME=flathub ISO_NAME=build/gablue-main.iso FLATPAK_REMOTE_REFS="app/com.usebottles.bottles/x86_64/stable app/io.freetubeapp.FreeTube/x86_64/stable app/org.mozilla.firefox/x86_64/stable app/org.atheme.audacious/x86_64/stable app/org.libreoffice.LibreOffice/x86_64/stable app/org.kde.haruna/x86_64/stable app/org.kde.haruna/x86_64/stable app/io.github.flattool.Warehouse/x86_64/stable app/io.missioncenter.MissionCenter/x86_64/stable app/io.github.dvlv.boxbuddyrs/x86_64/stable runtime/org.gnome.Platform/x86_64/45 runtime/org.freedesktop.Platform/x86_64/23.08 runtime/org.kde.Platform/x86_64/6.6 runtime/org.kde.Platform/x86_64/5.15-23.08 runtime/org.gnome.Platform/x86_64/46"
    clear
    echo "The gablue iso is ready in your download folder"

# Create gablue install iso (nvidia)
create-iso-nvidia:
    #!/usr/bin/env bash
    sudo podman run --rm --rmi --privileged --volume $(xdg-user-dir DOWNLOAD):/build-container-installer/build --security-opt label=disable ghcr.io/jasonn3/build-container-installer:latest VERSION=40 IMAGE_REPO=ghcr.io/elgabo86 IMAGE_NAME=gablue-nvidia IMAGE_TAG=latest VARIANT=Kinoite FLATPAK_REMOTE_NAME=flathub ISO_NAME=build/gablue-nvidia.iso FLATPAK_REMOTE_REFS="app/com.usebottles.bottles/x86_64/stable app/io.freetubeapp.FreeTube/x86_64/stable app/org.mozilla.firefox/x86_64/stable app/org.atheme.audacious/x86_64/stable app/org.libreoffice.LibreOffice/x86_64/stable app/org.kde.haruna/x86_64/stable app/org.kde.haruna/x86_64/stable app/io.github.flattool.Warehouse/x86_64/stable app/io.missioncenter.MissionCenter/x86_64/stable app/io.github.dvlv.boxbuddyrs/x86_64/stable runtime/org.gnome.Platform/x86_64/45 runtime/org.freedesktop.Platform/x86_64/23.08 runtime/org.kde.Platform/x86_64/6.6 runtime/org.kde.Platform/x86_64/5.15-23.08 runtime/org.gnome.Platform/x86_64/46"
    clear
    echo "The gablue iso is ready in your download folder"

# Install/Update Ollama+openwebui (cpu only)
ollama-cpu:
    #!/usr/bin/env bash
    podman kill --all
    podman create --pull always --replace --name ollama -p 11434:11434 -v ollama:/root/.ollama docker.io/ollama/ollama:latest
    podman create -p 127.0.0.1:3000:8080 --network=pasta:-T,11434 --add-host=ollama.local:127.0.0.1 --env 'OLLAMA_BASE_URL=http://ollama.local:11434' --env 'ANONYMIZED_TELEMETRY=False' -v open-webui:/app/backend/data --label io.containers.autoupdate=registry --replace --name open-webui --pull always ghcr.io/open-webui/open-webui:main
    podman start ollama open-webui
    clear
    echo "ALL DONE!!!"

# Install/Update Ollama+openwebui (nvidia)
ollama-nvidia:
    #!/usr/bin/env bash
    podman kill --all
    podman create --pull always --replace --name ollama -p 11434:11434 -v ollama:/root/.ollama --security-opt label=disable --device nvidia.com/gpu=all docker.io/ollama/ollama:latest
    podman create -p 127.0.0.1:3000:8080 --network=pasta:-T,11434 --add-host=ollama.local:127.0.0.1 --env 'OLLAMA_BASE_URL=http://ollama.local:11434' --env 'ANONYMIZED_TELEMETRY=False' -v open-webui:/app/backend/data --label io.containers.autoupdate=registry --replace --name open-webui --pull always ghcr.io/open-webui/open-webui:main
    podman start ollama open-webui
    clear
    echo "ALL DONE!!!"

# Install/Update Ollama+openwebui (nvidia)
ollama-amd:
    #!/usr/bin/env bash
    podman kill --all
    podman create --pull always --name ollama -p 11434:11434 -v ollama:/root/.ollama --security-opt label=disable  --device /dev/dri --device /dev/kfd  --replace --env HSA_OVERRIDE_GFX_VERSION=10.3.0 docker.io/ollama/ollama:rocm
    podman create -p 127.0.0.1:3000:8080 --network=pasta:-T,11434 --add-host=ollama.local:127.0.0.1 --env 'OLLAMA_BASE_URL=http://ollama.local:11434' --env 'ANONYMIZED_TELEMETRY=False' -v open-webui:/app/backend/data --label io.containers.autoupdate=registry --replace --name open-webui --pull always ghcr.io/open-webui/open-webui:main
    podman start ollama open-webui
    clear
    echo "ALL DONE!!!"

# Remove Ollama+openwebui and models data
ollama-remove:
    #!/usr/bin/env bash
    podman kill --all
    podman rm -f ollama open-webui
    podman volume rm -f ollama open-webui
    podman rmi --all
    clear
    echo "ALL DONE!!!"

# Install pyenv
pyenv-setup:
    #!/usr/bin/env bash
    git clone https://github.com/pyenv/pyenv.git ~/.pyenv
    cd ~/.pyenv && src/configure && make -C src
    clear
    echo "ALL DONE!!!"

# Update pyenv
pyenv-update:
    #!/usr/bin/env bash
    git -C ~/.pyenv pull
    clear
    echo "ALL DONE!!!"

# Remove pyenv
pyenv-remove:
    #!/usr/bin/env bash
    rm -rf ~/.pyenv
    clear
    echo "ALL DONE!!!"

# Install waydroid
waydroid-install:
    #!/usr/bin/env bash
    sudo systemctl start waydroid-container
    sudo waydroid init -c 'https://ota.waydro.id/system' -v 'https://ota.waydro.id/vendor'
    mkdir -p ~/.local/share/applications/
    cp /usr/share/ublue-os/gablue/desktops/KillWaydroid.desktop ~/.local/share/applications/
    update-desktop-database ~/.local/share/applications
    sudo systemctl stop waydroid-container
    clear
    echo "ALL DONE!!!"

# Configure waydroid for intel cpu
waydroid-configure-intel:
    #!/usr/bin/env bash
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
    sleep 20
    sudo /tmp/waydroid_script/venv/bin/python /tmp/waydroid_script/main.py install {libhoudini,widevine}
    sudo rm -rf /tmp/waydroid_script
    sudo sed -i "s/ro.hardware.gralloc=.*/ro.hardware.gralloc=${WAYDROID_GRALLOC:-minigbm_gbm_mesa}/g" /var/lib/waydroid/waydroid_base.prop
    waydroid session stop
    clear
    echo "ALL DONE!!!"

# Configure waydroid for amd cpu
waydroid-configure-amd:
    #!/usr/bin/env bash
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
    sleep 20
    sudo /tmp/waydroid_script/venv/bin/python /tmp/waydroid_script/main.py install {libndk,widevine}
    sudo rm -rf /tmp/waydroid_script
    sudo mkdir -p /var/lib/waydroid/overlay/system/lib64
    sudo cp /usr/share/ublue-os/gablue/waydroid/libndk_fixer.so /var/lib/waydroid/overlay/system/lib64
    sudo sed -i 's/ro.dalvik.vm.native.bridge=libndk_translation.so/ro.dalvik.vm.native.bridge=libndk_fixer.so/g' /var/lib/waydroid/waydroid_base.prop
    sudo sed -i "s/ro.hardware.gralloc=.*/ro.hardware.gralloc=${WAYDROID_GRALLOC:-minigbm_gbm_mesa}/g" /var/lib/waydroid/waydroid_base.prop
    waydroid session stop
    clear
    echo "ALL DONE!!!"

# Configure waydroid manualy
waydroid-configure-custom:
    #!/usr/bin/env bash
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
    clear
    echo "ALL DONE!!!"

# Remove waydroid
waydroid-remove:
    #!/usr/bin/env bash
    sudo systemctl stop waydroid-container.service
    bash -c 'sudo rm -rf /var/lib/waydroid /home/.waydroid ~/waydroid ~/.share/waydroid ~/.local/share/applications/*aydroid* ~/.local/share/waydroid'
    update-desktop-database ~/.local/share/applications
    clear
    echo "ALL DONE!!!"

# Set waydroid multi windows mode
waydroid-multiwin:
    #!/usr/bin/env bash
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
    clear
    echo "ALL DONE!!!"

# Set waydroid only one window mode
waydroid-onewin:
    #!/usr/bin/env bash
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
    clear
    echo "ALL DONE!!!"

# Add Suyu flatpak repo & install suyu
suyu-install:
    #!/usr/bin/env bash
    flatpak remote-add suyu-flatpak https://git.suyu.dev/suyu/flatpak/src/branch/main/org.suyu_emu.suyu.flatpakref
    flatpak install org.suyu_emu.suyu -y
    clear
    echo "ALL DONE!!!"

# Remove suyu and his repo
suyu-remove:
    #!/usr/bin/env bash
    flatpak remote-delete suyu-flatpak --force
    clear
    echo "ALL DONE!!!"

# Install default bottle and add default bottles folder with data user redirection and .desktop extras
windows-install:
    #!/usr/bin/env bash
    pgrep -f bottles |xargs -n1 kill -9 &
    sleep 2
    rm -rf ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def*
    mkdir -p ~/.var/app/com.usebottles.bottles/data/bottles/dxvk/ ~/.var/app/com.usebottles.bottles/data/bottles/runners/ ~/.var/app/com.usebottles.bottles/data/bottles/vkd3d/ ~/.var/app/com.usebottles.bottles/data/bottles/nvapi/  ~/.var/app/com.usebottles.bottles/data/bottles/vkd3d/ ~/.var/app/com.usebottles.bottles/data/bottles/latencyflex/
    wget -O ~/.var/app/com.usebottles.bottles/data/bottles/runners/soda-9.0-1-x86_64.tar.xz  https://github.com/bottlesdevs/wine/releases/download/soda-9.0-1/soda-9.0-1-x86_64.tar.xz
    tar -xf ~/.var/app/com.usebottles.bottles/data/bottles/runners/soda-9.0-1-x86_64.tar.xz -C ~/.var/app/com.usebottles.bottles/data/bottles/runners/
    rm -rf ~/.var/app/com.usebottles.bottles/data/bottles/runners/soda-9.0-1/
    mv ~/.var/app/com.usebottles.bottles/data/bottles/runners/soda-9.0-1-x86_64 ~/.var/app/com.usebottles.bottles/data/bottles/runners/soda-9.0-1
    rm -f ~/.var/app/com.usebottles.bottles/data/bottles/runners/soda-9.0-1-x86_64.tar.xz
    rm -rf ~/.var/app/com.usebottles.bottles/data/bottles/dxvk/*
    wget -O ~/.var/app/com.usebottles.bottles/data/bottles/dxvk/dxvk-gplasync-v2.4-1.tar.gz https://gitlab.com/Ph42oN/dxvk-gplasync/-/raw/main/releases/dxvk-gplasync-v2.4-1.tar.gz
    tar -xf ~/.var/app/com.usebottles.bottles/data/bottles/dxvk/dxvk-gplasync-v2.4-1.tar.gz -C ~/.var/app/com.usebottles.bottles/data/bottles/dxvk/
    rm -f ~/.var/app/com.usebottles.bottles/data/bottles/dxvk/dxvk-gplasync-v2.4-1.tar.gz
    wget -O ~/.var/app/com.usebottles.bottles/data/bottles/vkd3d/vkd3d-proton-2.13.tar.gz https://github.com/bottlesdevs/components/releases/download/vkd3d-proton-2.13/vkd3d-proton-2.13.tar.gz
    tar -xf ~/.var/app/com.usebottles.bottles/data/bottles/vkd3d/vkd3d-proton-2.13.tar.gz -C ~/.var/app/com.usebottles.bottles/data/bottles/vkd3d/
    rm -f ~/.var/app/com.usebottles.bottles/data/bottles/vkd3d/vkd3d-proton-2.13.tar.gz
    wget -O ~/.var/app/com.usebottles.bottles/data/bottles/nvapi/dxvk-nvapi-v0.7.1.tar.gz https://github.com/bottlesdevs/components/releases/download/dxvk-nvapi-v0.7.1/dxvk-nvapi-v0.7.1.tar.gz
    tar -xf ~/.var/app/com.usebottles.bottles/data/bottles/nvapi/dxvk-nvapi-v0.7.1.tar.gz -C ~/.var/app/com.usebottles.bottles/data/bottles/nvapi/
    rm -f ~/.var/app/com.usebottles.bottles/data/bottles/nvapi/dxvk-nvapi-v0.7.1.tar.gz
    wget -O ~/.var/app/com.usebottles.bottles/data/bottles/latencyflex/latencyflex-v0.1.1-2-3bc9636.tar.gz https://github.com/bottlesdevs/components/releases/download/latencyflex-v0.1.1-2-3bc9636/latencyflex-v0.1.1-2-3bc9636.tar.gz
    tar -xf ~/.var/app/com.usebottles.bottles/data/bottles/latencyflex/latencyflex-v0.1.1-2-3bc9636.tar.gz -C ~/.var/app/com.usebottles.bottles/data/bottles/latencyflex/
    rm -f ~/.var/app/com.usebottles.bottles/data/bottles/latencyflex/latencyflex-v0.1.1-2-3bc9636.tar.gz
    flatpak run --command=bottles-cli com.usebottles.bottles new --bottle-name "def" --environment gaming
    flatpak override com.usebottles.bottles --user --filesystem=~/Windows
    mkdir -p ~/Windows/UserData ~/Windows/SteamData ~/Windows/Games ~/Windows/Applications ~/Windows/Tools
    rm -rf ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/drive_c/users/
    ln -s ~/Windows/UserData/ ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/drive_c/users
    ln -s ~/Windows/SteamData/ ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/drive_c/ProgramData/Steam
    ln -s ~/Windows/Applications/ ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/drive_c/
    ln -s ~/Windows/Games/ ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/drive_c/
    ln -f -s -T ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/drive_c/ ~/Windows/WinDrive
    flatpak run --command=bottles-cli com.usebottles.bottles reg add -b def -k "HKEY_LOCAL_MACHINE\System\CurrentControlSet\Services\winebus" -v "Enable SDL" -d 1 -t "REG_DWORD"
    flatpak run --command=bottles-cli com.usebottles.bottles reg add -b def -k "HKEY_LOCAL_MACHINE\System\CurrentControlSet\Services\winebus" -v "DisableHidraw" -d 1 -t "REG_DWORD"
    cp /usr/share/ublue-os/gablue/desktops/com.usebottles.bottles.desktop ~/.local/share/applications/
    cp /usr/share/ublue-os/gablue/desktops/Windows.desktop ~/.local/share/applications/
    cp /usr/share/ublue-os/gablue/desktops/WindowsTerminal.desktop ~/.local/share/applications/
    cp /usr/share/ublue-os/gablue/desktops/KillWindows.desktop ~/.local/share/applications/
    update-desktop-database ~/.local/share/applications
    xdg-mime default Windows.desktop application/x-ms-dos-executable
    xdg-mime default Windows.desktop application/x-msi
    xdg-mime default Windows.desktop application/x-ms-shortcut
    xdg-mime default Windows.desktop application/x-wine-extension-msp
    xdg-mime default Windows.desktop application/x-bat
    wget -O ~/Windows/Tools/vc2015_2022-x86.exe https://aka.ms/vs/17/release/VC_redist.x86.exe
    wget -O ~/Windows/Tools/vc2015_2022-x64.exe https://aka.ms/vs/17/release/VC_redist.x64.exe
    wget -O ~/Windows/Tools/runtime-6.0.16-x64.exe https://download.visualstudio.microsoft.com/download/pr/85473c45-8d91-48cb-ab41-86ec7abc1000/83cd0c82f0cde9a566bae4245ea5a65b/windowsdesktop-runtime-6.0.16-win-x64.exe
    wget -O ~/Windows/Tools/wine-mono-9.2.0-x86.msi https://dl.winehq.org/wine/wine-mono/9.2.0/wine-mono-9.2.0-x86.msi
    flatpak run --command=bottles-cli com.usebottles.bottles run -b def -e ~/Windows/Tools/vc2015_2022-x86.exe /quiet /norestart
    flatpak run --command=bottles-cli com.usebottles.bottles run -b def -e ~/Windows/Tools/vc2015_2022-x64.exe /quiet /norestart
    flatpak run --command=bottles-cli com.usebottles.bottles run -b def -e ~/Windows/Tools/runtime-6.0.16-x64.exe /quiet
    flatpak run --command=bottles-cli com.usebottles.bottles run -b def -e ~/Windows/Tools/wine-mono-9.2.0-x86.msi /quiet
    flatpak run --env=WINEPREFIX=/var/data/bottles/bottles/def --env=WINE=/var/data/bottles/runners/soda-9.0-1/bin/wine --command=winetricks com.usebottles.bottles -q corefonts d3dcompiler_43 d3dcompiler_47 d3dx9 msls31 vb6run
    rm -rf ~/Windows/Tools
    flatpak install runtime/org.freedesktop.Platform.VulkanLayer.MangoHud/x86_64/23.08 -y
    flatpak install runtime/org.freedesktop.Platform.VulkanLayer.OBSVkCapture/x86_64/23.08 -y
    flatpak install runtime/org.freedesktop.Platform.VulkanLayer.gamescope/x86_64/23.08 -y
    mkdir -p ~/.config/MangoHud/
    cp /usr/share/ublue-os/gablue/MangoHud.conf ~/.config/MangoHud/
    flatpak run --command=bottles-cli com.usebottles.bottles edit -b def --params mangohud:true
    flatpak run --command=bottles-cli com.usebottles.bottles edit -b def --params gamemode:true
    clear
    echo "ALL DONE!!!"

# Remove default bottle but keep user data
windows-remove:
    #!/usr/bin/env bash
    pgrep -f bottles |xargs -n1 kill -9 &
    sleep 2
    rm -rf ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def*
    rm ~/Windows/WinDrive
    rm ~/.local/share/applications/com.usebottles.bottles.desktop
    rm ~/.local/share/applications/Windows.desktop
    rm ~/.local/share/applications/KillWindows.desktop
    update-desktop-database ~/.local/share/applications
    clear
    echo "ALL DONE!!!"

# Reset brew installation
brew-reset:
    #!/usr/bin/env bash
    if [[ -d "${HOME}/cache/Homebrew/" ]]; then
        echo "Removing '$HOME/cache/Homebrew/' directory"
        rm -r "${HOME}/cache/Homebrew/"
    else
        echo "'${HOME}/cache/Homebrew/' directory is already removed"
    fi
    if [[ -d "/var/lib/homebrew/" ]]; then
        echo "Removing '/var/lib/homebrew/' directory"
        sudo rm -rf "/var/lib/homebrew/"
    else
        echo "'/var/lib/homebrew/' directory is already removed"
    fi
    if [[ -d "/var/cache/homebrew/" ]]; then
        echo "Removing '/var/cache/homebrew/' directory"
        sudo rm -rf "/var/cache/homebrew/"
    else
        echo "'/var/cache/homebrew/' directory is already removed"
    fi
    ## This is the main directory where brew is located
    if [[ -d "/var/home/linuxbrew/" ]]; then
        echo "Removing '/home/linuxbrew/' directory"
        sudo rm -rf "/var/home/linuxbrew/"
    else
        echo "'/home/linuxbrew/' directory is already removed"
    fi
    if [[ -f "/etc/.linuxbrew" ]]; then
        echo "Removing empty '/etc/.linuxbrew' file"
        sudo rm -f "/etc/.linuxbrew"
    else
        echo "'/etc/.linuxbrew' file is already removed"
    fi
    clear
    echo "ALL DONE!!!"
    echo "Restart computer to reuse brew"

# Enable special scheduler for gaming (experimental)
scx-enable:
    sudo systemctl enable scx.service --now

# Disable special scheduler for gaming
scx-disable:
    sudo systemctl disable scx.service --now

# Remove grub double entry
fix-grub-double-entry:
    #!/usr/bin/bash
    if [ -d /sys/firmware/efi ]; then
      sudo grub2-switch-to-blscfg
      sudo grub2-mkconfig -o /boot/efi/EFI/fedora/grub.cfg
    else
      block_device=$(lsblk -spnlo name $(grub2-probe --target=device /boot/grub2) | tail -n1)
      sudo grub2-install $block_device
      sudo touch /boot/grub2/.grub2-blscfg-supported
      sudo grub2-mkconfig -o /boot/grub2/grub.cfg
    fi

# Fix screen desactivation on kde plasma (until plasma 6.2)
fix-screen:
    #!/usr/bin/bash
    rm -f ~/.config/kwinoutputconfig.json
    killall -9 kwin_wayland

# Add and connect a shared protected 5ghz wifi network
hotspot-add:
    #!/usr/bin/bash
    nmcli device wifi hotspot con-name Hotspot-5ghz ssid Hotspot-5ghz band a channel 161 password 4476525555
    clear
    echo "Done! Password is 4476525555"

# Add and connect a shared protected 5ghz wifi network
hotspot-remove:
    #!/usr/bin/bash
    nmcli connection delete Hotspot-5ghz
    clear
    echo "Hotspot is removed"
