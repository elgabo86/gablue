# Enable tailscale + firewall rules & connect to private network
tailscale-up:
    #!/usr/bin/bash
    sudo systemctl enable --now tailscaled.service &&
    sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent &&
    sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent &&
    sudo tailscale up --operator=$USER --login-server https://headscale.gabserv.duckdns.org &&
    clear &&
    echo "Tailscale is ready!" ||
    echo "Error..."

# Update gablue and flatpaks
gablue-update:
    #!/usr/bin/bash
    /usr/libexec/gablue-update

# Enable displaylink
displaylink-enable:
    #!/usr/bin/bash
    sudo systemctl enable --now displaylink.service &&
    clear &&
    echo "Displaylink is now activated" ||
    echo "Error..."

# Disable displaylink
displaylink-disable:
    #!/usr/bin/bash
    sudo systemctl disable --now displaylink.service &&
    clear &&
    echo "Displaylink is now disabled" ||
    echo "Error..."

# Install/Reinstall custom Archlinux
distrobox-archgab:
    #!/usr/bin/bash
    /usr/libexec/archgab-update

# Rebase Gablue to main non nvidia
gablue-rebase-main:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main:latest

# Rebase Gablue to nvidia variant
gablue-rebase-nvidia:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest

# Add amd kargs to have advanced settings in CoreCtrl
amd-corectrl-set-kargs:
    #!/usr/bin/bash
    echo 'Setting needed kargs for CoreCtrl...'
    rpm-ostree kargs --append="amdgpu.ppfeaturemask=0xffffffff"
    echo "ALL DONE!!!"

# Remove cpu security,systemctl enable scx-lavd.service boost performance
mitigations-off:
    #!/usr/bin/bash
    rpm-ostree kargs --append-if-missing=mitigations=off
    echo "ALL DONE!!!"

# Enable cpu security, more secure
mitigations-on:
    #!/usr/bin/bash
    rpm-ostree kargs --delete-if-present=mitigations=off
    echo "ALL DONE!!!"

# Enable fix id cpu for games like Hogwarts
cpuid-fix-on:
    #!/usr/bin/bash
    rpm-ostree kargs --append-if-missing=clearcpuid=514
    echo "ALL DONE!!!"

# Disable fix id cpu for games like Hogwarts
cpuid-fix-off:
    #!/usr/bin/bash
    rpm-ostree kargs --delete-if-present=clearcpuid=514
    echo "ALL DONE!!!"

# Install ble and atuin, for history bash sync and more
atuin-setup:
    #!/usr/bin/bash
    curl -L -o ble-nightly.tar.xz https://github.com/akinomyoga/ble.sh/releases/download/nightly/ble-nightly.tar.xz &&
    tar xJf ble-nightly.tar.xz &&
    bash ble-nightly/ble.sh --install ~/.local/share &&
    rm -rf ble-nightly &&
    rm -f ble-nightly.tar.xz &&
    echo 'source ~/.local/share/blesh/ble.sh' >> ~/.bashrc &&
    echo 'eval "$(atuin init bash)"' >> ~/.bashrc &&
    echo "Atuin is ready!" ||
    echo "Error..."

# Enable ssh remote connection
ssh-on:
    #!/usr/bin/bash
    sudo systemctl enable sshd.service --now
    echo "ALL DONE!!!"

# Disable ssh remote connection
ssh-off:
    #!/usr/bin/bash
    sudo systemctl disable sshd.service --now
    echo "ALL DONE!!!"

# Enable update notification
show-update:
    #!/usr/bin/bash
    sudo sed -i 's/dbus_notify = false/dbus_notify = true/g' /etc/ublue-update/ublue-update.toml
    echo "ALL DONE!!!"

# Disable update notification
hide-update:
    #!/usr/bin/bash
    sudo sed -i 's/dbus_notify = true/dbus_notify = false/g' /etc/ublue-update/ublue-update.toml
    echo "ALL DONE!!!"

# Hide grub menu on boot
hide-grub:
    #!/usr/bin/bash
    sudo sed -i 's/GRUB_TIMEOUT=5/GRUB_TIMEOUT=0/g' /etc/default/grub &&
    echo 'GRUB_TIMEOUT_STYLE=hidden' | sudo tee -a /etc/default/grub 1>/dev/null &&
    echo 'GRUB_HIDDEN_TIMEOUT=1' | sudo tee -a /etc/default/grub 1>/dev/null &&
    if [ -d /sys/firmware/efi ]; then
        sudo grub2-mkconfig -o /etc/grub2-efi.cfg
    else
        sudo grub2-mkconfig -o /etc/grub2.cfg
    fi &&
    echo "ALL DONE!!!"

# Show grub menu on boot
show-grub:
    #!/usr/bin/bash
    sudo sed -i '/GRUB_HIDDEN_TIMEOUT=1/d' /etc/default/grub &&
    sudo sed -i '/GRUB_TIMEOUT_STYLE=hidden/d' /etc/default/grub &&
    sudo sed -i 's/GRUB_TIMEOUT=0/GRUB_TIMEOUT=5/g' /etc/default/grub &&
    if [ -d /sys/firmware/efi ]; then
        sudo grub2-mkconfig -o /etc/grub2-efi.cfg
    else
        sudo grub2-mkconfig -o /etc/grub2.cfg
    fi &&
    echo "ALL DONE!!!"

# Set sound in 432hz
sound-432:
   #!/usr/bin/bash
    cp -rf /usr/share/pipewire ~/.config &&
    sed -i 's/#default.clock.rate          = 48000/default.clock.rate          = 43200/g' ~/.config/pipewire/pipewire.conf &&
    sed -i 's/#default.clock.allowed-rates = \[ 48000 \]/default.clock.allowed-rates = [ 43200 ]/g' ~/.config/pipewire/pipewire.conf &&
    sed -i 's\#node.latency       = 1024/48000\node.latency       = 2048/48000\g' ~/.config/pipewire/jack.conf &&
    systemctl --user restart pipewire &&
    echo "The sound is now set to 432hz"

# Reset sound config
sound-reset:
    #!/usr/bin/bash
    rm -rf ~/.config/pipewire &&
    systemctl --user restart pipewire &&
    echo "The sound is now at the default frequency"

# Create gablue install iso
create-iso-main:
    #!/usr/bin/bash
    sudo podman run --rm --rmi --privileged --volume $(xdg-user-dir DOWNLOAD):/build-container-installer/build --security-opt label=disable ghcr.io/jasonn3/build-container-installer:latest VERSION=40 IMAGE_REPO=ghcr.io/elgabo86 IMAGE_NAME=gablue-main IMAGE_TAG=latest VARIANT=Kinoite ISO_NAME=build/gablue-main.iso &&
    echo "The gablue iso is ready in your download folder"

# Create gablue install iso (nvidia)
create-iso-nvidia:
    #!/usr/bin/bash
    sudo podman run --rm --rmi --privileged --volume $(xdg-user-dir DOWNLOAD):/build-container-installer/build --security-opt label=disable ghcr.io/jasonn3/build-container-installer:latest VERSION=40 IMAGE_REPO=ghcr.io/elgabo86 IMAGE_NAME=gablue-nvidia IMAGE_TAG=latest VARIANT=Kinoite ISO_NAME=build/gablue-nvidia.iso &&
    echo "The gablue iso is ready in your download folder"

# Install/Update Ollama+openwebui (cpu only)
ollama-cpu:
    #!/usr/bin/bash
    podman kill --all &&
    podman create --pull always --replace --name ollama -p 11434:11434 -v ollama:/root/.ollama docker.io/ollama/ollama:latest &&
    podman create -p 127.0.0.1:3000:8080 --network=pasta:-T,11434 --add-host=ollama.local:127.0.0.1 --env 'OLLAMA_BASE_URL=http://ollama.local:11434' --env 'ANONYMIZED_TELEMETRY=False' -v open-webui:/app/backend/data --label io.containers.autoupdate=registry --replace --name open-webui --pull always ghcr.io/open-webui/open-webui:main &&
    podman start ollama open-webui &&
    echo "ALL DONE!!!"

# Install/Update Ollama+openwebui (nvidia)
ollama-nvidia:
    #!/usr/bin/bash
    podman kill --all &&
    podman create --pull always --replace --name ollama -p 11434:11434 -v ollama:/root/.ollama --security-opt label=disable --device nvidia.com/gpu=all docker.io/ollama/ollama:latest &&
    podman create -p 127.0.0.1:3000:8080 --network=pasta:-T,11434 --add-host=ollama.local:127.0.0.1 --env 'OLLAMA_BASE_URL=http://ollama.local:11434' --env 'ANONYMIZED_TELEMETRY=False' -v open-webui:/app/backend/data --label io.containers.autoupdate=registry --replace --name open-webui --pull always ghcr.io/open-webui/open-webui:main &&
    podman start ollama open-webui &&
    echo "ALL DONE!!!"

# Install/Update Ollama+openwebui (amd)
ollama-amd:
    #!/usr/bin/bash
    podman kill --all &&
    podman create --pull always --name ollama -p 11434:11434 -v ollama:/root/.ollama --security-opt label=disable  --device /dev/dri --device /dev/kfd  --replace --env HSA_OVERRIDE_GFX_VERSION=10.3.0 docker.io/ollama/ollama:rocm &&
    podman create -p 127.0.0.1:3000:8080 --network=pasta:-T,11434 --add-host=ollama.local:127.0.0.1 --env 'OLLAMA_BASE_URL=http://ollama.local:11434' --env 'ANONYMIZED_TELEMETRY=False' -v open-webui:/app/backend/data --label io.containers.autoupdate=registry --replace --name open-webui --pull always ghcr.io/open-webui/open-webui:main &&
    podman start ollama open-webui &&
    echo "ALL DONE!!!"

# Remove Ollama+openwebui and models data
ollama-remove:
    #!/usr/bin/bash
    podman kill --all &&
    podman rm -f ollama open-webui &&
    podman volume rm -f ollama open-webui &&
    podman rmi --all &&
    echo "ALL DONE!!!"

# Install pyenv
pyenv-setup:
    #!/usr/bin/bash
    git clone https://github.com/pyenv/pyenv.git ~/.pyenv &&
    cd ~/.pyenv && src/configure && make -C src &&
    echo "ALL DONE!!!"

# Update pyenv
pyenv-update:
    #!/usr/bin/bashh
    git -C ~/.pyenv pull &&
    echo "ALL DONE!!!"

# Remove pyenv
pyenv-remove:
    #!/usr/bin/bash
    rm -rf ~/.pyenv &&
    echo "ALL DONE!!!"

# Remove waydroid
waydroid-remove:
    #!/usr/bin/bash
    sudo systemctl stop waydroid-container.service &&
    bash -c 'sudo rm -rf /var/lib/waydroid /home/.waydroid ~/waydroid ~/.share/waydroid ~/.local/share/applications/*aydroid* ~/.local/share/waydroid' &&
    update-desktop-database ~/.local/share/applications &&
    echo "ALL DONE!!!"

# Install default bottle and add default bottles folder with data user redirection and .desktop extras
windows-install:
    #!/usr/bin/bash
    /usr/libexec/windows-update

# Remove default bottle but keep user data
windows-remove:
    #!/usr/bin/bash
    pgrep -f bottles |xargs -n1 kill -9 &
    sleep 2 &&
    rm -rf ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def* &&
    rm ~/Windows/WinDrive &&
    rm ~/.local/share/applications/com.usebottles.bottles.desktop &&
    rm ~/.local/share/applications/Windows.desktop &&
    rm ~/.local/share/applications/WindowsFix.desktop &&
    rm ~/.local/share/applications/WindowsTerminal.desktop &&
    rm ~/.local/share/applications/KillWindows.desktop &&
    rm ~/.local/share/kio/servicemenus/CreateWinShortcut.desktop &&
    rm ~/.local/share/kio/servicemenus/InstallWinReg.desktop &&
    update-desktop-database ~/.local/share/applications &&
    echo "ALL DONE!!!"

# Reset brew installation
brew-reset:
    #!/usr/bin/bash
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
    rm -f ~/.config/kwinoutputconfig.json &&
    killall -9 kwin_wayland

# Add and connect a shared protected 5ghz wifi network
hotspot-add:
    #!/usr/bin/bash
    nmcli device wifi hotspot con-name Hotspot-5ghz ssid Hotspot-5ghz band a channel 161 password 4476525555 &&
    echo "Done! Password is 4476525555"

# Remove the shared protected 5ghz wifi network
hotspot-remove:
    #!/usr/bin/bash
    nmcli connection delete Hotspot-5ghz &&
    echo "Hotspot is removed"

# Fix hdmi/dp/usb audio drop
audio-fix-enable:
    #!/usr/bin/bash
    mkdir -p ~/.config/wireplumber/wireplumber.conf.d/ &&
    cp -f /usr/share/ublue-os/gablue/fixes/50-sound-drop-fix.conf ~/.config/wireplumber/wireplumber.conf.d/ &&
    systemctl restart --user pipewire.socket wireplumber.service &&
    echo "Audio fix is installed"

# Disable fix for hdmi/dp/usb audio drop
audio-fix-disable:
    rm -f ~/.config/wireplumber/wireplumber.conf.d/50-sound-drop-fix.conf &&
    systemctl restart --user pipewire.socket wireplumber.service &&
    echo "Audio fix is removed"
