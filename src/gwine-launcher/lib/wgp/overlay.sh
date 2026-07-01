#!/bin/bash

################################################################################
# wgp-overlay.sh - Gestion des overlays kernel overlayfs pour fichiers temporaires
################################################################################

# =============================================================================
# Préparation des dossiers temporaires
# =============================================================================

prepare_temps() {
    local TEMPPATH_FILE="$MOUNT_DIR/.temppath"
    
    [ -f "$TEMPPATH_FILE" ] || return 0
    
    if grep -q "^\*$" "$TEMPPATH_FILE" 2>/dev/null; then
        echo "Mode overlay complet détecté (* dans .temppath)"
        prepare_full_overlay
        return 0
    fi
    
    prepare_temp_folders
}

prepare_temp_folders() {
    local TEMPPATH_FILE="$MOUNT_DIR/.temppath"
    local TEMP_WGP_DIR="$MOUNT_DIR/.temp"
    local TEMP_GAME_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"
    local GAME_ID
    
    if [ -n "${_GAME_TEMP_ID:-}" ]; then
        GAME_ID="$_GAME_TEMP_ID"
    else
        GAME_ID="wgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')"
    fi
    
    local TEMP_UPPER="/tmp/wgp-temp-upper/$GAME_ID"

    if [ ! -d "$TEMP_WGP_DIR" ]; then
        echo "Dossier .temp non trouvé dans le WGP, skip overlay"
        return 0
    fi

    # S'assurer que les repertoires existent AVANT le montage (crucial pour sandbox)
    ensure_dirs -s "$TEMP_GAME_DIR" "$TEMP_UPPER"
    chmod 777 "/tmp/wgp-temp-upper" 2>/dev/null || true

    echo "Montage de l'overlay pour les dossiers temporaires..."

    if mountpoint -q "$TEMP_GAME_DIR" 2>/dev/null; then
        echo "Démontage de l'overlay existant (rapide)..."
        unmount_overlay "$TEMP_GAME_DIR" -f
    fi

    local WORK_DIR="/tmp/wgp-temp-work/$GAME_ID"
    ensure_dir -s "$WORK_DIR"

    echo "  lowerdir: $TEMP_WGP_DIR"
    echo "  upperdir: $TEMP_UPPER"
    echo "  workdir: $WORK_DIR"
    echo "  mountpoint: $TEMP_GAME_DIR"
    
    mount_overlay "$TEMP_WGP_DIR" "$TEMP_UPPER" "$WORK_DIR" "$TEMP_GAME_DIR"

    export _TEMP_FOLDERS_MOUNT="$TEMP_GAME_DIR"
    export _TEMP_FOLDERS_UPPER="$TEMP_UPPER"

    echo "Overlay monté avec succès: $TEMP_GAME_DIR"
}

# =============================================================================
# Overlay complet (mode *)
# =============================================================================

prepare_full_overlay() {
    local GAME_ID="wgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')"
    local FULL_OVERLAY_BASE="/tmp/wgp-full-overlay"
    local MOUNT_OVERLAY="$FULL_OVERLAY_BASE/$GAME_ID"
    local TEMP_UPPER="$FULL_OVERLAY_BASE-upper/$GAME_ID"
    
    if [ -d "$MOUNT_OVERLAY" ]; then
        echo "Nettoyage de l'overlay précédent..."
        unmount_overlay "$MOUNT_OVERLAY" -f -l
        rm -rf "$MOUNT_OVERLAY" 2>/dev/null || true
    fi
    
    rm -rf "$TEMP_UPPER" 2>/dev/null || true
    
    ensure_dirs -s "$MOUNT_OVERLAY" "$TEMP_UPPER"
    chmod 777 "/tmp/wgp-full-overlay" "/tmp/wgp-full-overlay-upper" 2>/dev/null || true
    
    echo "Montage de l'overlay complet..."
    echo "  lowerdir: $MOUNT_DIR (WGP entier - lecture seule)"
    echo "  upperdir: $TEMP_UPPER (writable)"
    echo "  mountpoint: $MOUNT_OVERLAY"
    
    local WORK_DIR="/tmp/wgp-full-overlay-work/$GAME_ID"
    ensure_dir -s "$WORK_DIR"
    
    mount_overlay "$MOUNT_DIR" "$TEMP_UPPER" "$WORK_DIR" "$MOUNT_OVERLAY"
    
    local REL_EXE="${FULL_EXE_PATH#$MOUNT_DIR/}"
    # Utiliser le chemin via le symlink (bindé dans le sandbox) au lieu du chemin réel
    FULL_EXE_PATH="$TEMP_SYMLINK/$GAME_INTERNAL_NAME/$REL_EXE"
    
    export _FULL_OVERLAY_MOUNT="$MOUNT_OVERLAY"
    export _FULL_OVERLAY_UPPER="$TEMP_UPPER"
    
    echo "Overlay complet monté avec succès: $MOUNT_OVERLAY"
    echo "  Exécutable redirigé: $FULL_EXE_PATH"
}

# =============================================================================
# Nettoyage des overlays temporaires
# =============================================================================

cleanup_temp_symlink() {
    local GAME_ID=""
    
    if [ -n "${_FULL_OVERLAY_MOUNT:-}" ] || [ -n "${_FULL_OVERLAY_UPPER:-}" ]; then
        local MOUNT_OVERLAY="${_FULL_OVERLAY_MOUNT:-}"
        local TEMP_UPPER="${_FULL_OVERLAY_UPPER:-}"
        
        if [ -n "$GAME_INTERNAL_NAME" ] && [ -z "$GAME_ID" ]; then
            GAME_ID="wgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')"
        fi
        
        if [ -n "$MOUNT_OVERLAY" ]; then
            echo "Nettoyage de l'overlay complet..."
            # Lazy unmount rapide (-f seul suffit maintenant)
            unmount_overlay "$MOUNT_OVERLAY" -f
            
            # Nettoyer les répertoires après démontage (détaché immédiatement)
            rm -rf "$MOUNT_OVERLAY" 2>/dev/null || true
            rm -rf "$TEMP_UPPER" 2>/dev/null || true
            rm -rf "/tmp/wgp-full-overlay-work/${GAME_ID}" 2>/dev/null || true
        fi
        
        unset _FULL_OVERLAY_MOUNT _FULL_OVERLAY_UPPER
    fi
    
    if [ -n "${_TEMP_FOLDERS_MOUNT:-}" ] || [ -n "${_TEMP_FOLDERS_UPPER:-}" ]; then
        local TEMP_MOUNT="${_TEMP_FOLDERS_MOUNT:-}"
        local TEMP_UPPER="${_TEMP_FOLDERS_UPPER:-}"
        
        if [ -n "$GAME_INTERNAL_NAME" ] && [ -z "$GAME_ID" ]; then
            GAME_ID="wgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')"
        fi
        
        if [ -n "$TEMP_MOUNT" ]; then
            echo "Nettoyage de l'overlay temp folders..."
            # Lazy unmount rapide
            unmount_overlay "$TEMP_MOUNT" -f
            
            # Nettoyer les répertoires après démontage
            rm -rf "$TEMP_MOUNT" 2>/dev/null || true
            rm -rf "$TEMP_UPPER" 2>/dev/null || true
            rm -rf "/tmp/wgp-temp-work/${GAME_ID}" 2>/dev/null || true
        fi
        
        unset _TEMP_FOLDERS_MOUNT _TEMP_FOLDERS_UPPER
    fi
    
    if [ -n "$GAME_INTERNAL_NAME" ] && [ -z "$GAME_ID" ]; then
        GAME_ID="wgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')"
    fi
    
    if [ -n "$GAME_ID" ]; then
        local default_full_mount="/tmp/wgp-full-overlay/$GAME_ID"
        local default_full_upper="/tmp/wgp-full-overlay-upper/$GAME_ID"
        
        if mountpoint -q "$default_full_mount" 2>/dev/null; then
            unmount_overlay "$default_full_mount" -f
            rm -rf "$default_full_mount" 2>/dev/null || true
            rm -rf "$default_full_upper" 2>/dev/null || true
            rm -rf "/tmp/wgp-full-overlay-work/${GAME_ID}" 2>/dev/null || true
        fi
        
        local default_temp_mount="/tmp/wgp-temp/$GAME_ID"
        local default_temp_upper="/tmp/wgp-temp-upper/$GAME_ID"
        
        if mountpoint -q "$default_temp_mount" 2>/dev/null; then
            unmount_overlay "$default_temp_mount" -f
            rm -rf "$default_temp_mount" 2>/dev/null || true
            rm -rf "$default_temp_upper" 2>/dev/null || true
            rm -rf "/tmp/wgp-temp-work/${GAME_ID}" 2>/dev/null || true
        fi
        
        # Nettoyer les symlinks dans TEMP_SYMLINK
        if [ -n "$GAME_INTERNAL_NAME" ]; then
            rm -f "$TEMP_SYMLINK/$GAME_INTERNAL_NAME" 2>/dev/null || true
        fi
    fi
}

# =============================================================================
# Préparation des dossiers overlay (appelé avant le montage)
# =============================================================================

setup_temp_symlink() {
    local TEMPPATH_FILE="$MOUNT_DIR/.temppath"
    local GAME_ID="wgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')"
    
    export _GAME_TEMP_ID="$GAME_ID"

    if [ ! -f "$TEMPPATH_FILE" ]; then
        return 0
    fi
    
    # S'assurer que le répertoire de base existe (nécessaire pour l'overlay)
    # Utiliser le chemin reel derriere le symlink
    if [ -L "$TEMP_REAL" ] && [ ! -e "$TEMP_REAL" ]; then
        # Symlink casse, creer le repertoire cible
        local real_temp
        real_temp=$(readlink -f "$TEMP_REAL" 2>/dev/null) || real_temp="$SHARED_TMP_DIR/wgp-temp"
        ensure_dir -s "$real_temp"
    else
        ensure_dir -s "$TEMP_REAL"
    fi
    
    if grep -q "^\*$" "$TEMPPATH_FILE" 2>/dev/null; then
        local FULL_OVERLAY_BASE="/tmp/wgp-full-overlay"
        local MOUNT_OVERLAY="$FULL_OVERLAY_BASE/$GAME_ID"
        local TEMP_UPPER="$FULL_OVERLAY_BASE-upper/$GAME_ID"
        local TEMP_WORK="/tmp/wgp-full-overlay-work/$GAME_ID"
        
        if mountpoint -q "$MOUNT_OVERLAY" 2>/dev/null; then
            echo "Démontage de l'overlay existant (rapide)..."
            unmount_overlay "$MOUNT_OVERLAY" -f
        fi
        
        if [ -d "$MOUNT_OVERLAY" ]; then
            rmdir "$MOUNT_OVERLAY" 2>/dev/null || rm -rf "$MOUNT_OVERLAY" 2>/dev/null || true
        fi
        rm -rf "$TEMP_UPPER" 2>/dev/null || true
        rm -rf "$TEMP_WORK" 2>/dev/null || true
        
        ensure_dirs -s "$MOUNT_OVERLAY" "$TEMP_UPPER" "$TEMP_WORK"
        chmod 777 "/tmp/wgp-full-overlay" "/tmp/wgp-full-overlay-upper" "/tmp/wgp-full-overlay-work" 2>/dev/null || true
        chmod 777 "$TEMP_UPPER" "$TEMP_WORK"
        
        # Créer le symlink dans TEMP_SYMLINK pour le sandbox (comme saves et extras)
        ensure_dir -s "$TEMP_SYMLINK"
        local temp_symlink="$TEMP_SYMLINK/$GAME_INTERNAL_NAME"
        rm -f "$temp_symlink"
        ln -s "$MOUNT_OVERLAY" "$temp_symlink"
        echo "Symlink temp créé: $temp_symlink -> $MOUNT_OVERLAY"
        
        echo "Dossiers overlay préparés (mode full): $GAME_ID"
    else
        local TEMP_GAME_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"
        local TEMP_UPPER="/tmp/wgp-temp-upper/$GAME_ID"
        local TEMP_WORK="/tmp/wgp-temp-work/$GAME_ID"
        
        if mountpoint -q "$TEMP_GAME_DIR" 2>/dev/null; then
            echo "Démontage de l'overlay temp existant (rapide)..."
            unmount_overlay "$TEMP_GAME_DIR" -f
        fi
        
        if [ -d "$TEMP_GAME_DIR" ]; then
            rm -rf "$TEMP_GAME_DIR"
        fi
        if [ -d "$TEMP_UPPER" ]; then
            rm -rf "$TEMP_UPPER"
        fi
        if [ -d "$TEMP_WORK" ]; then
            rm -rf "$TEMP_WORK"
        fi
        
        ensure_dirs -s "$TEMP_GAME_DIR" "$TEMP_UPPER" "$TEMP_WORK"
        chmod 777 "/tmp/wgp-temp-upper" "/tmp/wgp-temp-work" "$TEMP_UPPER" "$TEMP_WORK" 2>/dev/null || true
        
        # Créer le symlink dans TEMP_SYMLINK pour le sandbox (comme saves et extras)
        ensure_dir -s "$TEMP_SYMLINK"
        local temp_symlink="$TEMP_SYMLINK/$GAME_INTERNAL_NAME"
        rm -f "$temp_symlink"
        ln -s "$TEMP_GAME_DIR" "$temp_symlink"
        echo "Symlink temp créé: $temp_symlink -> $TEMP_GAME_DIR"
        
        echo "Dossiers overlay préparés (mode temp folders): $GAME_ID"
    fi
}
