# Enable docker
docker-enable:
    #!/usr/bin/bash
    sudo systemctl enable --now docker.service &&
    clear &&
    echo "Docker is now activated" ||
    echo "Error..."

# Disable docker
docker-disable:
    #!/usr/bin/bash
    sudo systemctl disable --now docker.service &&
    clear &&
    echo "Docker is now disabled" ||
    echo "Error..."

# Add dx-group to user
dx-group:
    #!/usr/bin/env bash

    # Sauvegarder le nom d'utilisateur avant d'exécuter les commandes privilégiées
    CURRENT_USER=$(logname)

    # Script temporaire pour les opérations privilégiées
    TEMP_SCRIPT=$(mktemp)
    cat > "$TEMP_SCRIPT" << 'EOF'
    #!/bin/bash
    setup_privileged_groups() {
        local current_user="$1"
        shift
        local groups_to_add=("$@")

        for group_name in "${groups_to_add[@]}"; do
            if ! grep -q "^$group_name:" /etc/group; then
                if grep -q "^$group_name:" /usr/lib/group; then
                    echo "Appending $group_name to /etc/group"
                    grep "^$group_name:" /usr/lib/group >> /etc/group
                else
                    echo "Warning: Group $group_name does not exist in /usr/lib/group"
                    continue
                fi
            fi

            # Ajouter l'utilisateur au groupe
            usermod -aG "$group_name" "$current_user"
            echo "Added $current_user to group $group_name"
        done
    }

    setup_privileged_groups "$@"
    EOF

    # Rendre le script exécutable
    chmod +x "$TEMP_SCRIPT"

    # Exécuter toutes les opérations privilégiées via pkexec
    pkexec "$TEMP_SCRIPT" "$CURRENT_USER" "docker" "incus-admin" "libvirt" "dialout"

    # Nettoyer
    rm "$TEMP_SCRIPT"

    echo "Reboot system and log back in to use docker, libvirt, incus, and serial connections."

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

# Configure grub bootmenu visibility. pass action 'help' for more info.
configure-grub ACTION="":
    #!/usr/bin/bash
    source /usr/libexec/bazzite-boot-remount
    source /usr/lib/ujust/ujust.sh
    # Function to display usage/help with some color
    print_help() {
      echo -e "Usage: ujust configure-grub <option>"
      echo
      echo -e "Where <option> can be:"
      echo -e "  ${bold}${cyan}hide${normal} = GRUB is hidden after a successful boot, even for dual-boot setups."
      echo -e "  ${bold}${yellow}unhide${normal} = GRUB is hidden after a successful boot, but it will show if dual-booting."
      echo -e "  ${bold}${green}show${normal} = GRUB always shows on boot."
      echo
      echo "If <option> is omitted, you will be prompted to choose interactively."
    }
    # Function to get the current GRUB menu_auto_hide setting and explain it
    get_current_setting() {
      local CURRENT_SETTING
      if sudo grub2-editenv - list | grep -q '^menu_auto_hide='; then
        CURRENT_SETTING=$(sudo grub2-editenv - list | awk -F= '/menu_auto_hide/ {print $2}')
      else
        CURRENT_SETTING=""
      fi
      if [ -z "$CURRENT_SETTING" ]; then
        echo "Current GRUB menu_auto_hide setting: ${bold}${green}Not Set (Always Show)${normal}"
        echo "Explanation:"
        echo "  - When unset, GRUB always shows on boot."
        return 0
      else
        case "$CURRENT_SETTING" in
          0)
            echo "Current GRUB menu_auto_hide setting: ${bold}${green}0 (Always Show)${normal}"
            echo "Explanation:"
            echo "  - ${bold}0${normal}: GRUB always shows on boot."
            ;;
          1)
            echo "Current GRUB menu_auto_hide setting: ${bold}${yellow}1 (Hide After Successful Boot, except when Dual Booting)${normal}"
            echo "Explanation:"
            echo "  - ${bold}1${normal}: GRUB is hidden after a successful boot, but it will show if dual-booting."
            ;;
          2)
            echo "Current GRUB menu_auto_hide setting: ${bold}${cyan}2 (Hide After Successful Boot)${normal}"
            echo "Explanation:"
            echo "  - ${bold}2${normal}: GRUB is hidden after a successful boot, even for dual-boot setups."
            ;;
          *)
            echo "Current GRUB menu_auto_hide setting: ${bold}${red}Unknown${normal}"
            echo "Explanation:"
            echo "  - This setting is unrecognized. Reset it to 0, 1, or 2."
            ;;
        esac
      fi
    }
    # Interactive menu for choosing the new behavior
    interactive_menu() {
      local options=(
        "Hide After Successful Boot (menu_auto_hide=2)"
        "Same except when Dual Booting (menu_auto_hide=1)"
        "Always Show Grub (unset menu_auto_hide)"
        "Exit without changes"
      )
      local choice
      choice=$(ugum choose "${options[@]}")
      echo "$choice"
    }
    # Function to apply the selected setting
    apply_setting() {
      local selected_option="$1"
      remount_boot_rw
      # Support the interactive strings as well as short commands
      case "$(echo "$selected_option" | tr '[:upper:]' '[:lower:]')" in
        *"(menu_auto_hide=2)"*|hide)
          sudo grub2-editenv - set menu_auto_hide=2
          echo "GRUB menu is now set to ${bold}${cyan}Hide After Successful Boot${normal}."
          ;;
        *"(menu_auto_hide=1)"*|unhide)
          sudo grub2-editenv - set menu_auto_hide=1
          echo "GRUB menu is now set to ${bold}${yellow}Hide After Successful Boot except when Dual Booting${normal}."
          ;;
        *"(unset menu_auto_hide)"*|show)
          sudo grub2-editenv - unset menu_auto_hide
          echo "GRUB menu is now set to ${bold}${green}Always Show${normal}."
          ;;
        *"exit without changes"*|exit)
          echo "${bold}No changes were made. Exiting...${normal}"
          ;;
        help)
          print_help
          ;;
        *)
          echo "${bold}${red}Invalid option selected. No changes were made.${normal}"
          ;;
      esac
      remount_boot_ro
    }
    OPTION="{{ ACTION }}"   # from “configure-grub ACTION=...”
    if [ "$OPTION" == "help" ]; then
      print_help
      exit 0
    fi
    get_current_setting
    echo
    # If no ACTION was passed, go interactive
    if [ -z "$OPTION" ]; then
      NEW_SETTING=$(interactive_menu)
      if [ -n "$NEW_SETTING" ]; then
        apply_setting "$NEW_SETTING"
      else
        echo "${bold}No changes were made.${normal}"
      fi
    else
      apply_setting "$OPTION"
    fi

# Add amd kargs to have advanced settings in CoreCtrl
amd-corectrl-set-kargs:
    #!/usr/bin/bash
    echo 'Setting needed kargs for CoreCtrl...'
    rpm-ostree kargs --append-if-missing=$(printf 'amdgpu.ppfeaturemask=0x%x\n' "$(($(cat /sys/module/amdgpu/parameters/ppfeaturemask) | 0x4000))")
    echo "ALL DONE!!!"

# Désactiver les mitigations CPU pour gagner en performance
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

# Activer l'émulation CPUID fault (AMD AM4/Steam Deck uniquement)
cpuid-emu-on:
    #!/usr/bin/bash
    if lsmod | grep -q kvm_amd; then
        echo "KVM détecté, déchargement de kvm_amd et kvm..."
        sudo modprobe -r kvm_amd kvm
    fi
    sudo modprobe cpuid_fault_emulation
    echo "Module cpuid_fault_emulation chargé"

# Désactiver l'émulation CPUID fault et restaurer KVM
cpuid-emu-off:
    #!/usr/bin/bash
    sudo modprobe -r cpuid_fault_emulation
    sudo modprobe kvm_amd
    echo "KVM restauré"

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

# Set recommended kernel parameters
kernel-setup:
    #!/usr/bin/bash
    rpm-ostree kargs --delete-if-present=preempt=full --append-if-missing=bluetooth.disable_ertm=1 &&
    echo "Done!"

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
      "Disable Power Saving (i915.enable_dc=0)"
        echo "Disabling power-saving mode (i915.enable_dc=0)..."
        update_karg 0
        ;;
      "Set to Level 1 (i915.enable_dc=1)"
        echo "Setting power-saving mode to level 1 (i915.enable_dc=1)..."
        update_karg 1
        ;;
      "Set to Level 2 (i915.enable_dc=2)"
        echo "Setting power-saving mode to level 2 (i915.enable_dc=2)..."
        update_karg 2
        ;;
      "Set to Level 3 (i915.enable_dc=3)"
        echo "Setting power-saving mode to level 3 (i915.enable_dc=3)..."
        update_karg 3
        ;;
      "Set to Level 4 (i915.enable_dc=4)"
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

# Enable proprietary Broadcom wl driver for legacy WiFi chips (BCM4360 on old MacBooks)
# Blacklists open-source drivers (b43, bcma, brcmfmac) so wl can take over
configure-broadcom-wl:
    #!/usr/bin/bash
    printf '%s\n' \
        'install wl /sbin/modprobe --ignore-install wl' \
        'blacklist b43' \
        'blacklist bcma' \
        'blacklist brcmfmac' \
        | sudo tee /etc/modprobe.d/wl-enable.conf > /dev/null
    echo "Driver Broadcom wl activé. Redémarrage requis."
    echo "Pour revenir en arrière : ujust revert-broadcom-wl"

# Disable proprietary Broadcom wl driver, restore open-source defaults
revert-broadcom-wl:
    #!/usr/bin/bash
    sudo rm -f /etc/modprobe.d/wl-enable.conf
    echo "Driver Broadcom wl désactivé (drivers open-source par défaut). Redémarrage requis."

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

# Remove pyenv
pyenv-remove:
    #!/usr/bin/bash
    rm -rf ~/.pyenv &&
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
        echo "/var/lib/homebrew/' directory is already removed"
    fi
    if [[ -d "/var/cache/homebrew/" ]]; then
        echo "Removing '/var/cache/homebrew/' directory"
        sudo rm -rf "/var/cache/homebrew/"
    else
        echo "/var/cache/homebrew/' directory is already removed"
    fi
    ## This is the main directory where brew is located
    if [[ -d "/var/home/linuxbrew/" ]]; then
        echo "Removing '/home/linuxbrew/' directory"
        sudo rm -rf "/var/home/linuxbrew/"
    else
        echo "/home/linuxbrew/' directory is already removed"
    fi
    if [[ -f "/etc/.linuxbrew" ]]; then
        echo "Removing empty '/etc/.linuxbrew' file"
        sudo rm -f "/etc/.linuxbrew"
    else
        echo "/etc/.linuxbrew' file is already removed"
    fi
    echo "ALL DONE!!!"
    echo "Restart computer to reuse brew"

# Install/Reinstall custom Archlinux
distrobox-archgab:
    #!/usr/bin/bash
    /usr/libexec/archgab-update

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

# Enable TPM CRB polling mode (fix slow boot on AMD fTPM)
tpm-polling-enable:
    #!/usr/bin/bash
    rpm-ostree kargs --append-if-missing=tpm_crb.interrupts=0
    echo "TPM CRB polling mode enabled. Reboot required."

# Disable TPM CRB polling mode
tpm-polling-disable:
    #!/usr/bin/bash
    rpm-ostree kargs --delete-if-present=tpm_crb.interrupts=0
    echo "TPM CRB polling mode disabled. Reboot required."

# Enable special scheduler for gaming (experimental)
scx-enable:
    sudo systemctl enable scx.service --now

# Disable special scheduler for gaming
scx-disable:
    sudo systemctl disable scx.service --now

# Enable foreground booster (CPU weight boost for focused window + VRAM priority)
foreground-booster-enable:
    #!/usr/bin/bash
    kwriteconfig6 --file kcgroupsrc --group "Foreground Booster" --key autostart true
    systemctl --user restart plasma-foreground-booster.service 2>/dev/null &&
    echo "Foreground booster enabled." ||
    echo "Config saved. Relog for changes to take effect."

# Disable foreground booster (recommended for multitasking: compilation + video, etc.)
foreground-booster-disable:
    #!/usr/bin/bash
    kwriteconfig6 --file kcgroupsrc --group "Foreground Booster" --key autostart false
    systemctl --user stop plasma-foreground-booster.service 2>/dev/null
    echo "Foreground booster disabled."

# Update gablue and flatpaks
gablue-update:
    #!/usr/bin/bash
    /usr/bin/gablue-update

# Rebase Gablue to another variant
gablue-rebase:
    #!/usr/bin/bash
    set -euo pipefail
    variant=$(ugum choose main main-dx nvidia nvidia-open nvidia-open-dx)
    [ -z "$variant" ] && exit 0
    rpm-ostree rebase "ostree-image-signed:docker://ghcr.io/elgabo86/gablue-${variant}:latest"

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

# Enable vulkan-low-latency-layer globally (Anti-Lag 2 / Reflex 2 on any GPU)
low-latency-enable:
    #!/usr/bin/bash
    sudo tee /etc/profile.d/vulkan-low-latency.sh > /dev/null << 'EOF'
    export LOW_LATENCY_LAYER=1
    EOF
    echo "Vulkan low-latency layer enabled globally. Reboot or relog for changes to take effect."

# Disable vulkan-low-latency-layer globally
low-latency-disable:
    #!/usr/bin/bash
    sudo rm -f /etc/profile.d/vulkan-low-latency.sh
    echo "Vulkan low-latency layer disabled globally. Reboot or relog for changes to take effect."

# Enable btrfs snapshots on /var/home with snapper
snapshots-enable:
    #!/usr/bin/bash
    if sudo snapper get-config &>/dev/null; then
        echo "Snapper est déjà configuré."
        echo "Utilisez btrfs-assistant pour gérer les paramètres."
        exit 0
    fi
    echo "Configuration de snapper pour /var/home..."
    sudo snapper create-config /var/home
    sudo snapper set-config "TIMELINE_MIN_AGE=1800"
    sudo snapper set-config "TIMELINE_LIMIT_HOURLY=0"
    sudo snapper set-config "TIMELINE_LIMIT_DAILY=7"
    sudo snapper set-config "TIMELINE_LIMIT_WEEKLY=4"
    sudo snapper set-config "TIMELINE_LIMIT_MONTHLY=2"
    sudo snapper set-config "TIMELINE_LIMIT_QUARTERLY=0"
    sudo snapper set-config "TIMELINE_LIMIT_YEARLY=0"
    sudo snapper set-config "SPACE_LIMIT=0.5"
    sudo snapper set-config "FREE_LIMIT=0.2"
    sudo systemctl enable --now snapper-timeline.timer
    sudo systemctl enable --now snapper-cleanup.timer
    echo ""
    echo "Snapshots BTRFS activés !"
    echo "  - Rétention : 7 quotidiens, 4 hebdomadaires, 2 mensuels"
    echo "  - Limite espace : 50% max, 20% libre minimum"
    echo "  - Snapshots automatiques activés (timeline)"
    echo "  - Nettoyage automatique activé (cleanup)"
    echo ""
    echo "Utilisez btrfs-assistant pour gérer visuellement vos snapshots."

# Disable btrfs snapshots (snapper)
snapshots-disable:
    #!/usr/bin/bash
    if ! sudo snapper get-config &>/dev/null; then
        echo "Snapper n'est pas configuré."
        exit 0
    fi
    echo "Désactivation de snapper..."
    sudo systemctl disable --now snapper-timeline.timer
    sudo systemctl disable --now snapper-cleanup.timer
    sudo snapper delete-config
    echo ""
    echo "Snapshots BTRFS désactivés et configuration supprimée."

# Enroll Gablue Secure Boot keys - Enter password "secureblue" if prompted
secureboot:
    #!/usr/bin/bash
    ENROLLMENT_PASSWORD="secureblue"
    SECUREBOOT_KEY=/etc/pki/akmods/certs/gablue-secure-boot.der
    CPUID_FAULT_KEY=/etc/pki/akmods/certs/gablue-kmod.der
    sudo mokutil --timeout -1
    echo -e "$ENROLLMENT_PASSWORD\n$ENROLLMENT_PASSWORD" | sudo mokutil --import "$SECUREBOOT_KEY"
    echo -e "$ENROLLMENT_PASSWORD\n$ENROLLMENT_PASSWORD" | sudo mokutil --import "$CPUID_FAULT_KEY"
    echo 'At next reboot, the moktool UEFI menu UI will be displayed (*QWERTY* keyboard input and navigation).\nThen, select "Enroll MOK", and input "secureblue" as the password'

# Configure beesd for BTRFS deduplication (pas active par defaut)
configure-beesd ACTION="":
    #!/usr/bin/bash
    source /usr/lib/ujust/ujust.sh
    # Constants for bees hash table sizing
    readonly MIN_FS_TB=1
    readonly MAX_FS_TB=4
    readonly HASH_SIZE_MB_PER_TB=512
    remove_beesd_config() {
        local uuid="$1"
        echo "Removing configuration..."
        sudo systemctl disable --now "beesd@${uuid}.timer"
        sudo systemctl stop "beesd@${uuid}.service" 2>/dev/null || true
        sudo rm "/etc/bees/${uuid}.conf"
        sudo rm -f "/etc/systemd/system/beesd@${uuid}.service.d/override.conf"
        sudo rmdir "/etc/systemd/system/beesd@${uuid}.service.d" 2>/dev/null || true
        echo "Configuration removed and timer disabled."
        echo "Cleaning leftover data"
        fs_root=$(findmnt --source UUID="$uuid" -o TARGET,OPTIONS | grep "subvolid=5" | awk '{print $1}')
        is_manual_mount=false
        if [[ -z "$fs_root" ]]; then
            fs_root="/run/bees/mnt/${uuid}"
            sudo mkdir -p "$fs_root"
            if ! sudo mount -o subvolid=5 UUID="$uuid" "$fs_root"; then
                fs_root=""
            else
                is_manual_mount=true
            fi
        fi
        if [[ -n "$fs_root" ]]; then
            beeshome_id=$(sudo btrfs subvolume list "$fs_root" -t | grep "\.beeshome" | awk '{print $1}')
            if [[ -n "$beeshome_id" ]]; then
                echo "Found .beeshome subvolume with ID: $beeshome_id"
                sudo btrfs subvolume delete -i "$beeshome_id" "$fs_root"
                echo "Deleted .beeshome subvolume"
            else
                echo "No .beeshome subvolume found"
            fi
            if [[ "$is_manual_mount" == true ]]; then
                sudo umount "$fs_root"
                sleep 1
                # Device might still be busy
                # Not critical if this fails
                sudo rmdir "$fs_root" >/dev/null 2>&1
            fi
        fi
        sudo systemctl daemon-reload
    }
    while true; do
        # Get btrfs filesystem info
        mapfile -t raw_btrfs_output < <(sudo lsblk --fs --list --noheadings --output "NAME,UUID,SIZE" --filter 'FSTYPE == "btrfs"' --bytes)
        if [[ ${#raw_btrfs_output[@]} -eq 0 ]]; then
            echo "No BTRFS filesystems found."
            exit 0
        fi
        # Check for existing configurations and timer status
        unset configured_uuids
        declare -A configured_uuids
        if [[ -d "/etc/bees" ]]; then
            for config in /etc/bees/*.conf; do
                if [[ -f "$config" ]] && [[ "$config" != *"/beesd.conf.sample" ]]; then
                    filename=$(basename "$config" .conf)
                    if systemctl is-enabled "beesd@${filename}.timer" >/dev/null 2>&1; then
                        configured_uuids["$filename"]="enabled"
                    else
                        configured_uuids["$filename"]="disabled"
                    fi
                fi
            done
        fi
        # Create menu options and size map
        menu_options=()
        unset filesystem_size_map
        declare -A filesystem_size_map
        for line in "${raw_btrfs_output[@]}"; do
            read -r name uuid size_bytes <<< "$line"
            # Convert bytes to human readable for display
            size_human=$(numfmt --to=iec-i --suffix=B "$size_bytes")
            filesystem_size_map["$uuid"]="$size_bytes"
            if [[ "${configured_uuids[$uuid]:-}" == "disabled" ]]; then
                menu_options+=("$name $uuid $size_human (Configuré - Timer désactivé)")
            elif [[ "${configured_uuids[$uuid]:-}" == "enabled" ]]; then
                menu_options+=("$name $uuid $size_human (Configuré)")
            else
                menu_options+=("$name $uuid $size_human")
            fi
        done
        menu_options+=("Exit")
        echo "Select a BTRFS filesystem:"
        CHOICE=$(ugum choose "${menu_options[@]}")
        if [[ "$CHOICE" == "Exit" ]] || [[ -z "$CHOICE" ]]; then
            exit 0
        fi
        UUID=$(echo "$CHOICE" | awk '{print $2}')
        SIZE_BYTES="${filesystem_size_map[$UUID]}"
        if [[ -f "/etc/bees/${UUID}.conf" ]]; then
            echo "Configuration déjà présente pour UUID : ${UUID}"
            if [[ "${configured_uuids[$UUID]}" == "disabled" ]]; then
                ACTION=$(ugum choose "Activer le timer" "Supprimer la configuration" "Reinstaller/Écraser" "Retour")
            else
                ACTION=$(ugum choose "Supprimer la configuration" "Reinstaller/Écraser" "Retour")
            fi
        else
            ACTION=$(ugum choose "Créer la configuration" "Retour")
        fi
        case "$ACTION" in
            "Activer le timer")
                echo "Activation du timer beesd@${UUID}.timer..."
                sudo systemctl enable --now "beesd@${UUID}.timer"
                echo "Timer activé."
                continue
                ;;
            "Supprimer la configuration")
                remove_beesd_config "$UUID"
                continue
                ;;
            "Retour")
                continue
                ;;
            "")
                exit 0
                ;;
        esac
        # Round up to nearest TB by adding 0.5TB-->B and flooring
        size_tb=$(((SIZE_BYTES + 500000000000) / 1000**4))
        # Clamp filesystem size between min/max for hash table calculation
        # Avoid eating too much memory when bees starts
        size_tb=$(($size_tb < $MIN_FS_TB ? $MIN_FS_TB : $size_tb))
        size_tb=$(($size_tb > $MAX_FS_TB ? $MAX_FS_TB : $size_tb))
        # Bees Hash Table Sizing
        # Size MUST be multiple of 128KB
        # DB_SIZE=$((1024*1024*1024)) # 1G in bytes
        db_size=$((size_tb * $HASH_SIZE_MB_PER_TB * 1024**2))
        # Convert db_size from bytes to MB for memory threshold
        mem_thresh_mb=$((db_size / 1024**2))
        sudo mkdir -p /etc/bees
        sudo cp /usr/etc/bees/beesd.conf.sample "/etc/bees/${UUID}.conf"
        # https://github.com/Zygo/bees/blob/master/docs/options.md#load-management-options
        sudo sed -i "s/^UUID=.*/UUID=${UUID}/" "/etc/bees/${UUID}.conf"
        sudo sed -i "s/# DB_SIZE=.*/DB_SIZE=${db_size}/" "/etc/bees/${UUID}.conf"
        sudo sed -i "s/# OPTIONS=.*/OPTIONS=\"--strip-paths --no-timestamps --thread-factor 0.125 --thread-min 1 --loadavg-target 2\"/" "/etc/bees/${UUID}.conf"
        echo "Created /etc/bees/${UUID}.conf with DB_SIZE=${db_size} bytes"
        # Only start the service if enough memory is free
        # Per service as hash table size may differ in each configuration
        sudo mkdir -p "/etc/systemd/system/beesd@${UUID}.service.d"
        {
            echo "[Service]"
            echo "ExecCondition=/bin/sh -c \"test \$(free -m | awk '/^Mem:/ {print \$7}') -gt ${mem_thresh_mb}\""
        } | sudo tee "/etc/systemd/system/beesd@${UUID}.service.d/override.conf" > /dev/null
        sudo systemctl daemon-reload
        sudo systemctl enable --now "beesd@${UUID}.timer"
    done

# Setup kvmfr module for Looking Glass (GPU passthrough, 2 GPUs requis)
setup-kvmfr:
    #!/usr/bin/bash
    source /usr/lib/ujust/ujust.sh
    echo "Ce module (Looking Glass/kvmfr) est expérimental !"
    echo "Nécessite 2 GPUs (ou iGPU + dGPU)."
    echo ""
    # Modprobe config (128 Mo par défaut, suffisant jusqu'à 4K SDR)
    if [ ! -f /etc/modprobe.d/kvmfr.conf ]; then
        echo 'options kvmfr static_size=128' | sudo tee /etc/modprobe.d/kvmfr.conf > /dev/null
        echo "Configuration modprobe créée (/etc/modprobe.d/kvmfr.conf)"
    else
        echo "Configuration modprobe existante, inchangée."
    fi
    # Udev rule
    if [ ! -f /etc/udev/rules.d/99-kvmfr.rules ]; then
        echo 'KERNEL=="kvmfr0", OWNER="root", GROUP="kvm", MODE="0660"' | sudo tee /etc/udev/rules.d/99-kvmfr.rules > /dev/null
        sudo udevadm control --reload-rules
        echo "Règle udev créée."
    fi
    # SELinux context
    sudo semanage fcontext -a -t svirt_image_t /dev/kvmfr0 2>/dev/null || true
    sudo restorecon /dev/kvmfr0 2>/dev/null || true
    # Charger le module (sans reboot)
    if ! lsmod | grep -q kvmfr; then
        sudo modprobe kvmfr 2>/dev/null && echo "Module kvmfr chargé." || echo "Erreur chargement module (image non-DX ?)"
    else
        echo "Module kvmfr déjà chargé."
    fi
    echo ""
    echo "Configuration terminée. Consulte la doc Looking Glass pour la suite."

# Activer/désactiver la compression BTRFS zstd sur un filesystem
btrfs-compress ACTION="":
    #!/usr/bin/bash
    source /usr/lib/ujust/ujust.sh

    # Récolter les filesystems BTRFS avec leur label/UUID pour un affichage lisible
    declare -A fs_label
    declare -A fs_mount
    declare -A fs_seen
    while read -r target uuid label; do
        [[ -z "$target" ]] && continue
        [[ "$target" == /run/bees/* ]] && continue
        [[ "$target" == /sysroot* ]] && continue
        [[ "$target" == /etc ]] && continue
        if [[ -z "${fs_seen[$uuid]:-}" ]] || [[ ${#target} -gt ${#fs_mount[$uuid]} ]]; then
            fs_seen[$uuid]=1
            fs_mount[$uuid]="$target"
            [[ -n "$label" ]] && fs_label[$uuid]="$label"
        fi
    done < <(sudo findmnt -t btrfs -o TARGET,UUID,LABEL -n -l)

    if [[ ${#fs_mount[@]} -eq 0 ]]; then
        echo "Aucun filesystem BTRFS trouvé."
        exit 1
    fi

    menu_options=()
    declare -A menu_uuid
    for uuid in "${!fs_mount[@]}"; do
        label="${fs_label[$uuid]:-}"
        mountpoint="${fs_mount[$uuid]}"
        if [[ -n "$label" ]]; then
            menu_options+=("$label ($mountpoint)")
        else
            menu_options+=("$mountpoint")
        fi
        menu_uuid["$uuid"]="$mountpoint"
    done

    if [[ ${#menu_options[@]} -eq 0 ]]; then
        echo "Aucun filesystem BTRFS monté trouvé."
        exit 1
    fi

    menu_options+=("Annuler")
    echo "Choisir le filesystem BTRFS :"
    CHOICE=$(ugum choose "${menu_options[@]}")
    [[ "$CHOICE" == "Annuler" ]] || [[ -z "$CHOICE" ]] && exit 0

    # Retrouver l'UUID à partir du choix
    SELECTED_UUID=""
    for uuid in "${!menu_uuid[@]}"; do
        mp="${menu_uuid[$uuid]}"
        if [[ "$CHOICE" == *"($mp)"* ]] || [[ "$CHOICE" == "$mp" ]]; then
            SELECTED_UUID="$uuid"
            break
        fi
    done
    [[ -z "$SELECTED_UUID" ]] && exit 1

    # Lister tous les mountpoints BTRFS de cet UUID (sauf exclusions)
    mapfile -t all_mounts < <(sudo findmnt -t btrfs -o TARGET,UUID -n -l | \
        while read -r tgt uid; do
            [[ "$uid" == "$SELECTED_UUID" ]] || continue
            [[ "$tgt" == /run/bees/* || "$tgt" == /sysroot* || "$tgt" == /etc ]] && continue
            echo "$tgt"
        done | grep -v -E '(beeshome|/root[0-9]|\.snapshots)' || true)

    # Si un seul mount (ex: T5), utiliser btrfs subvolume list pour trouver les sous-volumes
    if [[ ${#all_mounts[@]} -le 1 ]]; then
        BASE_MOUNT="${all_mounts[0]:-}"
        [[ -z "$BASE_MOUNT" ]] && exit 1

        mapfile -t subvol_paths < <(sudo btrfs subvolume list "$BASE_MOUNT" 2>/dev/null | \
            awk '{print $NF}' | grep -v -E '^\.beeshome$|^root|\.snapshots$' || true)

        if [[ ${#subvol_paths[@]} -gt 0 ]]; then
            declare -a all_paths=()
            for sv in "${subvol_paths[@]}"; do
                all_paths+=("${BASE_MOUNT%/}/${sv#/}")
            done
            all_mounts=( "${all_paths[@]}" )
        fi
    fi

    if [[ ${#all_mounts[@]} -le 1 ]]; then
        TARGETS=( "${all_mounts[@]}" )
    else
        sub_options=("${all_mounts[@]}" "Tous" "Annuler")
        echo "Points de montage détectés, sur lesquels appliquer la compression ?"
        SUB_CHOICE=$(ugum choose "${sub_options[@]}")
        [[ "$SUB_CHOICE" == "Annuler" ]] || [[ -z "$SUB_CHOICE" ]] && exit 0

        if [[ "$SUB_CHOICE" == "Tous" ]]; then
            TARGETS=( "${all_mounts[@]}" )
        else
            TARGETS=( "$SUB_CHOICE" )
        fi
    fi

    # Détecter l'état actuel sur le premier mountpoint
    CURRENT_COMPRESSION=$(sudo btrfs property get "${TARGETS[0]}" compression 2>/dev/null | cut -d= -f2)
    [[ -z "$CURRENT_COMPRESSION" ]] && CURRENT_COMPRESSION="none"

    if [[ "$CURRENT_COMPRESSION" == "zstd" ]]; then
        ACTION=$(ugum choose "Désactiver la compression" "Annuler")
    else
        ACTION=$(ugum choose "Activer la compression zstd" "Annuler")
    fi

    case "$ACTION" in
        "Activer la compression zstd")
            for target in "${TARGETS[@]}"; do
                echo "→ Activation zstd sur $target..."
                sudo btrfs property set "$target" compression zstd
            done
            ;;
        "Désactiver la compression")
            for target in "${TARGETS[@]}"; do
                echo "→ Désactivation compression sur $target..."
                sudo btrfs property set "$target" compression ""
            done
            ;;
        *)
            exit 0
            ;;
    esac

# Sélectionner un disque sur lequel booter (efibootmgr --bootnext)
boot-select:
    #!/usr/bin/bash
    source /usr/lib/ujust/ujust.sh

    if ! command -v efibootmgr &>/dev/null; then
        echo "efibootmgr non installé."
        exit 1
    fi

    if [[ ! -d /sys/firmware/efi ]]; then
        echo "Système non démarré en mode UEFI."
        exit 1
    fi

    # Parser les entrées EFI : BootXXXX* Label\TDescription
    mapfile -t boot_lines < <(efibootmgr 2>/dev/null | grep '^Boot[0-9]')
    if [[ ${#boot_lines[@]} -eq 0 ]]; then
        echo "Aucune entrée de boot EFI trouvée."
        exit 1
    fi

    # Construction du menu : extraire le numéro, le label et le nom du disque
    menu_options=()
    declare -A boot_nums
    while IFS=$'\t' read -r first_part rest; do
        boot_num=$(echo "$first_part" | grep -oP '^Boot\K[0-9A-F]{4}')
        boot_label=$(echo "$first_part" | sed 's/^Boot[0-9A-F]\{4\}\* //')
        [[ -z "$boot_num" || -z "$boot_label" ]] && continue
        # Résoudre PARTUUID → nom du disque (ex: nvme0n1, sdd)
        part_uuid=$(echo "$rest" | grep -oP 'HD\([^,]+,[^,]+,\K[^,]+' || true)
        disk_name=""
        if [[ -n "$part_uuid" ]]; then
            part_dev=$(findfs PARTUUID="$part_uuid" 2>/dev/null || true)
            if [[ -n "$part_dev" ]]; then
                disk_name=$(lsblk -no PKNAME "$part_dev" 2>/dev/null || true)
            fi
        fi
        if [[ -n "$disk_name" ]]; then
            menu_options+=("$boot_label ($disk_name)")
        else
            menu_options+=("$boot_label")
        fi
        boot_nums["${menu_options[-1]}"]="$boot_num"
    done < <(printf '%s\n' "${boot_lines[@]}")

    if [[ ${#menu_options[@]} -eq 0 ]]; then
        echo "Impossible de parser les entrées EFI."
        exit 1
    fi

    menu_options+=("Annuler")
    echo "Choisir le disque sur lequel booter (bootnext, une seule fois) :"
    CHOICE=$(ugum choose "${menu_options[@]}")
    [[ "$CHOICE" == "Annuler" ]] || [[ -z "$CHOICE" ]] && exit 0

    BOOT_NUM="${boot_nums[$CHOICE]}"
    echo "Configuration du bootnext sur $CHOICE..."

    if sudo efibootmgr --bootnext "$BOOT_NUM"; then
        ugum confirm "Redémarrer maintenant ?" && systemctl reboot
    else
        echo "Erreur lors de la configuration du bootnext."
        exit 1
    fi

# Changer l'ordre de boot EFI de façon permanente
boot-order:
    #!/usr/bin/bash
    source /usr/lib/ujust/ujust.sh

    if ! command -v efibootmgr &>/dev/null; then
        echo "efibootmgr non installé."
        exit 1
    fi

    if [[ ! -d /sys/firmware/efi ]]; then
        echo "Système non démarré en mode UEFI."
        exit 1
    fi

    # Parser les entrées EFI (même logique que boot-select)
    mapfile -t boot_lines < <(efibootmgr 2>/dev/null | grep '^Boot[0-9]')
    if [[ ${#boot_lines[@]} -eq 0 ]]; then
        echo "Aucune entrée de boot EFI trouvée."
        exit 1
    fi

    declare -A boot_labels
    declare -A boot_nums
    all_options=()
    while IFS=$'\t' read -r first_part rest; do
        boot_num=$(echo "$first_part" | grep -oP '^Boot\K[0-9A-F]{4}')
        boot_label=$(echo "$first_part" | sed 's/^Boot[0-9A-F]\{4\}\* //')
        [[ -z "$boot_num" || -z "$boot_label" ]] && continue
        part_uuid=$(echo "$rest" | grep -oP 'HD\([^,]+,[^,]+,\K[^,]+' || true)
        disk_name=""
        if [[ -n "$part_uuid" ]]; then
            part_dev=$(findfs PARTUUID="$part_uuid" 2>/dev/null || true)
            [[ -n "$part_dev" ]] && disk_name=$(lsblk -no PKNAME "$part_dev" 2>/dev/null || true)
        fi
        if [[ -n "$disk_name" ]]; then
            entry="$boot_label ($disk_name)"
        else
            entry="$boot_label"
        fi
        all_options+=("$entry")
        boot_labels["$entry"]="$boot_num"
        boot_nums["$entry"]="$boot_num"
    done < <(printf '%s\n' "${boot_lines[@]}")

    # Afficher l'ordre actuel
    current_order=$(efibootmgr 2>/dev/null | grep '^BootOrder:' | sed 's/^BootOrder: //')
    echo "Ordre actuel : $current_order"
    echo ""

    # Sélectionner le nouvel ordre un par un
    echo "Sélectionnez les entrées dans l'ordre de priorité souhaité."
    echo "Choisissez « Terminé » pour placer automatiquement les entrées restantes à la fin."
    echo ""
    declare -a new_order=()
    declare -a chosen_labels=()
    remaining=( "${all_options[@]}" )
    for i in $(seq 1 ${#all_options[@]}); do
        [[ $i -gt 1 ]] && remaining+=("Terminé")
        echo "Entrée n°$i :"
        CHOICE=$(ugum choose "${remaining[@]}")
        if [[ "$CHOICE" == "Terminé" ]] || [[ -z "$CHOICE" ]]; then
            break
        fi
        echo "  → $CHOICE"
        new_order+=("${boot_nums[$CHOICE]}")
        chosen_labels+=("$CHOICE")
        # Retirer l'entrée choisie des options restantes
        declare -a new_remaining=()
        for opt in "${remaining[@]}"; do
            [[ "$opt" == "Terminé" ]] && continue
            [[ "$opt" != "$CHOICE" ]] && new_remaining+=("$opt")
        done
        remaining=( "${new_remaining[@]}" )
    done

    if [[ ${#new_order[@]} -eq 0 ]]; then
        echo "Aucun changement effectué."
        exit 0
    fi

    # Ajouter les entrées non sélectionnées à la fin (préserve toutes les entrées)
    for opt in "${remaining[@]}"; do
        [[ "$opt" == "Terminé" ]] && continue
        new_order+=("${boot_nums[$opt]}")
        chosen_labels+=("$opt (non modifié)")
    done

    NEW_ORDER=$(printf '%s,' "${new_order[@]}")
    NEW_ORDER="${NEW_ORDER%,}"

    echo ""
    echo "Nouvel ordre de boot : $NEW_ORDER"
    printf '  %s\n' "${chosen_labels[@]}"
    echo ""
    if ugum confirm "Appliquer ce nouvel ordre de boot ?"; then
        if sudo efibootmgr --bootorder "$NEW_ORDER"; then
            echo "Ordre de boot modifié avec succès."
        else
            echo "Erreur lors du changement d'ordre de boot."
            exit 1
        fi
    else
        echo "Aucun changement effectué."
        exit 0
    fi
btrfs-compress-defrag:
    #!/usr/bin/bash
    source /usr/lib/ujust/ujust.sh

    # Récolter les filesystems BTRFS avec leur label pour un affichage lisible
    declare -A fs_label
    declare -A fs_mount
    declare -A fs_seen
    while read -r target uuid label; do
        [[ -z "$target" ]] && continue
        [[ "$target" == /run/bees/* ]] && continue
        [[ "$target" == /sysroot* ]] && continue
        [[ "$target" == /etc ]] && continue
        if [[ -z "${fs_seen[$uuid]:-}" ]] || [[ ${#target} -gt ${#fs_mount[$uuid]} ]]; then
            fs_seen[$uuid]=1
            fs_mount[$uuid]="$target"
            [[ -n "$label" ]] && fs_label[$uuid]="$label"
        fi
    done < <(sudo findmnt -t btrfs -o TARGET,UUID,LABEL -n -l)

    if [[ ${#fs_mount[@]} -eq 0 ]]; then
        echo "Aucun filesystem BTRFS trouvé."
        exit 1
    fi

    menu_options=()
    declare -A menu_uuid
    for uuid in "${!fs_mount[@]}"; do
        label="${fs_label[$uuid]:-}"
        if [[ -n "$label" ]]; then
            menu_options+=("$label (${fs_mount[$uuid]})")
        else
            menu_options+=("${fs_mount[$uuid]}")
        fi
        menu_uuid["$uuid"]="${fs_mount[$uuid]}"
    done

    if [[ ${#menu_options[@]} -eq 0 ]]; then
        echo "Aucun filesystem BTRFS monté trouvé."
        exit 1
    fi

    menu_options+=("Annuler")
    echo "Choisir le filesystem BTRFS à compresser :"
    CHOICE=$(ugum choose "${menu_options[@]}")
    [[ "$CHOICE" == "Annuler" ]] || [[ -z "$CHOICE" ]] && exit 0

    # Retrouver l'UUID à partir du choix
    SELECTED_UUID=""
    for uuid in "${!menu_uuid[@]}"; do
        mp="${menu_uuid[$uuid]}"
        if [[ "$CHOICE" == *"($mp)"* ]] || [[ "$CHOICE" == "$mp" ]]; then
            SELECTED_UUID="$uuid"
            break
        fi
    done
    [[ -z "$SELECTED_UUID" ]] && exit 1

    # Lister tous les mountpoints BTRFS de cet UUID (sauf exclusions)
    mapfile -t all_mounts < <(sudo findmnt -t btrfs -o TARGET,UUID -n -l | \
        while read -r tgt uid; do
            [[ "$uid" == "$SELECTED_UUID" ]] || continue
            [[ "$tgt" == /run/bees/* || "$tgt" == /sysroot* || "$tgt" == /etc ]] && continue
            echo "$tgt"
        done | grep -v -E '(beeshome|/root[0-9]|\.snapshots)' | grep -v '^/var$' || true)

    # Si un seul mount (ex: T5), utiliser btrfs subvolume list pour trouver les sous-volumes
    if [[ ${#all_mounts[@]} -le 1 ]]; then
        BASE_MOUNT="${all_mounts[0]:-}"
        [[ -z "$BASE_MOUNT" ]] && exit 1

        mapfile -t subvol_paths < <(sudo btrfs subvolume list "$BASE_MOUNT" 2>/dev/null | \
            awk '{print $NF}' | grep -v -E '^\.beeshome$|^root|\.snapshots$' | grep -v '^var[0-9]*$' || true)

        if [[ ${#subvol_paths[@]} -gt 0 ]]; then
            declare -a all_paths=()
            for sv in "${subvol_paths[@]}"; do
                all_paths+=("${BASE_MOUNT%/}/${sv#/}")
            done
            all_mounts=( "${all_paths[@]}" )
        fi
    fi

    if [[ ${#all_mounts[@]} -le 1 ]]; then
        TARGETS=( "${all_mounts[@]}" )
    else
        sub_options=("${all_mounts[@]}" "Tous" "Annuler")
        echo "Points de montage détectés, lesquels compresser ?"
        echo "Cela peut prendre plusieurs heures selon la taille des données."
        SUB_CHOICE=$(ugum choose "${sub_options[@]}")
        [[ "$SUB_CHOICE" == "Annuler" ]] || [[ -z "$SUB_CHOICE" ]] && exit 0

        if [[ "$SUB_CHOICE" == "Tous" ]]; then
            TARGETS=( "${all_mounts[@]}" )
        else
            TARGETS=( "$SUB_CHOICE" )
        fi
    fi

    for target in "${TARGETS[@]}"; do
        echo "→ Compression de $target..."
        sudo btrfs -v filesystem defrag -czstd -r "$target"
    done

    echo "Défragmentation et compression terminées."

# Réinitialiser le cache capabilities de libvirt (corrige "video model 'virtio' unsupported" dans virt-manager)
libvirt-reset-cache:
    #!/usr/bin/bash
    # Ce cache est généré par libvirt au premier démarrage du daemon. S'il est créé
    # avant l'installation des modules QEMU display (virtio-gpu, qxl...), virt-manager
    # refuse les modèles virtio/qxl avec l'erreur "domain configuration does not
    # support video model 'virtio'".
    # Cache system (qemu:///system)
    if [ -d /var/cache/libvirt/qemu/capabilities ]; then
        sudo rm -rf /var/cache/libvirt/qemu/capabilities
        echo "Cache system effacé."
    fi
    # Cache user (qemu:///session)
    USER_CACHE="${HOME}/.cache/libvirt/qemu/cache/capabilities"
    if [ -d "$USER_CACHE" ]; then
        rm -rf "$USER_CACHE"
        echo "Cache user effacé."
    fi
    # Redémarrer le daemon system pour régénérer le cache
    if systemctl is-active --quiet virtqemud 2>/dev/null; then
        sudo systemctl restart virtqemud
        echo "Daemon virtqemud redémarré."
    elif systemctl is-active --quiet libvirtd 2>/dev/null; then
        sudo systemctl restart libvirtd
        echo "Daemon libvirtd redémarré."
    fi
    # Forcer la régénération du cache user
    virsh -c qemu:///session domcapabilities --virttype kvm --arch x86_64 --machine q35 >/dev/null 2>&1 || true
    echo ""
    echo "Cache capabilities libvirt réinitialisé."
    echo "Relance virt-manager : virtio et qxl sont maintenant disponibles."

# Réparer les règles KWin VRR (si VRR reste actif après une mise à jour)
vrr-fix:
    #!/usr/bin/bash
    SYSTEM_RULES="/etc/xdg/kwinrulesrc"
    USER_RULES="${HOME}/.config/kwinrulesrc"
    if [ ! -f "$SYSTEM_RULES" ]; then
        echo "Fichier système introuvable : $SYSTEM_RULES"
        exit 1
    fi
    SYSTEM_GROUPS=$(grep -oP '^\[\K[^]]+' "$SYSTEM_RULES" | grep -v '^General$' | sort -u)
    CURRENT_RULES=$(kreadconfig6 --file kwinrulesrc --group General --key rules 2>/dev/null || echo "")
    ALL_GROUPS="$SYSTEM_GROUPS"
    if [ -n "$CURRENT_RULES" ]; then
        IFS=',' read -ra USER_GROUPS <<< "$CURRENT_RULES"
        for g in "${USER_GROUPS[@]}"; do
            g=$(echo "$g" | xargs)
            if [ -n "$g" ] && ! echo "$ALL_GROUPS" | grep -qxF "$g"; then
                ALL_GROUPS="$ALL_GROUPS"$'\n'"$g"
            fi
        done
    fi
    MERGED=$(echo "$ALL_GROUPS" | paste -sd,)
    if [ "$MERGED" != "$CURRENT_RULES" ]; then
        kwriteconfig6 --file kwinrulesrc --group General --key rules "$MERGED"
        echo "Règles VRR appliquées. Redémarre la session pour prise d'effet."
    else
        echo "Règles VRR déjà à jour."
    fi
