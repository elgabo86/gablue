#!/usr/bin/bash
# Hook post-rootfs Gablue : Anaconda + kickstart + tweaks session live
#
# Ce script est exécuté APRÈS la régénération de l'initramfs.
# Il installe Anaconda, configure le kickstart pour l'installation
# bootc, applique les tweaks de la session live (désactivation
# services, configuration NVIDIA, etc.).
#
# Adapté de Bazzite (ublue-os/bazzite) pour Gablue.

set -exo pipefail

source /etc/os-release

# =============================================================================
# NETTOYAGE VERSIONLOCKS
# =============================================================================

# Supprimer tous les versionlocks pour éviter les problèmes de dépendances
dnf -qy versionlock clear

# =============================================================================
# INSTALLATION D'ANACONDA LIVE
# =============================================================================

dnf install -qy --enable-repo=fedora-cisco-openh264 --allowerasing firefox anaconda-live libblockdev-{btrfs,lvm,dm}

mkdir -p /var/lib/rpm-state

# =============================================================================
# VARIABLES GABLUE
# =============================================================================

imageref="$(podman images --format '{{ index .Names 0 }}\n' 'gablue*' | head -1)"
imageref="${imageref##*://}"
imageref="${imageref%%:*}"
imagetag="$(podman images --format '{{ .Tag }}\n' "$imageref" | head -1)"
sbkey='https://github.com/elgabo86/gablue/raw/main/secure_boot.der'
SECUREBOOT_KEY="/usr/share/gablue/sb_pubkey.der"
ENROLLMENT_PASSWORD="gablue"

# =============================================================================
# BRANDING GABLUE
# =============================================================================

echo "Gablue $VERSION_ID ($VERSION_CODENAME)" > /etc/system-release

# =============================================================================
# RÉCUPÉRATION CLÉ SECURE BOOT
# =============================================================================

mkdir -p /usr/share/gablue
curl -Lo "$SECUREBOOT_KEY" "$sbkey"

# =============================================================================
# KICKSTART PAR DÉFAUT
# =============================================================================

cat << KSEOF >> /usr/share/anaconda/interactive-defaults.ks

# Créer le répertoire de logs
%pre
mkdir -p /tmp/anaconda_custom_logs
%end

# Supprimer le répertoire EFI fedora (doit correspondre à efi_dir du profil)
%pre-install --erroronfail
rm -rf /mnt/sysroot/boot/efi/EFI/fedora
%end

# Relabel la partition boot
%pre-install --erroronfail --log=/tmp/anaconda_custom_logs/repartitioning.log
set -x
xboot_dev=\$(findmnt -o SOURCE --nofsroot --noheadings -f --target /mnt/sysroot/boot)
if [[ -z \$xboot_dev ]]; then
  echo "ERROR: xboot_dev not found"
  exit 1
fi
e2label "\$xboot_dev" "gablue_xboot"
%end

# Afficher les logs d'installation en cas d'erreur
%onerror
run0 --user=liveuser yad --timeout=0 --text-info --no-buttons \
    --width=600 --height=400 \
    --text="Une erreur est survenue pendant l'installation. Veuillez signaler ce problème." \
    < /tmp/anaconda.log
%end

ostreecontainer --url=$imageref:$imagetag --transport=containers-storage --no-signature-verification
%include /usr/share/anaconda/post-scripts/install-configure-upgrade.ks
%include /usr/share/anaconda/post-scripts/disable-fedora-flatpak.ks
%include /usr/share/anaconda/post-scripts/install-flatpaks.ks
%include /usr/share/anaconda/post-scripts/secureboot-enroll-key.ks
KSEOF

# =============================================================================
# POST-SCRIPT : BOOTC SWITCH (IMAGES SIGNÉES)
# =============================================================================

cat << PSEOF >> /usr/share/anaconda/post-scripts/install-configure-upgrade.ks
%post --erroronfail --log=/tmp/anaconda_custom_logs/bootc-switch.log
bootc switch --mutate-in-place --enforce-container-sigpolicy --transport registry $imageref:$imagetag
%end
PSEOF

# =============================================================================
# POST-SCRIPT : ENROLLMENT CLÉ SECURE BOOT
# =============================================================================

cat << 'SBEOF' >> /usr/share/anaconda/post-scripts/secureboot-enroll-key.ks
%post --erroronfail --nochroot --log=/tmp/anaconda_custom_logs/secureboot-enroll-key.log
set -oue pipefail
readonly ENROLLMENT_PASSWORD="gablue"
readonly SECUREBOOT_KEY="/usr/share/gablue/sb_pubkey.der"
if [[ ! -d "/sys/firmware/efi" ]]; then
    echo "Mode EFI non détecté. Enrollment de clé ignoré."
    exit 0
fi
if [[ ! -f "$SECUREBOOT_KEY" ]]; then
    echo "Clé Secure Boot non trouvée."
    exit 0
fi
mokutil --timeout -1 || :
echo -e "$ENROLLMENT_PASSWORD\n$ENROLLMENT_PASSWORD" | mokutil --import "$SECUREBOOT_KEY" || :
%end
SBEOF

# =============================================================================
# DÉSACTIVATION DES SERVICES DANS LE LIVE
# =============================================================================

echo "Désactivation des services non nécessaires dans le live..."
(
    set +e
    for s in \
        rpm-ostree-countme.service \
        tailscaled.service \
        system-flatpak-setup.service \
        rpm-ostreed-automatic.timer \
        flatpak-system-update.timer \
        cec-poweroff-tv.service \
        cec-active-source.timer \
        dmemcg-booster-system.service \
        scx_loader.service \
        displaylink.service \
        brew-upgrade.timer \
        brew-update.timer \
        brew-setup.service \
        ublue-os-libvirt-workarounds.service \
        gablue-dx-groups.service \
        bootloader-update.service \
        greenboot-healthcheck.service \
        greenboot-set-rollback-trigger.service; do
        if systemctl list-unit-files "$s" > /dev/null 2>&1; then
            systemctl disable "$s"
        fi
    done
    for s in \
        podman-auto-update.timer \
        flatpak-user-update.timer \
        dmemcg-booster-user.service; do
        if systemctl --global list-unit-files "$s" > /dev/null 2>&1; then
            systemctl --global disable "$s"
        fi
    done
)

# =============================================================================
# TWEAKS NVIDIA (LIVE)
# =============================================================================

# Fix GSK_RENDERER pour les variantes NVIDIA
if [[ $imageref == *-nvidia* ]]; then
    mkdir -p /etc/environment.d /etc/skel/.config/environment.d
    echo "GSK_RENDERER=gl" >> /etc/environment.d/99-nvidia-fix.conf
    echo "GSK_RENDERER=gl" >> /etc/skel/.config/environment.d/99-nvidia-fix.conf
fi

# Réactiver nouveau pour les variantes NVIDIA (kernel vanilla pas de proprio)
if [[ $imageref == *-nvidia* ]]; then
    for pkg in nvidia-gpu-firmware mesa-vulkan-drivers; do
        dnf -yq reinstall --allowerasing $pkg || dnf -yq install --allowerasing $pkg
    done
fi

# =============================================================================
# SUPPRESSION APPLICATIONS LOURDES (LIVE UNIQUEMENT)
# =============================================================================

# Ne pas lancer Steam au login dans le live
rm -vf /etc/skel/.config/autostart/steam*.desktop

# Supprimer les applis lourdes inutiles dans le live
dnf -yq remove steam lutris waydroid || :

# Désactiver l'écran de bienvenue Plasma dans le live
# Le module KDED kded_plasma_welcome.so le lance automatiquement au boot
dnf -yq remove plasma-welcome || :

# Ne pas vérifier l'image vérifiée
rm -vf /etc/profile.d/verify_motd.sh

# =============================================================================
# INSTALLATION GPARTED
# =============================================================================

dnf -yq install gparted

# =============================================================================
# NETTOYAGE
# =============================================================================

dnf clean all -yq
