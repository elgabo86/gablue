#!/bin/bash

################################################################################
# unpacklgp.sh - Script d'extraction de paquets LGP
#
# Ce script permet d'extraire des archives LGP (.lgp) compressées en squashfs,
# en restaurant les sauvegardes depuis ~/.local/share/lgp-saves si elles existent.
################################################################################

#======================================
# Variables globales
#======================================
LGPACK_FILE=""
GAME_NAME=""
GAME_INTERNAL_NAME=""  # Nom du jeu depuis .gamename (identique à celui utilisé dans makelgp)
OUTPUT_DIR=""
TEMP_DIR=""

#======================================
# Fonctions d'affichage et utilitaires
#======================================

# Affiche un message d'erreur et quitte
error_exit() {
    echo "Erreur: $1" >&2
    exit 1
}

# Pose une question oui/non via kdialog ou console
ask_yes_no() {
    local prompt="$1"
    local yes_label="${2:-Oui}"
    local no_label="${3:-Non}"

    if command -v kdialog &> /dev/null; then
        kdialog --yesno "$prompt" --yes-label "$yes_label" --no-label "$no_label"
    else
        read -p "$prompt (o/N): " -r
        [[ "$REPLY" =~ ^[oOyY]$ ]]
    fi
}

#======================================
# Fonctions de validation et confirmation
#======================================

# Vérifie les arguments et initialise les variables
validate_arguments() {
    [ $# -eq 0 ] && error_exit "Usage: $0 <fichier_lgp>"

    LGPACK_FILE="$(realpath "$1")"

    [ ! -f "$LGPACK_FILE" ] && error_exit "Le fichier '$LGPACK_FILE' n'existe pas"

    GAME_NAME="$(basename "$LGPACK_FILE" .lgp)"
    OUTPUT_DIR="./$GAME_NAME"
    TEMP_DIR="$OUTPUT_DIR.tmp"

    echo "=== Extraction du paquet: $GAME_NAME ==="
    echo "Fichier source: $LGPACK_FILE"
    echo "Dossier de sortie: $OUTPUT_DIR"
}

# Demande confirmation avant de commencer
confirm_extraction() {
    local prompt="Voulez-vous extraire le contenu de:\n\n$(basename "$LGPACK_FILE")\n\ndans le dossier:\n$OUTPUT_DIR"

    if ! ask_yes_no "$prompt"; then
        echo "Extraction annulée"
        exit 0
    fi
}

# Gère le cas où le dossier de sortie existe déjà
handle_existing_output() {
    [ -d "$OUTPUT_DIR" ] || return 0

    echo "Attention: le dossier '$OUTPUT_DIR' existe déjà"

    if ask_yes_no "Le dossier existe déjà.\nSon contenu sera écrasé.\n\nContinuer ?" "Oui" "Non"; then
        rm -rf "$OUTPUT_DIR"
    else
        echo "Extraction annulée"
        exit 0
    fi
}

#======================================
# Fonctions d'extraction
#======================================

# Extrait le squashfs avec interface graphique
extract_with_dialog() {
    # Fenêtre d'attente avec bouton Annuler
    kdialog --msgbox "Extraction en cours...\nAppuyez sur Annuler pour arrêter" --ok-label "Annuler" >/dev/null &
    local KDIALOG_PID=$!

    # Lancer unsquashfs en arrière-plan
    unsquashfs -f -d "$TEMP_DIR" -no-xattrs "$LGPACK_FILE" &
    local UNSQUASH_PID=$!

    # Surveiller tant que unsquashfs tourne
    while kill -0 $UNSQUASH_PID 2>/dev/null; do
        # Annulation si kdialog fermé
        if ! kill -0 $KDIALOG_PID 2>/dev/null; then
            kill -9 $UNSQUASH_PID 2>/dev/null
            pkill -9 unsquashfs 2>/dev/null
            rm -rf "$TEMP_DIR"
            echo ""
            echo "Extraction annulée"
            exit 0
        fi
        sleep 0.2
    done

    # Fermer kdialog
    kill $KDIALOG_PID 2>/dev/null

    # Vérifier le code de retour
    wait $UNSQUASH_PID
}

# Extrait le squashfs en mode console
extract_console() {
    echo "Extraction de $LGPACK_FILE..."
    unsquashfs -f -d "$TEMP_DIR" -no-xattrs "$LGPACK_FILE"
}

# Lance l'extraction
extract_lgp() {
    echo ""
    echo "=== Extraction en cours ==="

    local EXIT_CODE

    if command -v kdialog &> /dev/null; then
        extract_with_dialog
        EXIT_CODE=$?
    else
        extract_console
        EXIT_CODE=$?
    fi

    if [ $EXIT_CODE -ne 0 ]; then
        echo "Erreur lors de l'extraction"
        rm -rf "$TEMP_DIR"
        exit 1
    fi

    # Renommer le dossier temporaire en dossier final
    mv "$TEMP_DIR" "$OUTPUT_DIR"

    # Lire le fichier .gamename pour obtenir le nom interne du jeu
    local GAMENAME_FILE="$OUTPUT_DIR/.gamename"
    if [ -f "$GAMENAME_FILE" ]; then
        GAME_INTERNAL_NAME=$(cat "$GAMENAME_FILE")
    else
        # Fallback: utiliser le nom du dossier extrait
        GAME_INTERNAL_NAME="$GAME_NAME"
    fi
}

#======================================
# Fonctions de gestion des sauvegardes
#======================================

# Restaure une sauvegarde (fichier ou dossier) depuis .save/
restore_save_item() {
    local SAVE_REL_PATH="$1"
    local ITEM_NAME=$(basename "$SAVE_REL_PATH")

    local OUTPUT_ITEM="$OUTPUT_DIR/$SAVE_REL_PATH"
    local LGP_SAVE_ITEM="$OUTPUT_DIR/.save/$SAVE_REL_PATH"

    # Déterminer le type (fichier ou dossier) depuis .save/
    local item_type=""
    if [ -f "$LGP_SAVE_ITEM" ]; then
        item_type="file"
    elif [ -d "$LGP_SAVE_ITEM" ]; then
        item_type="dir"
    else
        echo ""
        echo "Avertissement: l'élément n'existe pas dans .save/: $SAVE_REL_PATH"
        return
    fi

    # Toujours utiliser .save/ du LGP
    echo ""
    echo "Copie de $ITEM_NAME depuis LGP..."

    mkdir -p "$(dirname "$OUTPUT_ITEM")"

    if [ "$item_type" = "file" ]; then
        rm -f "$OUTPUT_ITEM" 2>/dev/null
        cp "$LGP_SAVE_ITEM" "$OUTPUT_ITEM"
        echo "Fichier de sauvegardes copié avec succès."
    else
        rm -rf "$OUTPUT_ITEM" 2>/dev/null
        cp -a "$LGP_SAVE_ITEM"/. "$OUTPUT_ITEM/"
        echo "Dossier de sauvegardes copié avec succès."
    fi
}

# Parcourt .savepath et restaure toutes les sauvegardes depuis LGP
restore_all_saves() {
    local SAVE_FILE="$OUTPUT_DIR/.savepath"
    [ -f "$SAVE_FILE" ] || return 0

    echo ""
    echo "=== Restitution des sauvegardes ==="

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] || restore_save_item "$SAVE_REL_PATH"
    done < "$SAVE_FILE"
}

#======================================
# Fonctions de gestion des fichiers temporaires
#======================================

# Copie un fichier temporaire (fichier ou dossier) depuis .temp vers son emplacement d'origine
copy_temp_item() {
    local TEMP_REL_PATH="$1"
    local ITEM_NAME=$(basename "$TEMP_REL_PATH")

    local OUTPUT_ITEM="$OUTPUT_DIR/$TEMP_REL_PATH"
    local LGP_TEMP_ITEM="$OUTPUT_DIR/.temp/$TEMP_REL_PATH"

    if [ -f "$LGP_TEMP_ITEM" ]; then
        echo ""
        echo "Copie du fichier temporaire ($ITEM_NAME)..."
        mkdir -p "$(dirname "$OUTPUT_ITEM")"
        rm -f "$OUTPUT_ITEM" 2>/dev/null
        cp "$LGP_TEMP_ITEM" "$OUTPUT_ITEM"
        echo "Fichier temporaire copié avec succès."
    elif [ -d "$LGP_TEMP_ITEM" ]; then
        echo ""
        echo "Copie du dossier temporaire ($ITEM_NAME)..."
        mkdir -p "$(dirname "$OUTPUT_ITEM")"
        rm -rf "$OUTPUT_ITEM" 2>/dev/null
        cp -a "$LGP_TEMP_ITEM"/ "$OUTPUT_ITEM/"
        echo "Dossier temporaire copié avec succès."
    else
        echo ""
        echo "Avertissement: l'élément temporaire n'existe pas dans le LGP: $LGP_TEMP_ITEM"
    fi
}

# Parcourt .temppath et restaure tous les fichiers temporaires à leur emplacement d'origine
copy_all_temps() {
    local TEMPPATH_FILE="$OUTPUT_DIR/.temppath"
    [ -f "$TEMPPATH_FILE" ] || return 0

    echo ""
    echo "=== Restauration des fichiers temporaires ==="

    while IFS= read -r TEMP_REL_PATH; do
        [ -z "$TEMP_REL_PATH" ] || copy_temp_item "$TEMP_REL_PATH"
    done < "$TEMPPATH_FILE"
}

#======================================
# Fonctions de restauration des symlinks
#======================================

# Restaure les symlinks depuis le fichier .symlinks_backup
restore_symlinks() {
    local SYMLINKS_FILE="$OUTPUT_DIR/.symlinks_backup"
    [ -f "$SYMLINKS_FILE" ] || return 0

    echo ""
    echo "=== Restauration des symlinks ==="

    while IFS='|' read -r rel_path target; do
        [ -z "$rel_path" ] && continue

        local source_path="$OUTPUT_DIR/$rel_path"

        # Ne pas restaurer si c'est un fichier de sauvegarde ou extra (géré ailleurs)
        if [[ "$rel_path" == .save/* ]] || [[ "$rel_path" == .extra/* ]] || [[ "$rel_path" == .temp/* ]]; then
            continue
        fi

        echo "Restauration du symlink: $rel_path -> $target"

        # Créer le dossier parent si nécessaire
        mkdir -p "$(dirname "$source_path")"

        # Supprimer l'ancien fichier/dossier si présent
        rm -rf "$source_path" 2>/dev/null

        # Créer le symlink
        ln -s "$target" "$source_path"
        echo "Symlink restauré avec succès."
    done < "$SYMLINKS_FILE"
}

#======================================
# Fonctions de gestion des extras
#======================================

# Copie un extra (fichier ou dossier) depuis .extra vers l'emplacement final
copy_extra_item() {
    local EXTRA_REL_PATH="$1"
    local ITEM_NAME=$(basename "$EXTRA_REL_PATH")

    local OUTPUT_ITEM="$OUTPUT_DIR/$EXTRA_REL_PATH"
    local LGP_ITEM="$OUTPUT_DIR/.extra/$EXTRA_REL_PATH"

    if [ -f "$LGP_ITEM" ]; then
        echo ""
        echo "Copie du fichier d'extra ($ITEM_NAME)..."
        mkdir -p "$(dirname "$OUTPUT_ITEM")"
        # Supprimer le symlink si présent
        rm -f "$OUTPUT_ITEM" 2>/dev/null
        cp "$LGP_ITEM" "$OUTPUT_ITEM"
        echo "Fichier d'extra copié avec succès."
    elif [ -d "$LGP_ITEM" ]; then
        echo ""
        echo "Copie du dossier d'extra ($ITEM_NAME)..."
        mkdir -p "$(dirname "$OUTPUT_ITEM")"
        # Supprimer le symlink si présent avant la copie
        rm -rf "$OUTPUT_ITEM" 2>/dev/null
        cp -a "$LGP_ITEM"/. "$OUTPUT_ITEM/"
        echo "Dossier d'extra copié avec succès."
    else
        echo ""
        echo "Avertissement: l'élément d'extra n'existe pas dans le LGP: $LGP_ITEM"
    fi
}

# Parcourt .extrapath et copie tous les extras
copy_all_extras() {
    local EXTRAPATH_FILE="$OUTPUT_DIR/.extrapath"
    [ -f "$EXTRAPATH_FILE" ] || return 0

    echo ""
    echo "=== Copie des extras depuis .extra ==="

    while IFS= read -r EXTRA_REL_PATH; do
        [ -z "$EXTRA_REL_PATH" ] || copy_extra_item "$EXTRA_REL_PATH"
    done < "$EXTRAPATH_FILE"
}

#======================================
# Fonctions de nettoyage
#======================================

# Supprime les fichiers temporaires du LGP
cleanup_lgp_files() {
    rm -rf "$OUTPUT_DIR/.extra"
    rm -rf "$OUTPUT_DIR/.save"
    rm -rf "$OUTPUT_DIR/.temp"
    rm -f "$OUTPUT_DIR/.symlinks_backup"
}

#======================================
# Fonctions d'affichage final
#======================================

# Affiche le résumé d'extraction
show_summary() {
    local FILE_COUNT=$(find "$OUTPUT_DIR" -type f | wc -l)

    echo ""
    echo "=== Extraction terminée avec succès ==="
    echo "Dossier: $OUTPUT_DIR"
    echo "Fichiers extraits: $FILE_COUNT"

    # Fenêtre de succès KDE
    if command -v kdialog &> /dev/null; then
        local MSG="Paquet extrait avec succès !\n\n"
        MSG+="Fichier: $(basename "$LGPACK_FILE")\n"
        MSG+="Dossier: $OUTPUT_DIR\n"
        MSG+="Fichiers: $FILE_COUNT"

        kdialog --title "Succès" --msgbox "$MSG"
    fi
}

#======================================
# Fonction principale
#======================================

main() {
    # Validation
    validate_arguments "$@"

    # Confirmations
    confirm_extraction
    handle_existing_output

    # Extraction
    extract_lgp

    # Restauration des fichiers temporaires
    copy_all_temps

    # Restauration des sauvegardes depuis LGP
    restore_all_saves

    # Restauration des symlinks
    restore_symlinks

    # Copie des extras
    copy_all_extras

    # Nettoyage
    cleanup_lgp_files

    # Résumé
    show_summary
}

# Lancement du script
main "$@"
