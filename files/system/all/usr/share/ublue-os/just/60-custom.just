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

# Rebase Gablue to nvidia-open variant
gablue-rebase-nvidia-open:
    rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia-open:latest

# Add amd kargs to have advanced settings in CoreCtrl
amd-corectrl-set-kargs:
    #!/usr/bin/bash
    echo 'Setting needed kargs for CoreCtrl...'
    rpm-ostree kargs --append-if-missing=$(printf 'amdgpu.ppfeaturemask=0x%x\n' "$(($(cat /sys/module/amdgpu/parameters/ppfeaturemask) | 0x4000))")
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
    echo 'eval "$(atuin init bash --disable-up-arrow)"' >> ~/.bashrc &&
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

# Create gablue install iso
create-iso-main:
    #!/usr/bin/bash
    sudo podman run --rm --rmi --privileged --volume $(xdg-user-dir DOWNLOAD):/build-container-installer/build --security-opt label=disable ghcr.io/jasonn3/build-container-installer:latest VERSION=42 IMAGE_REPO=ghcr.io/elgabo86 IMAGE_NAME=gablue-main IMAGE_TAG=latest VARIANT=Kinoite ISO_NAME=build/gablue-main.iso &&
    echo "The gablue iso is ready in your download folder"

# Create gablue install iso (nvidia)
create-iso-nvidia:
    #!/usr/bin/bash
    sudo podman run --rm --rmi --privileged --volume $(xdg-user-dir DOWNLOAD):/build-container-installer/build --security-opt label=disable ghcr.io/jasonn3/build-container-installer:latest VERSION=42 IMAGE_REPO=ghcr.io/elgabo86 IMAGE_NAME=gablue-nvidia IMAGE_TAG=latest VARIANT=Kinoite ISO_NAME=build/gablue-nvidia.iso &&
    echo "The gablue iso is ready in your download folder"

# Install pyenv
pyenv-setup:
    #!/usr/bin/bash
    git clone https://github.com/pyenv/pyenv.git ~/.pyenv &&
    cd ~/.pyenv && src/configure && make -C src &&
    echo "ALL DONE!!!"

# Update pyenv
pyenv-update:
    #!/usr/bin/bash
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

# Fix hdmi/dp/usb audio drop
audio-fix-enable:
    #!/usr/bin/bash
    mkdir -p ~/.config/wireplumber/wireplumber.conf.d/ &&
    cp -f /usr/share/ublue-os/gablue/fixes/50-sound-drop-fix.conf ~/.config/wireplumber/wireplumber.conf.d/ &&
    systemctl restart --user pipewire.socket wireplumber.service &&
    echo "Audio fix is installed"

# Disable fix for hdmi/dp/usb audio drop
audio-fix-disable:
    #!/usr/bin/bash
    rm -f ~/.config/wireplumber/wireplumber.conf.d/50-sound-drop-fix.conf &&
    systemctl restart --user pipewire.socket wireplumber.service &&
    echo "Audio fix is removed"

# Set recommended kernel parameters
kernel-setup:
    #!/usr/bin/bash
    rpm-ostree kargs --delete-if-present=preempt=full --append-if-missing=bluetooth.disable_ertm=1 &&
    echo "Done!"

# Enable wl legacy
wl-enable:
    #!/usr/bin/bash
    sudo rm -f /etc/modprobe.d/broadcom-wl-blacklist.conf
    sudo rm -f /etc/modprobe.d/default-disable-broadcom-wl.conf
    echo "Done! Please reboot!"

# Disable wl legacy
wl-disable:
    #!/usr/bin/bash
    sudo bash -c '> /etc/modprobe.d/broadcom-wl-blacklist.conf'
    sudo bash -c 'echo "blacklist wl" > /etc/modprobe.d/default-disable-broadcom-wl.conf'
    echo "Done! Please reboot!"

# enable or disable wake-on-lan functionality
toggle-wol ACTION="":
    #!/usr/bin/bash
    source /usr/lib/ujust/ujust.sh
    INTERFACE=$(ip link show | awk '/state UP/ {print $2}' | tr -d ':' | grep -E '^(en|eth)')
    if [[ -z "$INTERFACE" ]]; then
      echo -e "${bold}No active Ethernet interface found.${normal}"
      echo "Please ensure your Ethernet connection is enabled or connected."
      echo "Exiting without making any changes."
      exit 0
    fi
    CONFIG_FILE="/etc/udev/rules.d/81-wol.rules"
    SERVICE_FILE="/etc/systemd/system/force-wol.service"
    WOL_STATUS=$(sudo ethtool $INTERFACE | grep -P "^\s+Wake-on" | awk '{print $2}')
    CURRENT_STATE="Disabled"
    if [[ "$WOL_STATUS" == "g" ]]; then
      CURRENT_STATE="Enabled"
    fi
    OPTION="{{ ACTION }}"
    if [ "$OPTION" == "help" ]; then
      echo "Usage: ujust toggle-wol <option>"
      echo "  <option>: Specify the quick option to skip the prompt"
      echo "  Use 'enable' to enable Wake-on-LAN"
      echo "  Use 'disable' to disable Wake-on-LAN"
      echo "  Use 'force-enable' to force-enable Wake-on-LAN (persists across reboots)"
      exit 0
    elif [ -z "$OPTION" ]; then
      echo "Wake-on-LAN is currently: ${bold}${CURRENT_STATE}${normal}"
      echo "Enable, Disable Wake-on-LAN, Force-Enable, or Exit without saving?"
      echo "Note: Force-Enable will make WOL persist across reboots"
      OPTION=$(ugum choose Enable Disable Force-Enable Exit)
    fi
    if [[ "${OPTION,,}" == "enable" ]]; then
      echo "You chose to enable Wake-on-LAN."
      echo "Requesting root privileges..."
      sudo ethtool -s $INTERFACE wol g
      if ! grep -q "$INTERFACE" "$CONFIG_FILE" 2>/dev/null; then
        echo "Creating udev rule to make this setting persistent..."
        echo "ACTION==\"add\", SUBSYSTEM==\"net\", NAME==\"$INTERFACE\", RUN+=\"/usr/bin/ethtool -s \$name wol g\"" | sudo tee "$CONFIG_FILE" > /dev/null
      fi
      echo "Wake-on-LAN has been ${green}${bold}enabled${normal}."
    elif [[ "${OPTION,,}" == "disable" ]]; then
      echo "You chose to disable Wake-on-LAN."
      echo "Requesting root privileges..."
      sudo ethtool -s $INTERFACE wol d
      if [[ -f "$CONFIG_FILE" ]]; then
        echo "Removing udev rule to disable persistence..."
        sudo rm -f "$CONFIG_FILE"
      fi
      if systemctl is-enabled force-wol.service &>/dev/null; then
        sudo systemctl disable force-wol.service
        echo "Force-WOL service has been disabled."
      fi
      echo "Wake-on-LAN has been ${red}${bold}disabled${normal}."
    elif [[ "${OPTION,,}" == "force-enable" ]]; then
      echo "You chose to force-enable wake on LAN."
      echo "Requesting root privileges..."
      if [ -f "/etc/systemd/system/force-wol.service" ]; then
        echo "Removing deprecated service file"
        sudo rm "/etc/systemd/system/force-wol.service"
      fi
      sudo systemctl daemon-reload
      sudo systemctl enable --now force-wol.service
      echo "Wake on LAN force-enabled: ${green}${bold}force-wol.service${normal}"
    else
      echo "No changes were made."
    fi

# enable or disable a fix for 7th and 8th generation Intel chips not being able to sleep
toggle-i915-sleep-fix:
    #!/usr/bin/bash
    # Explain the purpose of the script
    echo -e "This script manages the i915.enable_dc kernel parameter, which controls a power-saving feature for Intel graphics"
    echo -e "Enabling this setting can reduce power consumption, but may cause issues like random reboots or failed suspend on certain devices"
    echo -e "Disabling it ensures stability at the cost of slightly higher power usage"
    # Get the current i915.enable_dc setting
    get_current_status() {
      local karg_status
      karg_status=$(cat /proc/cmdline | grep -o 'i915.enable_dc=[-0-9]' | cut -d= -f2)
      if [[ -z "$karg_status" ]]; then
        echo "Not Set"
      else
        echo "$karg_status"
      fi
    }
    # Toggle i915.enable_dc kernel parameter
    update_karg() {
      local new_value=$1
      if [[ $new_value -ge 0 && $new_value -le 4 ]]; then
        echo -e "\nYou are setting power-saving mode (i915.enable_dc=$new_value).\n"
        if [[ $new_value -eq 0 ]]; then
          echo -e "This disables power-saving mode and prioritizes stability.\n"
        elif [[ $new_value -eq 1 ]]; then
          echo -e "This enables basic power-saving mode but may cause minor stability issues.\n"
        elif [[ $new_value -ge 2 ]]; then
          echo -e "This enables higher levels of power-saving mode, which may impact stability further.\n"
        fi
      elif [[ $new_value -eq -1 ]]; then
        echo -e "\nYou are setting power-saving mode to auto (i915.enable_dc=-1).\n"
      else
        echo -e "\nInvalid value for i915.enable_dc. Please choose a valid value.\n"
        return
      fi
      rpm-ostree kargs --replace "i915.enable_dc=$new_value"
      echo -e "Kernel parameter updated. Reboot required to apply changes."
    }
    # Display current status
    current_status=$(get_current_status)
    echo -e "\nCurrent i915.enable_dc setting: $current_status\n"
    # Prompt user for action
    CHOICE=$(ugum choose "Set to Auto (i915.enable_dc=-1)" "Disable Power Saving (i915.enable_dc=0)" "Set to Level 1 (i915.enable_dc=1)" "Set to Level 2 (i915.enable_dc=2)" "Set to Level 3 (i915.enable_dc=3)" "Set to Level 4 (i915.enable_dc=4)" "Unset Parameter" "Exit without changes")
    case "$CHOICE" in
      "Set to Auto (i915.enable_dc=-1)")
        echo "Setting power-saving mode to auto (i915.enable_dc=-1)..."
        update_karg -1
        ;;
      "Disable Power Saving (i915.enable_dc=0)")
        echo "Disabling power-saving mode (i915.enable_dc=0)..."
        update_karg 0
        ;;
      "Set to Level 1 (i915.enable_dc=1)")
        echo "Setting power-saving mode to level 1 (i915.enable_dc=1)..."
        update_karg 1
        ;;
      "Set to Level 2 (i915.enable_dc=2)")
        echo "Setting power-saving mode to level 2 (i915.enable_dc=2)..."
        update_karg 2
        ;;
      "Set to Level 3 (i915.enable_dc=3)")
        echo "Setting power-saving mode to level 3 (i915.enable_dc=3)..."
        update_karg 3
        ;;
      "Set to Level 4 (i915.enable_dc=4)")
        echo "Setting power-saving mode to level 4 (i915.enable_dc=4)..."
        update_karg 4
        ;;
      "Unset Parameter")
        echo "Unsetting i915.enable_dc..."
        rpm-ostree kargs --delete "i915.enable_dc=[-0-9]"
        echo -e "Kernel parameter unset. Reboot required to apply changes."
        ;;
      "Exit without changes")
        echo "No changes made."
        ;;
      *)
        echo "Invalid choice. Exiting without changes."
        ;;
    esac

# enable or disable IWD as a replacement for wpa_supplicant
toggle-iwd:
    #!/usr/bin/bash
    # Explain the purpose of the script
    echo -e "This script manages enabling or disabling iwd as a replacement for wpa_supplicant for Wi-Fi networking."
    echo -e "Enabling this can improve throughput, mesh networking, and reduce latency increases when scanning for networks"
    echo -e "Disabling this can improve corporate or eduroam network compatibility"
    echo -e ""
    echo -e "WARNING: Changing this will remove all saved wifi networks"
    get_current_status() {
      if [[ -f "/etc/NetworkManager/conf.d/iwd.conf" ]]; then
        echo "Enabled"
      else
        echo "Disabled"
      fi
    }
    remove_saved_networks() {
      nmcli -t -f NAME connection show | while read -r line; do sudo nmcli connection delete "$line"; done
    }
    enable_iwd() {
      sudo mkdir -p "/etc/NetworkManager/conf.d/"
      sudo rm -f "/etc/NetworkManager/conf.d/iwd.conf"
      printf "[device]\nwifi.backend=iwd" | sudo tee /etc/NetworkManager/conf.d/iwd.conf > /dev/null
      remove_saved_networks
      echo -e "iwd enabled. Reboot required to apply changes."
    }
    disable_iwd() {
      sudo rm -f "/etc/NetworkManager/conf.d/iwd.conf"
      remove_saved_networks
      echo -e "iwd disabled. Reboot required to apply changes."
    }
    # Display current status
    current_status=$(get_current_status)
    echo -e "\nCurrent iwd status: $current_status\n"
    # Prompt user for action
    CHOICE=$(ugum choose "Enable iwd" "Disable iwd" "Exit without changes")
    case "$CHOICE" in
      "Enable iwd")
        enable_iwd
        ;;
      "Disable iwd")
        disable_iwd
        ;;
      "Exit without changes")
        echo "No changes made."
        ;;
      *)
        echo "Invalid choice. Exiting without changes."
        ;;
    esac
