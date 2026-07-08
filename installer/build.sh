#!/usr/bin/bash
# Script principal de construction du Live ISO Gablue
# Inspiré de Bazzite (ublue-os/bazzite) - adapté pour Gablue
#
# Étapes :
# 1. Pré-cache des Flatpaks (Firefox, VLC, Audacious)
# 2. Pull de l'image à installer dans le stockage local
# 3. Copie des fichiers système Gablue
# 4. Swap kernel OGC → vanilla Fedora (Secure Boot)
# 5. Régénération initramfs avec dracut-live
# 6. Installation livesys-scripts (session live KDE)
# 7. Installation Anaconda + configuration kickstart
# 8. Tweaks session live (services, NVIDIA, etc.)
# 9. Overrides branding Gablue
# 10. Configuration EFI + ISO

set -eoux pipefail

{ export PS4='+( ${BASH_SOURCE}:${LINENO} ): ${FUNCNAME[0]:+${FUNCNAME[0]}(): }'; } 2>/dev/null

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_IMAGE=${BASE_IMAGE:?}
INSTALL_IMAGE_PAYLOAD=${INSTALL_IMAGE_PAYLOAD:?}

# =============================================================================
# PRÉPARATION
# =============================================================================

# Créer le répertoire que /root symlink
mkdir -p "$(realpath /root)"

# bwrap tente d'écrire dans /proc/sys/user/max_user_namespaces (monté ro)
# On le remonte en rw
mount -o remount,rw /proc/sys

# =============================================================================
# FLATPAKS - PRÉ-CACHE POUR INSTALLATION OFFLINE
# =============================================================================

echo "Installation des Flatpaks dans l'image live..."
curl --retry 3 -Lo /etc/flatpak/remotes.d/flathub.flatpakrepo https://dl.flathub.org/repo/flathub.flatpakrepo
xargs -r flatpak install -y --noninteractive < <(cat /src/flatpaks /src/flatpaks-optional)
# Nettoyer le cache de téléchargement flatpak pour libérer de l'espace disque
rm -rf /root/.cache /var/tmp/*

# =============================================================================
# PULL DE L'IMAGE À INSTALLER DANS LE STOCKAGE LOCAL
# =============================================================================

echo "Récupération de l'image à installer..."
if mountpoint -q /usr/lib/containers/storage || mountpoint -q /var/lib/containers/storage; then
    # Si le stockage est un mountpoint, on utilise save/load local
    podman save --format oci-archive "$INSTALL_IMAGE_PAYLOAD" | podman load --storage-opt additionalimagestore=''
else
    podman pull "$INSTALL_IMAGE_PAYLOAD"
fi
# Nettoyer le cache de téléchargement podman avant la suite du build
rm -rf /var/tmp/* /root/.cache

# =============================================================================
# FICHIERS SYSTÈME GABLUE
# =============================================================================

echo "Copie des fichiers système partagés..."
cp -a /src/system_files/shared/. /

# Copier les listes de flatpaks pour le post-install
mkdir -p /usr/share/gablue
cp /src/flatpaks /usr/share/gablue/flatpaks-required
cp /src/flatpaks-optional /usr/share/gablue/flatpaks-optional

# Ajouter les runtimes NVIDIA flatpak pour les variantes NVIDIA (64 et 32 bits)
if [[ "${BASE_IMAGE:-}" == *nvidia* ]]; then
    NVIDIA_VER=$(rpm -q nvidia-driver --qf '%{VERSION}' 2>/dev/null || true)
    if [ -n "$NVIDIA_VER" ]; then
        echo "runtime/org.freedesktop.Platform.GL.nvidia-${NVIDIA_VER//./-}/x86_64/1.4" >> /usr/share/gablue/flatpaks-required
        echo "runtime/org.freedesktop.Platform.GL32.nvidia-${NVIDIA_VER//./-}/x86_64/1.4" >> /usr/share/gablue/flatpaks-required
        echo "Runtimes NVIDIA $NVIDIA_VER (64+32 bits) ajoutés à la liste flatpak"
    fi
fi

# =============================================================================
# HOOK PRE-INITRAMFS : SWAP KERNEL OGC → VANILLA FEDORA
# =============================================================================

"$SCRIPT_DIR/titanoboa_hook_preinitramfs.sh"

# =============================================================================
# INITRAMFS LIVE (dracut-live)
# =============================================================================

echo "Génération de l'initramfs live..."
dnf install -y dracut-live
kernel=$(kernel-install list --json pretty | jq -r '.[] | select(.has_kernel == true) | .version')
DRACUT_NO_XATTR=1 dracut -v --force --zstd --reproducible --no-hostonly \
    --add "dmsquash-live dmsquash-live-autooverlay" \
    "/usr/lib/modules/${kernel}/initramfs.img" "${kernel}"

# =============================================================================
# LIVESYS-SCRIPTS (SESSION LIVE KDE)
# =============================================================================

echo "Installation de livesys-scripts et yad..."
dnf install -y livesys-scripts yad
sed -i "s/^livesys_session=.*/livesys_session=kde/" /etc/sysconfig/livesys
systemctl enable livesys.service livesys-late.service

# =============================================================================
# HOOK POST-ROOTFS : ANACONDA + LIVE TWEAKS
# =============================================================================

"$SCRIPT_DIR/titanoboa_hook_postrootfs.sh"

# =============================================================================
# OVERRIDES BRANDING GABLUE
# =============================================================================

echo "Copie des overrides Gablue..."
if [ -d /src/system_files/overrides ]; then
    cp -af /src/system_files/overrides/. /
fi

# =============================================================================
# PACK CACHE GWINE (OFFLINE) → /extra
# =============================================================================

# Génère un pack cache gwine complet (runners gwine/gwine-proton, DXVK,
# DXVK-GPLAsync, VKD3D-Proton, DXVK-NVAPI, Wine Mono/Gecko, composants Windows)
# et le place dans /extra sous forme de dossier installer déployable offline.
# /extra n'existe que dans le rootfs live : l'installation sur disque redéploie
# l'image container propre (ostreecontainer + bootc switch), donc /extra ne se
# retrouve jamais sur l'OS installé.
echo "Génération du pack cache gwine pour /extra..."
if command -v gwine > /dev/null 2>&1; then
    gwine_pack_tmp="$(mktemp -d)"
    if gwine --download-components && ( cd "$gwine_pack_tmp" && gwine --cachepack ); then
        mkdir -p /extra
        cp -a "$gwine_pack_tmp/gwine-cache-installer" /extra/
        echo "Pack cache gwine déployé dans /extra/gwine-cache-installer"
    else
        echo "AVERTISSEMENT : échec de la génération du pack cache gwine, /extra ignoré"
    fi
    # Le cache brut fait doublon avec l'archive du pack : on le supprime du payload
    rm -rf "$gwine_pack_tmp" "$HOME/.cache/gwine"
else
    echo "AVERTISSEMENT : gwine introuvable, pack cache /extra ignoré"
fi

# =============================================================================
# CONTENU EXTRA PERSONNALISÉ (BUILD LOCAL) → /extra
# =============================================================================

# Copie le contenu du dossier installer/extra/ (bind-monté sur /src/extra) dans
# /extra du live. Permet d'embarquer des fichiers/dossiers arbitraires pour les
# builds locaux. Le dossier est gitignore : absent ou vide en CI → section
# ignorée. /extra n'existe que dans le live, jamais sur l'OS installé.
if [ -d /src/extra ] && [ -n "$(ls -A /src/extra 2>/dev/null | grep -v '^.gitkeep$')" ]; then
    echo "Copie du contenu extra personnalisé dans /extra..."
    mkdir -p /extra
    cp -a /src/extra/. /extra/
    rm -f /extra/.gitkeep
fi

# =============================================================================
# CONFIGURATION EFI POUR L'ISO
# =============================================================================

# image-builder a besoin de gcdx64.efi
dnf install -y grub2-efi-x64-cdboot

# image-builder attend le répertoire EFI dans /boot/efi
mkdir -p /boot/efi
cp -av /usr/lib/efi/*/*/EFI /boot/efi/

# Remplacer le fallback efi
cp -v /boot/efi/EFI/fedora/grubx64.efi /boot/efi/EFI/BOOT/fbx64.efi

# =============================================================================
# FUSEAU HORAIRE UTC
# =============================================================================

rm -f /etc/localtime
systemd-firstboot --timezone UTC

# =============================================================================
# /var/tmp SUR TMPFS PLUS LARGE
# =============================================================================

# / dans un ISO live est un overlayfs avec upperdir sous /run
# /var/tmp est donc sous /run (tmpfs petit)
# ostree a besoin de beaucoup d'espace dans /var/tmp
# On monte un tmpfs plus large à la place
rm -rf /var/tmp
mkdir /var/tmp
cat > /etc/systemd/system/var-tmp.mount << 'UNITEOF'
[Unit]
Description=Tmpfs large pour /var/tmp sur le système live

[Mount]
What=tmpfs
Where=/var/tmp
Type=tmpfs
Options=size=50%,nr_inodes=1m,x-systemd.graceful-option=usrquota

[Install]
WantedBy=local-fs.target
UNITEOF
systemctl enable var-tmp.mount

# =============================================================================
# /var/lib/flatpak EN LECTURE SEULE
# =============================================================================

cat > /etc/systemd/system/var-lib-flatpak.mount << 'UNITEOF'
[Mount]
Type=none
What=/var/lib/flatpak
Where=/var/lib/flatpak
Options=bind,ro

[Install]
WantedBy=multi-user.target
UNITEOF
systemctl enable var-lib-flatpak.mount

# =============================================================================
# CONFIG ISO POUR IMAGE-BUILDER
# =============================================================================

mkdir -p /usr/lib/bootc-image-builder
cp /src/iso.yaml /usr/lib/bootc-image-builder/iso.yaml

# =============================================================================
# NETTOYAGE FINAL
# =============================================================================

dnf clean all
