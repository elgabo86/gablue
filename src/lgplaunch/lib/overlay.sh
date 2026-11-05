# =============================================================================
# Gestion des overlays — kernel overlayfs natif via unshare (user namespaces)
# =============================================================================
# Les overlays sont montés dans le même unshare que le jeu (montage différé).

# Variable globale : liste des overlays kernel à monter (une ligne par overlay)
# Format: lowerdir|upperdir|workdir|mountpoint
_LGP_KERNEL_OVERLAY_MOUNTS=""
_LGP_USING_KERNEL_OVERLAY=false

# Vérifie que le kernel overlayfs via user namespaces est disponible
_lgp_require_kernel_overlay() {
    if ! unshare -U -m --map-root-user true 2>/dev/null; then
        error_exit "Kernel overlayfs indisponible : user namespaces non supportés"
    fi
    if [ ! -f /proc/filesystems ] || ! grep -qw overlay /proc/filesystems 2>/dev/null; then
        error_exit "Kernel overlayfs indisponible : module overlay non chargé"
    fi
}

# Génère un identifiant court pour un mountpoint
_lgp_kernel_overlay_id() {
    local mp="$1"
    echo "$mp" | md5sum 2>/dev/null | cut -c1-8 || echo "$mp" | cksum 2>/dev/null | cut -d' ' -f1
}

# Enregistre un overlay kernel (montage différé dans le unshare du jeu)
_lgp_register_kernel_overlay() {
    local lower="$1"
    local upper="$2"
    local work="$3"
    local mountpoint="$4"
    
    _LGP_KERNEL_OVERLAY_MOUNTS+="${lower}|${upper}|${work}|${mountpoint}"$'\n'
    _LGP_USING_KERNEL_OVERLAY=true
    
    local pid_file="/tmp/lgp-ko-$UID-$( _lgp_kernel_overlay_id "$mountpoint" ).pid"
    echo "deferred" > "$pid_file"
    
    echo "Overlay kernel enregistré (montage différé): $mountpoint"
}

# Configure les symlinks dans le namespace courant pour traduire
# les chemins /tmp/lgp-* hardcodés des .lgp vers leur version $UID
# Utilise des symlinks plutôt que des bind mounts car mount --rbind
# échoue dans les namespaces utilisateur (unshare -U)
_lgp_setup_bind_mounts() {
    for base in lgp-saves lgp-extra lgp-temp lgpackmount edenln \
                lgp-temp-upper lgp-temp-work \
                lgp-full-overlay lgp-full-overlay-upper lgp-full-overlay-work; do
        umount "/tmp/$base" 2>/dev/null || true
        rm -rf "/tmp/$base" 2>/dev/null || true
        ln -sfn "/tmp/$base-$UID" "/tmp/$base"
    done
}

# Génère les commandes bash pour monter les overlays kernel enregistrés
_lgp_kernel_overlay_mount_script() {
    local script=""
    while IFS='|' read -r lower upper work mountpoint; do
        [ -z "$lower" ] && continue
        script+="mkdir -p \"$(dirname "$mountpoint")\" 2>/dev/null; "
        script+="mount -t overlay overlay -o lowerdir=\"$lower\",upperdir=\"$upper\",workdir=\"$work\" \"$mountpoint\" 2>/dev/null || true; "
    done <<< "$_LGP_KERNEL_OVERLAY_MOUNTS"
    echo "$script"
}

# Nettoie les marqueurs kernel overlay après usage
_lgp_cleanup_kernel_overlay_markers() {
    while IFS='|' read -r lower upper work mountpoint; do
        [ -z "$mountpoint" ] && continue
        rm -f "/tmp/lgp-ko-$UID-$( _lgp_kernel_overlay_id "$mountpoint" ).pid"
    done <<< "$_LGP_KERNEL_OVERLAY_MOUNTS"
    _LGP_KERNEL_OVERLAY_MOUNTS=""
    _LGP_USING_KERNEL_OVERLAY=false
}

# Monte un overlay (kernel overlayfs différé)
mount_overlay() {
    _lgp_require_kernel_overlay
    _lgp_register_kernel_overlay "$@"
}

# Démonte un overlay kernel
unmount_overlay() {
    local mount_point="$1"
    
    [ -z "$mount_point" ] && return 0
    
    local pid_file="/tmp/lgp-ko-$UID-$( _lgp_kernel_overlay_id "$mount_point" ).pid"
    if [ -f "$pid_file" ]; then
        rm -f "$pid_file"
        return 0
    fi
    
    if mountpoint -q "$mount_point" 2>/dev/null; then
        umount -l "$mount_point" 2>/dev/null || true
    fi
}

# Nettoie les overlays kernel orphelins au démarrage
_lgp_cleanup_kernel_overlay_pidfiles() {
    for pid_file in /tmp/lgp-ko-$UID-*.pid; do
        [ -f "$pid_file" ] || continue
        local marker
        marker=$(cat "$pid_file" 2>/dev/null)
        if [ "$marker" = "deferred" ]; then
            rm -f "$pid_file"
        elif [ -n "$marker" ] && kill -0 "$marker" 2>/dev/null; then
            kill "$marker" 2>/dev/null || true
            rm -f "$pid_file"
        else
            rm -f "$pid_file"
        fi
    done
}
_lgp_cleanup_kernel_overlay_pidfiles

