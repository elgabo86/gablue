#!/bin/bash

################################################################################
# wgp-modes.sh - Modes d'exécution WGP (sélection exécutable, etc.)
################################################################################

# =============================================================================
# Mode interactif de sélection d'exécutable dans un WGP
# =============================================================================

select_exe_from_wgp() {
    echo "Recherche des exécutables dans le pack..."

    local found=0
    local exe_array=()

    while IFS= read -r -d '' exe; do
        exe_array+=("$exe")
        found=$((found + 1))
    done < <(find "$MOUNT_DIR" -type f -iname "*.exe" -print0 | head -z -n 20)

    if [ $found -eq 0 ]; then
        echo "Aucun fichier .exe trouvé dans le pack"
        cleanup_wgp
        exit 1
    fi

    local menu_args=("Choisissez un exécutable à lancer :")

    for exe in "${exe_array[@]}"; do
        local rel_path="${exe#$MOUNT_DIR/}"
        menu_args+=("$rel_path" "$rel_path")
    done

    local EXE_REL_PATH
    if command -v kdialog &> /dev/null; then
        EXE_REL_PATH=$(kdialog --menu "${menu_args[@]}")
    else
        echo ""
        echo "Exécutables disponibles:"
        for i in "${!exe_array[@]}"; do
            local rel_path="${exe_array[$i]#$MOUNT_DIR/}"
            echo "  $((i+1)). $rel_path"
        done
        echo ""
        read -p "Entrez le numéro de l'exécutable: " choice
        if [[ "$choice" =~ ^[0-9]+$ ]] && [ "$choice" -ge 1 ] && [ "$choice" -le ${#exe_array[@]} ]; then
            EXE_REL_PATH="${exe_array[$((choice-1))]#$MOUNT_DIR/}"
        fi
    fi
    
    local exit_status=$?

    if [ $exit_status -ne 0 ] || [ -z "$EXE_REL_PATH" ]; then
        echo "Annulé par l'utilisateur"
        cleanup_wgp
        exit 0
    fi

    FULL_EXE_PATH="$MOUNT_DIR/$EXE_REL_PATH"

    if [ ! -f "$FULL_EXE_PATH" ]; then
        echo "Erreur: exécutable introuvable: $FULL_EXE_PATH"
        cleanup_wgp
        exit 1
    fi

    echo "Exécutable sélectionné: $EXE_REL_PATH"
}
