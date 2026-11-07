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

    local -a rel_paths=()
    for exe in "${exe_array[@]}"; do
        local rel_path="${exe#$MOUNT_DIR/}"
        rel_paths+=("$rel_path")
    done

    # Choix graphique par Gablue Manager, menu numéroté en terminal — plus de
    # kdialog. `ui_choose` imprime le choix sur stdout.
    local EXE_REL_PATH
    EXE_REL_PATH=$(ui_choose "Choisir un exécutable" \
        "Choisissez un exécutable à lancer :" "${rel_paths[@]}") || true

    if [ -z "$EXE_REL_PATH" ]; then
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
