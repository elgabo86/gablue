# Prépare les fichiers d'extra depuis .extra vers ~/.cache/lgp
prepare_extras() {
    local EXTRAPATH_FILE="$MOUNT_DIR/.extrapath"
    local EXTRA_LGP_DIR="$MOUNT_DIR/.extra"
    local EXTRA_CACHE_DIR="$EXTRA_REAL/$GAME_INTERNAL_NAME"

    [ -f "$EXTRAPATH_FILE" ] || return 0

    # Supprimer l'ancien EXTRA_DIR s'il existe et n'est pas un symlink
    if [ -d "$EXTRA_DIR" ] && [ ! -L "$EXTRA_DIR" ]; then
        rm -rf "$EXTRA_DIR"
    fi

    # Amorçage PAR ITEM (comme prepare_saves) : un item absent du cache est
    # copié depuis le pack, un item présent est intact. Le cache non vide
    # ne court-circuite PLUS l'amorçage (ancien « tout ou rien » supprimé).

    while IFS= read -r EXTRA_REL_PATH; do
        [ -z "$EXTRA_REL_PATH" ] && continue

        local EXTRA_LGP_ITEM="$EXTRA_LGP_DIR/$EXTRA_REL_PATH"
        local FINAL_EXTRA_ITEM="$EXTRA_CACHE_DIR/$EXTRA_REL_PATH"

        if [ -e "$FINAL_EXTRA_ITEM" ] || [ -L "$FINAL_EXTRA_ITEM" ]; then
            continue
        fi

        if [ -e "$EXTRA_LGP_ITEM" ] || [ -L "$EXTRA_LGP_ITEM" ]; then
            echo "Amorçage de l'extra manquant: $EXTRA_REL_PATH"
            if [ -d "$EXTRA_LGP_ITEM" ]; then
                # Copier récursivement en traitant tous les symlinks
                _copy_dir_with_symlinks "$EXTRA_LGP_ITEM" "$FINAL_EXTRA_ITEM" "$EXTRA_LGP_DIR" "$EXTRA_CACHE_DIR"
            elif [ -L "$EXTRA_LGP_ITEM" ]; then
                mkdir -p "$(dirname "$FINAL_EXTRA_ITEM")"
                _copy_symlink_as_abs "$EXTRA_LGP_ITEM" "$FINAL_EXTRA_ITEM" "$EXTRA_LGP_DIR" "$EXTRA_CACHE_DIR"
            else
                mkdir -p "$(dirname "$FINAL_EXTRA_ITEM")"
                cp -n "$EXTRA_LGP_ITEM" "$FINAL_EXTRA_ITEM"
            fi
        fi
    done < "$EXTRAPATH_FILE"

    mkdir -p "$EXTRA_BASE"
    rm -f "$EXTRA_DIR"
    ln -s "$EXTRA_CACHE_DIR" "$EXTRA_DIR"
}

