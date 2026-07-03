#!/usr/bin/bash
# Hook pre-initramfs Gablue : swap kernel OGC → vanilla Fedora
# 
# Le kernel OGC (optimisé gaming) n'est pas signé pour Secure Boot.
# On le remplace par le kernel vanilla Fedora (signé) pour que le
# live ISO puisse booter avec Secure Boot activé.
# 
# Adapté de Bazzite (ublue-os/bazzite) pour Gablue.

set -exo pipefail

# =============================================================================
# SUPPRESSION DU KERNEL OGC
# =============================================================================

kernel_pkgs=(
    kernel
    kernel-core
    kernel-devel
    kernel-devel-matched
    kernel-modules
    kernel-modules-core
    kernel-modules-extra
)
dnf -y versionlock delete "${kernel_pkgs[@]}"
dnf --setopt=protect_running_kernel=False -y remove "${kernel_pkgs[@]}"
(cd /usr/lib/modules && rm -rf -- ./*)

# =============================================================================
# INSTALLATION DU KERNEL VANILLA FEDORA (SIGNÉ SECURE BOOT)
# =============================================================================

dnf -y --repo fedora,updates --setopt=tsflags=noscripts install kernel kernel-core
kernel=$(find /usr/lib/modules -maxdepth 1 -type d -printf '%P\n' | grep .)
depmod "$kernel"

# =============================================================================
# RÉFÉRENCE DE L'IMAGE (POUR LE HOOK POSTROOTFS)
# =============================================================================

imageref="$(podman images --format '{{ index .Names 0 }}\n' 'gablue*' | head -1)"
imageref="${imageref##*://}"
imageref="${imageref%%:*}"

# =============================================================================
# FIRMWARE NVIDIA (OPTIONNEL, POUR VARIANTES NVIDIA)
# =============================================================================

dnf install -yq nvidia-gpu-firmware || :

dnf clean all -yq
