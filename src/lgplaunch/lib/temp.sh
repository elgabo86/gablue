# Monte l'overlay kernel pour les fichiers temporaires (mode normal - dossiers spécifiques)
prepare_temps() {
    local TEMPPATH_FILE="$MOUNT_DIR/.temppath"
    local TEMP_LGP_DIR="$MOUNT_DIR/.temp"
    local TEMP_GAME_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"
    local GAME_ID="${_GAME_TEMP_ID:-lgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')}"
    local TEMP_UPPER="/tmp/lgp-temp-upper-$UID/$GAME_ID"

    [ -f "$TEMPPATH_FILE" ] || return 0

    if grep -q "^\*$" "$TEMPPATH_FILE" 2>/dev/null; then
        echo "Mode overlay complet détecté (* dans .temppath)"
        prepare_full_overlay
        return 0
    fi

    if [ ! -d "$TEMP_LGP_DIR" ]; then
        echo "Dossier .temp non trouvé dans le LGP, skip overlay"
        return 0
    fi

    echo "Montage de l'overlay kernel pour les fichiers temporaires..."

    if mountpoint -q "$TEMP_GAME_DIR" 2>/dev/null; then
        echo "Démontage de l'overlay existant..."
        unmount_overlay "$TEMP_GAME_DIR"
    fi

    local WORK_DIR="/tmp/lgp-temp-work-$UID/$GAME_ID"

    echo "  lowerdir: $TEMP_LGP_DIR"
    echo "  upperdir: $TEMP_UPPER"
    echo "  workdir: $WORK_DIR"
    echo "  mountpoint: $TEMP_GAME_DIR"

    mount_overlay "$TEMP_LGP_DIR" "$TEMP_UPPER" "$WORK_DIR" "$TEMP_GAME_DIR"

    echo "Overlay kernel enregistré avec succès: $TEMP_GAME_DIR"
}

# Monte un overlay kernel complet sur TOUT le jeu (mode "full overlay" avec * dans .temppath)
prepare_full_overlay() {
    local GAME_ID="lgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')"
    local FULL_OVERLAY_BASE="/tmp/lgp-full-overlay-$UID"
    local MOUNT_OVERLAY="$FULL_OVERLAY_BASE/$GAME_ID"
    local TEMP_UPPER="$FULL_OVERLAY_BASE-upper/$GAME_ID"
    local TEMP_WORK="$FULL_OVERLAY_BASE-work/$GAME_ID"
    
    if mountpoint -q "$MOUNT_OVERLAY" 2>/dev/null; then
        echo "Démontage de l'overlay complet existant..."
        unmount_overlay "$MOUNT_OVERLAY"
    fi
    rm -rf "$MOUNT_OVERLAY" "$TEMP_UPPER" "$TEMP_WORK" 2>/dev/null || true
    
    mkdir -p "$MOUNT_OVERLAY" "$TEMP_UPPER" "$TEMP_WORK"
    
    echo "Montage de l'overlay kernel complet sur tout le jeu..."
    echo "  lowerdir: $MOUNT_DIR (LGP entier)"
    echo "  upperdir: $TEMP_UPPER"
    echo "  workdir: $TEMP_WORK"
    echo "  mountpoint: $MOUNT_OVERLAY"
    
    mount_overlay "$MOUNT_DIR" "$TEMP_UPPER" "$TEMP_WORK" "$MOUNT_OVERLAY"
    
    local REL_EXE="${FULL_EXE_PATH#$MOUNT_DIR/}"
    FULL_EXE_PATH="$MOUNT_OVERLAY/$REL_EXE"
    
    export _FULL_OVERLAY_MOUNT="$MOUNT_OVERLAY"
    export _FULL_OVERLAY_UPPER="$TEMP_UPPER"
    export _FULL_OVERLAY_WORK="$TEMP_WORK"
    
    echo "Overlay kernel complet enregistré: $MOUNT_OVERLAY"
    echo "  Exécutable redirigé: $FULL_EXE_PATH"
}

