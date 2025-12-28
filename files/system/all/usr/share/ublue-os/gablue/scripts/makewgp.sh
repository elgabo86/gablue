#!/bin/bash

################################################################################
# makewgp.sh - Script de création de paquets WGP (Windows Game Packs)
#
# Ce script permet de créer des archives compressées (squashfs) contenant un jeu
# Windows avec ses métadonnées pour une portabilité facilitée.
################################################################################

#======================================
# Variables globales
#======================================
GAME_DIR=""
GAME_NAME=""
WGPACK_NAME=""
COMPRESS_CMD=""
SUFFIX=""

# Chemins des fichiers de configuration
LAUNCH_FILE=""
ARGS_FILE=""
FIX_FILE=""
SAVE_FILE=""
EXTRAPATH_FILE=""

# Répertoires temporaires dans le jeu
SAVE_WGP_DIR=""
EXTRA_WGP_DIR=""

#======================================
# Fonctions d'affichage et utilitaires
#======================================

# Affiche un message d'erreur et quitte
error_exit() {
    echo "Erreur: $1" >&2
    exit 1
}

# Ouvre un dialogue kdialog ou pose une question en console
kdialog_or_input() {
    local prompt="$1"
    local default="$2"

    if command -v kdialog &> /dev/null; then
        kdialog --inputbox "$prompt" "$default"
    else
        echo "$prompt"
        read -r
        echo "${REPLY:-$default}"
    fi
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

# Sélectionne un élément dans une liste via kdialog ou console
select_from_list() {
    local title="$1"
    shift
    local items=("$@")

    if command -v kdialog &> /dev/null; then
        kdialog --radiolist "$title" "${items[@]}"
    else
        local i=0
        echo "$title:"
        while [ $i -lt ${#items[@]} ]; do
            echo "  $((i/3+1)). ${items[$i+1]}"
            i=$((i + 3))
        done
        read -p "Entrez le numéro: " -r
        local idx=$((REPLY - 1))
        echo "${items[$((idx * 3))]}"
    fi
}

# Sélectionne un répertoire via kdialog ou console
select_directory() {
    local base_dir="$1"

    if command -v kdialog &> /dev/null; then
        kdialog --getexistingdirectory "$base_dir"
    else
        echo "Dossiers disponibles dans $base_dir:"
        local dirs=()
        while IFS= read -r dir; do
            dirs+=("$dir")
            echo "  ${#dirs[@]}. $(basename "$dir")"
        done < <(find "$base_dir" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort)
        read -p "Entrez le numéro (0 pour annuler): " -r
        if [ "$REPLY" -gt 0 ] 2>/dev/null; then
            echo "${dirs[$((REPLY-1))]}"
        fi
    fi
}

# Sélectionne un fichier via kdialog ou console
select_file() {
    local base_dir="$1"

    if command -v kdialog &> /dev/null; then
        kdialog --getopenfilename "$base_dir" "Tous les fichiers (*)"
    else
        read -p "Entrez le chemin relatif (depuis $base_dir): " -r
        if [ -n "$REPLY" ]; then
            echo "$base_dir/$REPLY"
        fi
    fi
}

#======================================
# Fonctions de configuration
#======================================

# Configure le niveau de compression
configure_compression() {
    local DEFAULT_LEVEL=15

    echo ""
    echo "=== Configuration de la compression ==="

    local INPUT
    if command -v kdialog &> /dev/null; then
        INPUT=$(kdialog --inputbox "Niveau de compression zstd (1-19):\n1 = le plus rapide à lire\n19 = la plus petite taille\n0 = pas de compression" "$DEFAULT_LEVEL")
    else
        echo "Niveau de compression zstd (1-19):"
        echo "  1 = le plus rapide à lire"
        echo "  19 = la plus petite taille"
        echo "  0 = pas de compression"
        read -r
        INPUT="${REPLY:-$DEFAULT_LEVEL}"
    fi

    case "$INPUT" in
        "0"|"none"|"non")
            COMPRESS_CMD=""
            SUFFIX=""
            ;;
        [1-9]|1[0-9])
            COMPRESS_CMD="-comp zstd -Xcompression-level $INPUT"
            SUFFIX="_zstd$INPUT"
            ;;
        *)
            error_exit "Choix invalide"
            ;;
    esac
}

# Sélectionne l'exécutable principal
select_executable() {
    echo ""
    echo "=== Sélection de l'exécutable ==="

    local EXE_LIST
    EXE_LIST=$(find "$GAME_DIR" \( -type f -o -type l \) \( -iname "*.exe" -o -iname "*.bat" \) 2>/dev/null)

    [ -z "$EXE_LIST" ] && error_exit "Aucun fichier .exe ou .bat trouvé dans $GAME_DIR"

    local EXE_ARRAY=()
    local COUNT=0
    while IFS= read -r exe; do
        local REL_PATH="${exe#$GAME_DIR/}"
        EXE_ARRAY+=("$exe" "$REL_PATH")
        [ $COUNT -eq 0 ] && EXE_ARRAY+=("on") || EXE_ARRAY+=("off")
        COUNT=$((COUNT + 1))
    done <<< "$EXE_LIST"

    local SELECTED
    SELECTED=$(select_from_list "Sélectionnez l'exécutable principal:" "${EXE_ARRAY[@]}")

    [ ! -f "$SELECTED" ] && error_exit "Exécutable non valide"

    echo ""
    echo "Exécutable sélectionné: ${SELECTED#$GAME_DIR/}"

    # Créer le fichier .launch
    mkdir -p "$(dirname "$LAUNCH_FILE")"
    echo "${SELECTED#$GAME_DIR/}" > "$LAUNCH_FILE"
}

# Configure les arguments de lancement
configure_arguments() {
    local BOTTLE_ARGS=""

    if ask_yes_no "Voulez-vous ajouter des arguments de lancement ?\n(ex: --dx12, --window, --no-borders, etc.)"; then
        if command -v kdialog &> /dev/null; then
            BOTTLE_ARGS=$(kdialog --inputbox "Entrez les arguments de lancement")
        else
            read -p "Entrez les arguments: " -r
            BOTTLE_ARGS="$REPLY"
        fi
    fi

    if [ -n "$BOTTLE_ARGS" ]; then
        mkdir -p "$(dirname "$ARGS_FILE")"
        echo "$BOTTLE_ARGS" > "$ARGS_FILE"
        echo "Arguments: $BOTTLE_ARGS"
    fi
}

# Configure le fix manette
configure_fix() {
    if ask_yes_no "Voulez-vous activer le fix manette pour ce jeu ?\n\nCela modifie une clé de registre Wine (DisableHidraw)\npour résoudre des problèmes de compatibilité manette."; then
        mkdir -p "$(dirname "$FIX_FILE")"
        touch "$FIX_FILE"
        echo "Fix manette activé"
    fi
}

#======================================
# Fonctions de gestion des sauvegardes
#======================================

# Demande et configure un dossier/fichier de sauvegarde
add_save_item() {
    ask_yes_no "Dossier/fichier de sauvegarde à gérer ?\n\nSauvegardes persistantes stockées dans UserData.\nUne copie reste dans le paquet pour la portabilité." || return 1

    # Type de sauvegarde
    local SAVE_TYPE
    if command -v kdialog &> /dev/null; then
        SAVE_TYPE=$(kdialog --radiolist "Que voulez-vous conserver ?" "dir" "Dossier" "on" "file" "Fichier" "off")
    else
        read -p "Type [d]ossier ou [f]ichier ? (D/f): " -r
        [[ "$REPLY" =~ ^[fF]$ ]] && SAVE_TYPE="file" || SAVE_TYPE="dir"
    fi

    # Sélection de l'élément
    local SELECTED_ITEM=""
    if [ "$SAVE_TYPE" = "dir" ]; then
        SELECTED_ITEM=$(select_directory "$GAME_DIR")
    else
        SELECTED_ITEM=$(select_file "$GAME_DIR")
    fi

    [ -z "$SELECTED_ITEM" ] && return 1

    # Validation et traitement
    local SAVE_REL_PATH="${SELECTED_ITEM#$GAME_DIR/}"
    if [[ "$SELECTED_ITEM" != "$GAME_DIR/"* ]]; then
        error_exit "L'élément doit être dans le dossier du jeu: $GAME_DIR"
    fi

    [ "$SAVE_TYPE" = "dir" ] && [ ! -d "$SELECTED_ITEM" ] && error_exit "Le dossier n'existe pas"
    [ "$SAVE_TYPE" = "file" ] && [ ! -f "$SELECTED_ITEM" ] && error_exit "Le fichier n'existe pas"

    process_save_item "$SAVE_TYPE" "$SELECTED_ITEM" "$SAVE_REL_PATH"
}

# Traite un élément de sauvegarde (crée .save, .savepath, symlink)
process_save_item() {
    local SAVE_TYPE="$1"
    local SAVE_ITEM_ABSOLUTE="$2"
    local SAVE_REL_PATH="$3"

    echo ""
    echo "Élément de sauvegarde: $SAVE_REL_PATH"

    # Chemins externes
    local WINDOWS_HOME="$HOME/Windows/UserData"
    local SAVES_BASE="$WINDOWS_HOME/$USER/LocalSavesWGP"
    local SAVES_DIR="$SAVES_BASE/$GAME_NAME"

    # Créer .save dans le WGP
    local SAVE_WGP_ITEM="$GAME_DIR/.save/$SAVE_REL_PATH"
    mkdir -p "$(dirname "$SAVE_WGP_ITEM")"

    # Copier vers .save (portabilité)
    echo "Copie vers .save pour portabilité..."
    if [ "$SAVE_TYPE" = "dir" ]; then
        cp -a "$SAVE_ITEM_ABSOLUTE"/. "$SAVE_WGP_ITEM"/
    else
        cp "$SAVE_ITEM_ABSOLUTE" "$SAVE_WGP_ITEM"
    fi

    # Créer symlink vers UserData
    echo "Création du symlink vers UserData..."
    if [ "$SAVE_TYPE" = "dir" ]; then
        rm -rf "$SAVE_ITEM_ABSOLUTE"
        ln -s "$SAVES_DIR/$SAVE_REL_PATH" "$SAVE_ITEM_ABSOLUTE"
    else
        rm -f "$SAVE_ITEM_ABSOLUTE"
        ln -s "$SAVES_DIR/$SAVE_REL_PATH" "$SAVE_ITEM_ABSOLUTE"
    fi

    # Mettre à jour .savepath
    mkdir -p "$(dirname "$SAVE_FILE")"
    echo "$SAVE_REL_PATH" >> "$SAVE_FILE"
}

# Configure tous les éléments de sauvegarde
configure_saves() {
    echo ""
    echo "=== Gestion des sauvegardes ==="

    while true; do
        add_save_item || break
    done
}

#======================================
# Fonctions de gestion des extras
#======================================

# Demande et configure un fichier/dossier d'extra
add_extra_item() {
    ask_yes_no "Fichier/dossier d'extra à conserver ?\n\nLes extras seront stockés dans /tmp/wgp-extra\n\nCe sont des fichiers temporaires (config, cache...)\nperdus après la fermeture du jeu.\nLe WGP utilisera un symlink vers /tmp." || return 1

    # Type d'extra
    local EXTRA_TYPE
    if command -v kdialog &> /dev/null; then
        EXTRA_TYPE=$(kdialog --radiolist "Que voulez-vous conserver ?" "file" "Fichier" "on" "dir" "Dossier" "off")
    else
        read -p "Type [f]ichier ou [d]ossier ? (f/D): " -r
        [[ "$REPLY" =~ ^[fF]$ ]] && EXTRA_TYPE="file" || EXTRA_TYPE="dir"
    fi

    # Sélection de l'élément
    local SELECTED_ITEM=""
    if [ "$EXTRA_TYPE" = "dir" ]; then
        SELECTED_ITEM=$(select_directory "$GAME_DIR")
    else
        SELECTED_ITEM=$(select_file "$GAME_DIR")
    fi

    [ -z "$SELECTED_ITEM" ] && return 1

    # Validation et traitement
    local EXTRA_REL_PATH="${SELECTED_ITEM#$GAME_DIR/}"
    if [[ "$SELECTED_ITEM" != "$GAME_DIR/"* ]]; then
        error_exit "L'élément doit être dans le dossier du jeu: $GAME_DIR"
    fi

    [ "$EXTRA_TYPE" = "dir" ] && [ ! -d "$SELECTED_ITEM" ] && error_exit "Le dossier n'existe pas"
    [ "$EXTRA_TYPE" = "file" ] && [ ! -f "$SELECTED_ITEM" ] && error_exit "Le fichier n'existe pas"

    process_extra_item "$EXTRA_TYPE" "$SELECTED_ITEM" "$EXTRA_REL_PATH"
}

# Traite un élément d'extra (crée .extra, .extrapath, symlink)
process_extra_item() {
    local EXTRA_TYPE="$1"
    local EXTRA_ITEM_ABSOLUTE="$2"
    local EXTRA_REL_PATH="$3"

    echo ""
    echo "Élément d'extra: $EXTRA_REL_PATH"

    # Chemin externe
    local EXTRA_BASE="/tmp/wgp-extra"
    local EXTRA_DIR="$EXTRA_BASE/$GAME_NAME"

    # Créer .extra dans le WGP
    local EXTRA_WGP_ITEM="$GAME_DIR/.extra/$EXTRA_REL_PATH"
    mkdir -p "$(dirname "$EXTRA_WGP_ITEM")"

    # Copier vers .extra (portabilité)
    echo "Copie vers .extra pour portabilité..."
    if [ "$EXTRA_TYPE" = "dir" ]; then
        cp -a "$EXTRA_ITEM_ABSOLUTE"/. "$EXTRA_WGP_ITEM"/
    else
        cp "$EXTRA_ITEM_ABSOLUTE" "$EXTRA_WGP_ITEM"
    fi

    # Créer symlink vers /tmp
    echo "Création du symlink vers /tmp/wgp-extra..."
    if [ "$EXTRA_TYPE" = "dir" ]; then
        rm -rf "$EXTRA_ITEM_ABSOLUTE"
        ln -s "$EXTRA_DIR/$EXTRA_REL_PATH" "$EXTRA_ITEM_ABSOLUTE"
    else
        rm -f "$EXTRA_ITEM_ABSOLUTE"
        ln -s "$EXTRA_DIR/$EXTRA_REL_PATH" "$EXTRA_ITEM_ABSOLUTE"
    fi

    # Mettre à jour .extrapath
    mkdir -p "$(dirname "$EXTRAPATH_FILE")"
    echo "$EXTRA_REL_PATH" >> "$EXTRAPATH_FILE"
}

# Configure tous les éléments d'extra
configure_extras() {
    echo ""
    echo "=== Gestion des extras ==="

    while true; do
        add_extra_item || break
    done
}

#======================================
# Fonctions de restauration
#======================================

# Restaure un élément depuis .save ou .extra
restore_item() {
    local ITEM_TYPE="$1"   # "save" ou "extra"
    local ITEM_REL_PATH="$2"
    local ORIGIN_DIR="$3"  # ".save" ou ".extra"

    local ORIGINAL_ITEM="$GAME_DIR/$ITEM_REL_PATH"
    local WGP_ITEM="${GAME_DIR}/${ORIGIN_DIR}/${ITEM_REL_PATH}"

    if [ ! -e "$WGP_ITEM" ]; then
        return
    fi

    # Supprimer le symlink existant
    rm -rf "$ORIGINAL_ITEM"

    # Copier depuis le WGP
    if [ -d "$WGP_ITEM" ]; then
        cp -a "$WGP_ITEM"/. "$ORIGINAL_ITEM"/
        echo "$ITEM_TYPE restauré: $ITEM_REL_PATH"
    elif [ -f "$WGP_ITEM" ]; then
        cp "$WGP_ITEM" "$ORIGINAL_ITEM"
        echo "$ITEM_TYPE restauré: $ITEM_REL_PATH"
    fi
}

# Restaure toutes les sauvegardes depuis UserData
restore_all_saves() {
    [ -f "$SAVE_FILE" ] || return 0

    echo ""
    echo "=== Restitution des sauvegardes ==="

    local WINDOWS_HOME="$HOME/Windows/UserData"
    local SAVES_DIR="$WINDOWS_HOME/$USER/LocalSavesWGP/$GAME_NAME"

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local ORIGINAL_ITEM="$GAME_DIR/$SAVE_REL_PATH"
        local SAVE_WGP_ITEM="$GAME_DIR/.save/$SAVE_REL_PATH"
        local FINAL_SAVE_ITEM="$SAVES_DIR/$SAVE_REL_PATH"

        # Supprimer le symlink
        rm -rf "$ORIGINAL_ITEM"

        # Copier depuis .save
        if [ -d "$SAVE_WGP_ITEM" ]; then
            cp -a "$SAVE_WGP_ITEM"/. "$ORIGINAL_ITEM"/
            echo "Save restaurée: $SAVE_REL_PATH"
        elif [ -f "$SAVE_WGP_ITEM" ]; then
            cp "$SAVE_WGP_ITEM" "$ORIGINAL_ITEM"
            echo "Save restaurée: $SAVE_REL_PATH"
        fi
    done < "$SAVE_FILE"
}

# Restaure tous les extras depuis .extra
restore_all_extras() {
    [ -f "$EXTRAPATH_FILE" ] || return 0

    echo ""
    echo "=== Restitution des extras ==="

    while IFS= read -r EXTRA_REL_PATH; do
        [ -z "$EXTRA_REL_PATH" ] && continue
        restore_item "Extra" "$EXTRA_REL_PATH" ".extra"
    done < "$EXTRAPATH_FILE"
}

#======================================
# Fonctions de nettoyage
#======================================

# Nettoie tous les fichiers temporaires
cleanup_temp_files() {
    rm -f "$LAUNCH_FILE" 2>/dev/null
    [ -f "$ARGS_FILE" ] && rm -f "$ARGS_FILE" 2>/dev/null
    [ -f "$FIX_FILE" ] && rm -f "$FIX_FILE" 2>/dev/null
    [ -f "$SAVE_FILE" ] && rm -f "$SAVE_FILE" 2>/dev/null
    [ -d "$GAME_DIR/.save" ] && rm -rf "$GAME_DIR/.save" 2>/dev/null
    [ -f "$EXTRAPATH_FILE" ] && rm -f "$EXTRAPATH_FILE" 2>/dev/null
    [ -d "$GAME_DIR/.extra" ] && rm -rf "$GAME_DIR/.extra" 2>/dev/null
    [ -f "$GAME_DIR/.gamename" ] && rm -f "$GAME_DIR/.gamename" 2>/dev/null
}

# Restaure les fichiers originaux et nettoie les temporaires
full_restore() {
    restore_all_saves
    restore_all_extras
    cleanup_temp_files
}

#======================================
# Fonctions d'affichage final
#======================================

# Affiche le résumé des choix avant création avec confirmation
show_summary_before_build() {
    local EXE_REL_PATH=$(cat "$LAUNCH_FILE")
    local BOTTLE_ARGS=""
    [ -f "$ARGS_FILE" ] && BOTTLE_ARGS=$(cat "$ARGS_FILE")
    local FIX_ENABLED=""
    [ -f "$FIX_FILE" ] && FIX_ENABLED="Oui"

    # Compression
    local COMP_INFO="sans compression"
    if [ -n "$COMPRESS_CMD" ]; then
        COMP_INFO="zstd $(echo "$COMPRESS_CMD" | grep -o '[0-9]\+' | head -1)"
    fi

    # Sauvegardes
    local SAVES_LIST=""
    if [ -f "$SAVE_FILE" ]; then
        local SAVE_COUNT=$(wc -l < "$SAVE_FILE")
        SAVES_LIST="$SAVE_COUNT sauvegarde(s)"
    else
        SAVES_LIST="Aucune"
    fi

    # Extras
    local EXTRAS_LIST=""
    if [ -f "$EXTRAPATH_FILE" ]; then
        local EXTRA_COUNT=$(wc -l < "$EXTRAPATH_FILE")
        EXTRAS_LIST="$EXTRA_COUNT extra(s)"
    else
        EXTRAS_LIST="Aucun"
    fi

    local MSG="=== Résumé de la création du WGP ===

Jeu: $GAME_NAME
Dossier: $GAME_DIR
Fichier de sortie: $WGPACK_NAME

Exécutable: $EXE_REL_PATH
Arguments: ${BOTTLE_ARGS:-Aucun}
Fix manette: ${FIX_ENABLED:-Non}
Compression: $COMP_INFO
Sauvegardes: $SAVES_LIST
Extras: $EXTRAS_LIST

Voulez-vous continuer la création ?"

    if command -v kdialog &> /dev/null; then
        # kdialog gère les sauts de ligne dans son propre message
        :
    else
        # Affichage console compact
        echo ""
        echo "$MSG" | tr '\n' ' '
        echo ""
    fi

    if ! ask_yes_no "$MSG"; then
        echo ""
        echo "Annulation demandée."
        full_restore
        exit 0
    fi
}

#======================================
# Fonctions de création du squashfs
#======================================

# Vérifie si le fichier existe déjà et demande confirmation
check_existing_wgp() {
    [ ! -f "$WGPACK_NAME" ] && return 0

    if ask_yes_no "Le fichier $WGPACK_NAME existe déjà.\nVoulez-vous l'écraser ?"; then
        echo "Suppression de l'ancien fichier: $WGPACK_NAME"
        rm -f "$WGPACK_NAME"
        return 0
    fi

    # Annulation: restaurer et quitter
    full_restore
    exit 0
}

# Crée le squashfs avec annulation possible
create_squashfs() {
    echo ""
    echo "=== Création du squashfs ==="

    if command -v kdialog &> /dev/null; then
        create_squashfs_with_dialog
    else
        create_squashfs_console
    fi

    return $?
}

# Crée le squashfs avec interface graphique (annulation possible)
create_squashfs_with_dialog() {
    # Fenêtre informative
    kdialog --msgbox "Compression en cours...\nAppuyez sur Annuler pour arrêter" --ok-label "Annuler" >/dev/null &
    local KDIALOG_PID=$!

    # Lancer mksquashfs en arrière-plan
    mksquashfs "$GAME_DIR" "$WGPACK_NAME" $COMPRESS_CMD -all-root &
    local MKSQUASH_PID=$!

    # Surveiller jusqu'à ce que mksquashfs se termine
    while kill -0 $MKSQUASH_PID 2>/dev/null; do
        # Annulation si kdialog fermé
        if ! kill -0 $KDIALOG_PID 2>/dev/null; then
            kill -9 $MKSQUASH_PID 2>/dev/null
            pkill -9 mksquashfs 2>/dev/null
            rm -f "$WGPACK_NAME"
            echo ""
            echo "Compression annulée."
            full_restore
            exit 0
        fi
        sleep 0.2
    done

    # Fermer kdialog
    kill $KDIALOG_PID 2>/dev/null

    # Vérifier le code de retour
    wait $MKSQUASH_PID
    local EXIT_CODE=$?

    if [ $EXIT_CODE -ne 0 ]; then
        echo "Erreur lors de la création du squashfs"
        full_restore
        exit 1
    fi
}

# Crée le squashfs en mode console
create_squashfs_console() {
    echo "Création de $WGPACK_NAME en cours..."
    mksquashfs "$GAME_DIR" "$WGPACK_NAME" $COMPRESS_CMD -nopad

    if [ $? -ne 0 ]; then
        echo "Erreur lors de la création du squashfs"
        full_restore
        exit 1
    fi
}

#======================================
# Fonctions d'affichage final
#======================================

# Affiche le résumé de création du paquet
show_summary() {
    local EXE_REL_PATH="$1"
    local BOTTLE_ARGS="$2"

    # Calcul des tailles
    local SIZE_BEFORE=$(du -s "$GAME_DIR" | cut -f1)
    local SIZE_BEFORE_GB=$(echo "scale=2; $SIZE_BEFORE / 1024 / 1024" | bc)
    local SIZE_AFTER=$(du -s "$WGPACK_NAME" | cut -f1)
    local SIZE_AFTER_GB=$(echo "scale=2; $SIZE_AFTER / 1024 / 1024" | bc)
    local COMPRESSION_RATIO=$(echo "scale=1; (1 - $SIZE_AFTER / $SIZE_BEFORE) * 100" | bc)

    echo ""
    echo "=== Paquet créé avec succès ==="
    echo "Fichier: $WGPACK_NAME"
    echo "Taille avant: ${SIZE_BEFORE_GB} Go"
    echo "Taille après: ${SIZE_AFTER_GB} Go"
    echo "Gain: ${COMPRESSION_RATIO}%"
    echo "Exécutable: $EXE_REL_PATH"
    [ -n "$BOTTLE_ARGS" ] && echo "Arguments: $BOTTLE_ARGS"

    # Fenêtre de succès KDE
    if command -v kdialog &> /dev/null; then
        local MSG="Paquet créé avec succès !\n\n"
        MSG+="Fichier: $WGPACK_NAME\n"
        MSG+="Taille avant: ${SIZE_BEFORE_GB} Go\n"
        MSG+="Taille après: ${SIZE_AFTER_GB} Go\n"
        MSG+="Gain: ${COMPRESSION_RATIO}%\n\n"
        MSG+="Exécutable: $EXE_REL_PATH"
        [ -n "$BOTTLE_ARGS" ] && MSG+="\nArguments: $BOTTLE_ARGS"

        kdialog --title "Succès" --msgbox "$MSG"
    fi
}

#======================================
# Fonction principale
#======================================

main() {
    # Vérification des arguments
    [ $# -eq 0 ] && error_exit "Usage: $0 <dossier_du_jeu>"

    GAME_DIR="$(realpath "$1")"
    [ ! -d "$GAME_DIR" ] && error_exit "Le dossier '$GAME_DIR' n'existe pas"

    # Initialisation des variables
    GAME_NAME="$(basename "$GAME_DIR")"
    WGPACK_NAME="$(dirname "$GAME_DIR")/${GAME_NAME}.wgp"

    # Chemins des fichiers de configuration
    LAUNCH_FILE="$GAME_DIR/.launch"
    ARGS_FILE="$GAME_DIR/.args"
    FIX_FILE="$GAME_DIR/.fix"
    SAVE_FILE="$GAME_DIR/.savepath"
    EXTRAPATH_FILE="$GAME_DIR/.extrapath"

    echo "=== Création du paquet pour: $GAME_NAME ==="
    echo "Dossier source: $GAME_DIR"

    # Nettoyage initial
    echo ""
    echo "Nettoyage des dossiers temporaires..."
    rm -rf "$GAME_DIR/.save" "$GAME_DIR/.extra"
    rm -f "$GAME_DIR/.savepath" "$GAME_DIR/.extrapath"
    rm -f "$GAME_DIR/.gamename"

    # Demander le nom du jeu avec kdialog (pré-rempli avec le nom du dossier)
    if command -v kdialog &> /dev/null; then
        local INPUT
        INPUT=$(kdialog --inputbox "Nom du jeu (sera utilisé pour les sauvegardes et extras)" "$GAME_NAME")
        if [ -n "$INPUT" ]; then
            GAME_NAME="$INPUT"
            # Mettre à jour le nom du fichier de sortie
            WGPACK_NAME="$(dirname "$GAME_DIR")/${GAME_NAME}.wgp"
        fi
    fi

    # Créer le fichier .gamename pour conserver le nom du jeu
    echo "$GAME_NAME" > "$GAME_DIR/.gamename"

    # Étapes de configuration
    configure_compression
    select_executable
    configure_arguments
    configure_fix
    configure_saves
    configure_extras

    # Résumé et confirmation avant création
    show_summary_before_build

    # Création du paquet
    check_existing_wgp
    create_squashfs

    # Restaurer le chemin de l'exécutable et les arguments avant le nettoyage
    local EXE_REL_PATH=$(cat "$LAUNCH_FILE")
    local BOTTLE_ARGS=""
    [ -f "$ARGS_FILE" ] && BOTTLE_ARGS=$(cat "$ARGS_FILE")

    # Restauration finale des fichiers
    restore_all_saves
    restore_all_extras
    cleanup_temp_files

    # Affichage du résumé
    show_summary "$EXE_REL_PATH" "$BOTTLE_ARGS"
}

# Lancement du script
main "$@"
