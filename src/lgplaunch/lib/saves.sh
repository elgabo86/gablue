# Prépare les sauvegardes depuis UserData
prepare_saves() {
    local SAVE_FILE="$MOUNT_DIR/.savepath"
    local SAVE_LGP_DIR="$MOUNT_DIR/.save"
    local SAVES_DIR="$SAVES_REAL/$GAME_INTERNAL_NAME"

    [ -f "$SAVE_FILE" ] || return 0

    # Amorçage PAR ITEM : un item absent de la destination est copié depuis
    # le pack, un item présent est laissé intact (les données du user
    # gagnent toujours — aucune suppression, aucun écrasement). Remplace
    # l'ancien « tout ou rien » (copie seulement si le dossier racine du
    # jeu est vide), qui laissait des trous quand un pack reconstruit
    # ajoutait ou reclassait un item. Coût en régime stable : un stat par
    # item, négligeable.

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SAVE_LGP_ITEM="$SAVE_LGP_DIR/$SAVE_REL_PATH"
        local FINAL_SAVE_ITEM="$SAVES_DIR/$SAVE_REL_PATH"

        if [ -e "$FINAL_SAVE_ITEM" ] || [ -L "$FINAL_SAVE_ITEM" ]; then
            continue
        fi

        if [ -e "$SAVE_LGP_ITEM" ] || [ -L "$SAVE_LGP_ITEM" ]; then
            echo "Amorçage de la sauvegarde manquante: $SAVE_REL_PATH"
            if [ -d "$SAVE_LGP_ITEM" ]; then
                # Copier récursivement en traitant tous les symlinks
                _copy_dir_with_symlinks "$SAVE_LGP_ITEM" "$FINAL_SAVE_ITEM" "$SAVE_LGP_DIR" "$SAVES_DIR"
            elif [ -L "$SAVE_LGP_ITEM" ]; then
                mkdir -p "$(dirname "$FINAL_SAVE_ITEM")"
                _copy_symlink_as_abs "$SAVE_LGP_ITEM" "$FINAL_SAVE_ITEM" "$SAVE_LGP_DIR" "$SAVES_DIR"
            else
                mkdir -p "$(dirname "$FINAL_SAVE_ITEM")"
                cp -n "$SAVE_LGP_ITEM" "$FINAL_SAVE_ITEM"
            fi
        fi
    done < "$SAVE_FILE"
}

