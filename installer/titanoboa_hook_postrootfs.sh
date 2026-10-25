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

dnf install -qy --enable-repo=fedora-cisco-openh264 --allowerasing anaconda-live libblockdev-{btrfs,lvm,dm}

mkdir -p /var/lib/rpm-state

# Outils des dialogues utilisateur de l'installateur :
#   qrencode = QR de la doc Secure Boot
#   yad      = dialogues (déjà installé par build.sh pour livesys, réaffirmé ici)
#   jq       = lecture du JSON de lsblk pour la détection BitLocker
dnf install -qy --setopt=install_weak_deps=0 qrencode yad jq

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
SECUREBOOT_DOC_URL="https://github.com/elgabo86/gablue"
SECUREBOOT_QR="/usr/share/gablue/secure_boot_qr.png"

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

# Détection d'une partition Windows BitLocker : Anaconda peut se bloquer ou
# échouer dessus (partition illisible). On prévient AVANT le partitionnement et
# on laisse l'utilisateur annuler proprement (porté de Bazzite, #4577/d541fd8).
%pre --erroronfail --log=/tmp/anaconda_custom_logs/detect_bitlocker.log
DOCS_QR=/tmp/detect_bitlocker_qr.png
IS_BITLOCKER=\$(lsblk -o FSTYPE --json | jq '.blockdevices | map(select(.fstype == "BitLocker")) | . != []')
{ WARNING_MSG="\$(</dev/stdin)"; } << 'WARNINGEOF'
<span size="x-large">Partition Windows BitLocker détectée</span>

L'installation peut être interrompue par cette partition.
Dans ce cas, faites <b>une</b> des actions suivantes :
    a) Débrancher le disque concerné.
    b) Désactiver BitLocker dans Windows.
    c) Supprimer la partition avec GNOME Disks.

Voulez-vous continuer ?
WARNINGEOF

if [[ \$IS_BITLOCKER =~ true ]]; then
    qrencode -o "\$DOCS_QR" "https://www.wikihow.com/Turn-Off-BitLocker"
    _EXITLOCK=1
    while [[ \$_EXITLOCK -ne 0 ]]; do
        run0 --user=liveuser yad --on-top --center --skip-taskbar \
            --image="\$DOCS_QR" --text="\$WARNING_MSG" \
            --button="Oui, continuer":0 --button="Annuler l'installation":10
        _RETCODE=\$?
        case \$_RETCODE in
            0) _EXITLOCK=0 ;;
            10) _EXITLOCK=0; pkill liveinst; exit 0 ;;
        esac
    done
fi
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

# Questions interactives Gablue (compression, flatpaks, cache gwine)
# Posées en %pre-install (après formatage, avant déploiement) pour ne plus
# interrompre l'installation ensuite. Les choix sont écrits dans /tmp et lus
# par les %post correspondants. La compression BTRFS est appliquée
# directement ici (héritée par le déploiement, /var et les flatpaks).
%include /usr/share/anaconda/pre-scripts/gablue-questions.ks

# Afficher les logs d'installation en cas d'erreur
%onerror
run0 --user=liveuser yad --timeout=0 --text-info --no-buttons \
    --width=600 --height=400 \
    --text="Une erreur est survenue pendant l'installation. Veuillez signaler ce problème." \
    < /tmp/anaconda.log
%end

# Langue et clavier par défaut (préremplis dans l'UI, modifiables par l'utilisateur)
lang fr_CH.UTF-8
keyboard --vckeymap=ch-fr --xlayouts='ch (fr)'

ostreecontainer --url=$imageref:$imagetag --transport=containers-storage --no-signature-verification
%include /usr/share/anaconda/post-scripts/install-configure-upgrade.ks
%include /usr/share/anaconda/post-scripts/disable-fedora-flatpak.ks
%include /usr/share/anaconda/post-scripts/install-flatpaks.ks
%include /usr/share/anaconda/post-scripts/restore-flatpak-selinux.ks
%include /usr/share/anaconda/post-scripts/install-extra.ks
%include /usr/share/anaconda/post-scripts/secureboot-enroll-key.ks
%include /usr/share/anaconda/post-scripts/secureboot-docs.ks
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
# POST-SCRIPT : DOCUMENTATION SECURE BOOT (QR + rappel d'enrollment)
# =============================================================================

# La clé est seulement *ajoutée à la requête MOK* par le script précédent : si
# l'utilisateur ne valide pas MokManager au démarrage suivant, l'enrollment est
# perdu et les kmods signés ne se chargent pas (secureboot muet). Ce dialog le
# lui rappelle explicitement, avec un QR vers la doc. Non bloquant.
cat << DOCEOF >> /usr/share/anaconda/post-scripts/secureboot-docs.ks
%post --nochroot --log=/tmp/anaconda_custom_logs/secureboot-docs.log
if [[ ! -d /sys/firmware/efi ]]; then
    exit 0
fi
if LC_ALL=C mokutil -t "$SECUREBOOT_KEY" 2>/dev/null | grep -q "already in the enrollment request"; then
    run0 --user=liveuser yad --timeout=0 --on-top --center --skip-taskbar \
        --button=Ok:0 --image="$SECUREBOOT_QR" --text="<b>Clé Secure Boot Gablue ajoutée</b>
Au prochain démarrage, le menu MokManager s'ouvre :
« Enroll MOK » → « Continue » → mot de passe <b>$ENROLLMENT_PASSWORD</b>

Documentation : $SECUREBOOT_DOC_URL"
fi
%end
DOCEOF

# QR généré au build (qrencode installé plus haut), embarqué dans le live
qrencode -o "$SECUREBOOT_QR" "$SECUREBOOT_DOC_URL"

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
# FLATPAKS VISIBLES DANS LA SESSION LIVE
# =============================================================================

# Rend les flatpaks pré-cachés visibles dans le menu Plasma du live
# system-flatpak-setup.service est désactivé (inutile dans un live readonly),
# on configure manuellement XDG_DATA_DIRS pour exposer les .desktop flatpak
mkdir -p /etc/environment.d
cat > /etc/environment.d/99-gablue-flatpak-live.conf << 'EOF'
XDG_DATA_DIRS=/var/lib/flatpak/exports/share:/usr/local/share:/usr/share
EOF

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
    # Garde-fou : le live NVIDIA n'a AUCUN pilote propriétaire (kernel vanilla) et
    # repose entièrement sur nouveau. Sans ICD Vulkan nouveau (fichier renommé ou
    # supprimé par une mise à jour de Mesa), l'ISO se construit quand même et
    # l'utilisateur se retrouve avec un rendu logiciel sans explication → on
    # échoue au build plutôt qu'à l'exécution.
    (
        shopt -u nullglob
        ls /usr/share/vulkan/icd.d/nouveau_icd.*.json > /dev/null
    ) || {
        echo >&2 "ERREUR : aucun ICD Vulkan nouveau dans /usr/share/vulkan/icd.d/ (live NVIDIA sans accélération GPU)"
        exit 1
    }
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

# rpm-ostree n'a aucun sens dans une session live (racine = overlay jetable) et
# laisse le système dans un état incohérent si l'utilisateur le lance : on retire
# le binaire du live (Bazzite 1fe630e). L'installateur passe par bootc/ostree.
rm -f /usr/bin/rpm-ostree

# =============================================================================
# INSTALLATION GPARTED + XVFB
# =============================================================================

dnf -yq install gparted xorg-x11-server-Xvfb

# =============================================================================
# NETTOYAGE
# =============================================================================

dnf clean all -yq
