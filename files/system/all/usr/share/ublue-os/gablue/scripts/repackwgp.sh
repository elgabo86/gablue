#!/bin/bash

################################################################################
# repackwgp.sh - Script de repackaging de paquets WGP
#
# Ce script permet de mettre à jour un vieux WGP vers la nouvelle architecture
# (symlinks /tmp/wgp-saves au lieu de chemins utilisateur durs).
#
# Le script monte d'abord le WGP pour vérifier, puis offre deux options :
# - Mode Fix : Corrige uniquement les symlinks si nécessaire
# - Mode Complet : Rebuid en permettant d'éditer tout (comme makewgp.sh)
################################################################################

#======================================
# Variables globales
#======================================
WGPACK_FILE=""
GAME_NAME=""
GAME_INTERNAL_NAME=""
OUTPUT_FILE=""
TEMP_DIR=""
MOUNT_DIR=""
CHANGES_NEEDED=false
FULL_REBUILD=false
COMPRESS_CMD=""
COMPRESS_LEVEL_DEFAULT="15"
SUFFIX=""

#======================================
# Fonctions de nettoyage
#======================================

cleanup() {
    # Démonter le squashfs si monté
    if [ -n "$MOUNT_DIR" ] && mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        fusermount -u "$MOUNT_DIR" 2>/dev/null || true
        rm -rf "$MOUNT_DIR"
    fi
    # Nettoyer l'extraction si faite
    if [ -n "$TEMP_DIR" ]; then
        rm -rf "$TEMP_DIR" 2>/dev/null
    fi
}

trap cleanup EXIT

error_exit() {
    echo "Erreur: $1" >&2
    exit 1
}

#======================================
# Fonctions d'affichage et utilitaires
#======================================

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
# Choix du mode de repackage
#======================================

choose_repack_mode() {
    echo ""
    echo "=== Choix du mode de repackage ==="

    # Si aucun fix n'est nécessaire, passer directement en mode complet
    if ! $CHANGES_NEEDED; then
        echo "Ce WGP est déjà compatible."
        FULL_REBUILD=true
        CHANGES_NEEDED=true
        configure_compression
        echo "Mode Complet : rebuilder avec modification"
        return
    fi

    local msg_text="Voulez-vous :\n\n"
    msg_text+="1. Corriger uniquement les symlinks de sauvegarde (mode Fix)\n"
    msg_text+="2. Rebuilder le WGP avec modification complète (mode Complet)"

    local MODE_OPTION
    if command -v kdialog &> /dev/null; then
        MODE_OPTION=$(kdialog --radiolist "Mode de repackage" \
            "fix" "Corriger les symlinks (garder tout le reste)" "on" \
            "full" "Rebuilder complet (éditer tout)" "off")
    else
        echo "$msg_text" | sed 's/\\n/\n/g'
        read -p "Entrez le numéro (default: 1): " -r
        case "$REPLY" in
            ""|1) MODE_OPTION="fix" ;;
            2) MODE_OPTION="full" ;;
            *) error_exit "Choix invalide" ;;
        esac
    fi

    case "$MODE_OPTION" in
        "fix")
            echo "Mode Fix : correction des symlinks uniquement"
            FULL_REBUILD=false
            ;;
        "full")
            echo "Mode Complet : rebuilder avec modification"
            FULL_REBUILD=true
            ;;
        *)
            error_exit "Choix invalide"
            ;;
    esac

    # Configuration de la compression (commun à tous les modes)
    configure_compression
}

#======================================
# Fonctions de validation
#======================================

validate_arguments() {
    [ $# -eq 0 ] && error_exit "Usage: $0 <fichier_wgp> [fichier_sortie]"

    WGPACK_FILE="$(realpath "$1")"

    [ ! -f "$WGPACK_FILE" ] && error_exit "Le fichier '$WGPACK_FILE' n'existe pas"

    GAME_NAME=$(basename "$WGPACK_FILE" .wgp)

    if [ -n "$2" ]; then
        OUTPUT_FILE="$2"
    else
        OUTPUT_FILE="${WGPACK_FILE%.wgp}_repacked.wgp"
    fi
}

#======================================
# Fonctions de configuration
#======================================

# Configure le niveau de compression
configure_compression() {
    local DEFAULT_LEVEL="$COMPRESS_LEVEL_DEFAULT"

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
            ;;
        [1-9]|1[0-9])
            COMPRESS_CMD="-comp zstd -Xcompression-level $INPUT"
            ;;
        *)
            error_exit "Choix invalide"
            ;;
    esac
}

# Sélectionne le dossier temporaire d'extraction
select_temp_dir() {
    echo ""
    echo "=== Sélection du dossier temporaire ==="

    local CACHE_DIR="$HOME/.cache/repackwgp"
    local WGP_DIR="$(dirname "$WGPACK_FILE")"

    # Calculer la taille du WGP en Mo
    local WGP_SIZE_KB=$(du -s "$WGPACK_FILE" | cut -f1)
    local WGP_SIZE_MB=$((WGP_SIZE_KB / 1024))

    # Calculer la RAM libre disponible en Mo
    local MEM_AVAILABLE_KB=$(grep MemAvailable /proc/meminfo | awk '{print $2}')
    local MEM_AVAILABLE_MB=$((MEM_AVAILABLE_KB / 1024))

    # Besoin estimé : taille du WGP x 1.6 (extraction décompressée) + 60% de marge
    local REQUIRED_MB=$((WGP_SIZE_MB * 8 / 5))

    # Proposer /tmp si la RAM est suffisante
    if [ $MEM_AVAILABLE_MB -gt $REQUIRED_MB ]; then
        if command -v kdialog &> /dev/null; then
            if kdialog --yesno "Utiliser /tmp (RAM) pour l'extraction?\\n\\nTaille WGP: ${WGP_SIZE_MB} Mo\\nBesoin estimé: ${REQUIRED_MB} Mo\\nRAM libre: ${MEM_AVAILABLE_MB} Mo" --yes-label "Oui" --no-label "Choisir autre"; then
                TEMP_DIR="/tmp/${GAME_NAME}_repack_$$"
                echo "Dossier temporaire: $TEMP_DIR"
                return
            fi
        else
            read -p "Utiliser /tmp (RAM) pour l'extraction? (WGP: ${WGP_SIZE_MB} Mo, besoin: ${REQUIRED_MB} Mo, RAM dispo: ${MEM_AVAILABLE_MB} Mo) [O/n]: " -r
            [[ ! "$REPLY" =~ ^[nN]$ ]] && {
                TEMP_DIR="/tmp/${GAME_NAME}_repack_$$"
                echo "Dossier temporaire: $TEMP_DIR"
                return
            }
        fi
    fi

    # Sinon, proposer les autres options
    local TEMP_OPTION
    if command -v kdialog &> /dev/null; then
        TEMP_OPTION=$(select_from_list "Où extraire le WGP?" \
            "cache" "$CACHE_DIR (conseillé)" "on" \
            "local" "$WGP_DIR (même dossier)" "off")
    else
        echo "Où extraire le WGP?"
        echo "  1. $CACHE_DIR (conseillé)"
        echo "  2. $WGP_DIR (même dossier)"
        read -p "Entrez le numéro (default: 1): " -r
        case "$REPLY" in
            ""|1) TEMP_OPTION="cache" ;;
            2) TEMP_OPTION="local" ;;
        esac
    fi

    case "$TEMP_OPTION" in
        "cache")
            mkdir -p "$CACHE_DIR"
            TEMP_DIR="$CACHE_DIR/${GAME_NAME}_repack_$$"
            ;;
        "local")
            TEMP_DIR="$WGP_DIR/${GAME_NAME}_repack_$$"
            ;;
        *)
            error_exit "Choix invalide"
            ;;
    esac

    echo "Dossier temporaire: $TEMP_DIR"
}

#======================================
# Fonctions de montage et vérification
#======================================

mount_wgp() {
    MOUNT_DIR="/tmp/wgp_mount_$$"
    mkdir -p "$MOUNT_DIR"

    echo ""
    echo "=== Montage du WGP pour vérification ==="
    echo "Fichier: $WGPACK_FILE"
    echo "Dossier de montage: $MOUNT_DIR"

    if ! command -v squashfuse &> /dev/null; then
        error_exit "squashfuse n'est pas installé"
    fi

    squashfuse -r "$WGPACK_FILE" "$MOUNT_DIR"

    # Lire le nom interne du jeu
    GAMENAME_FILE="$MOUNT_DIR/.gamename"
    if [ -f "$GAMENAME_FILE" ]; then
        GAME_INTERNAL_NAME=$(cat "$GAMENAME_FILE")
    else
        GAME_INTERNAL_NAME="$GAME_NAME"
    fi

    echo "Nom interne du jeu: $GAME_INTERNAL_NAME"
}

check_symlinks() {
    [ ! -f "$MOUNT_DIR/.savepath" ] && return 0

    echo ""
    echo "=== Vérification des symlinks de sauvegarde ==="

    local symlinks_count=0

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SYMLINK_PATH="$MOUNT_DIR/$SAVE_REL_PATH"

        if [ -L "$SYMLINK_PATH" ]; then
            local current_target
            current_target=$(readlink "$SYMLINK_PATH")
            symlinks_count=$((symlinks_count + 1))

            # Vérifier si le symlink pointe déjà vers /tmp/wgp-saves
            if [[ "$current_target" == /tmp/wgp-saves/* ]]; then
                echo "Symlink: $SAVE_REL_PATH → $current_target (OK)"
            else
                echo "Symlink: $SAVE_REL_PATH → $current_target (à corriger)"
                CHANGES_NEEDED=true
            fi
        fi
    done < "$MOUNT_DIR/.savepath"

    if [ $symlinks_count -eq 0 ]; then
        echo "Aucun symlink de sauvegarde trouvé."
    fi

    if ! $CHANGES_NEEDED; then
        echo ""
        echo "✓ Tous les symlinks pointent déjà vers /tmp/wgp-saves"
    fi
}

check_extras() {
    [ ! -f "$MOUNT_DIR/.extrapath" ] && return 0

    echo ""
    echo "=== Extras ==="
    echo "Les extras utilisent /tmp/wgp-extra avec le nom du dossier."
    echo "Ils ne sont pas concernés par ce repackaging."
}

#======================================
# Fonctions d'édition complète
#======================================

# Sélectionne l'exécutable principal
select_executable() {
    echo ""
    echo "=== Sélection de l'exécutable ==="

    local EXE_LIST
    EXE_LIST=$(find "$TEMP_DIR" \( -type f -o -type l \) \( -iname "*.exe" -o -iname "*.bat" \) 2>/dev/null)

    [ -z "$EXE_LIST" ] && {
        echo "Aucun fichier .exe ou .bat trouvé"
        return 1
    }

    local EXE_ARRAY=()
    local COUNT=0
    while IFS= read -r exe; do
        local REL_PATH="${exe#$TEMP_DIR/}"
        EXE_ARRAY+=("$exe" "$REL_PATH")
        [ $COUNT -eq 0 ] && EXE_ARRAY+=("on") || EXE_ARRAY+=("off")
        COUNT=$((COUNT + 1))
    done <<< "$EXE_LIST"

    local SELECTED
    SELECTED=$(select_from_list "Sélectionnez l'exécutable principal:" "${EXE_ARRAY[@]}")

    [ ! -f "$SELECTED" ] && {
        echo "Exécutable non valide"
        return 1
    }

    echo ""
    echo "Exécutable sélectionné: ${SELECTED#$TEMP_DIR/}"

    # Mettre à jour le fichier .launch
    echo "${SELECTED#$TEMP_DIR/}" > "$TEMP_DIR/.launch"
}

# Configure les arguments de lancement
configure_arguments() {
    local BOTTLE_ARGS=""

    if ask_yes_no "Voulez-vous modifier les arguments de lancement ?\n(ex: --dx12, --window, --no-borders, etc.)"; then
        # Lire les arguments actuels
        if [ -f "$TEMP_DIR/.args" ]; then
            local CURRENT_ARGS
            CURRENT_ARGS=$(cat "$TEMP_DIR/.args")
            if command -v kdialog &> /dev/null; then
                BOTTLE_ARGS=$(kdialog --inputbox "Arguments de lancement" "$CURRENT_ARGS")
            else
                echo "Arguments actuels: $CURRENT_ARGS"
                read -p "Entrez les nouveaux arguments (laisser vide pour conserver): " -r
                [ -n "$REPLY" ] && BOTTLE_ARGS="$REPLY" || BOTTLE_ARGS="$CURRENT_ARGS"
            fi
        else
            if command -v kdialog &> /dev/null; then
                BOTTLE_ARGS=$(kdialog --inputbox "Entrez les arguments de lancement")
            else
                read -p "Entrez les arguments: " -r
                BOTTLE_ARGS="$REPLY"
            fi
        fi
    fi

    if [ -n "$BOTTLE_ARGS" ]; then
        echo "$BOTTLE_ARGS" > "$TEMP_DIR/.args"
        echo "Arguments: $BOTTLE_ARGS"
    else
        rm -f "$TEMP_DIR/.args"
        echo "Arguments: Aucun"
    fi
}

# Configure le fix manette
configure_fix() {
    if [ -f "$TEMP_DIR/.fix" ]; then
        echo "Fix manette: Déjà activé"
        if ask_yes_no "Désactiver le fix manette ?" "Désactiver" "Garder"; then
            rm -f "$TEMP_DIR/.fix"
            echo "Fix manette: Désactivé"
        fi
    else
        if ask_yes_no "Voulez-vous activer le fix manette pour ce jeu ?\n\nCela modifie une clé de registre Wine (DisableHidraw)\npour résoudre des problèmes de compatibilité manette."; then
            touch "$TEMP_DIR/.fix"
            echo "Fix manette: Activé"
        else
            echo "Fix manette: Non activé"
        fi
    fi
}

#======================================
# Fonctions de gestion des sauvegardes
#======================================

# Demande et configure un dossier/fichier de sauvegarde
add_save_item() {
    ask_yes_no "Ajouter/modifier un dossier/fichier de sauvegarde ?\n\nSauvegardes persistantes stockées dans UserData.\nUne copie reste dans le paquet pour la portabilité." || return 1

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
        SELECTED_ITEM=$(select_directory "$TEMP_DIR")
    else
        SELECTED_ITEM=$(select_file "$TEMP_DIR")
    fi

    [ -z "$SELECTED_ITEM" ] && return 1

    # Validation
    local SAVE_REL_PATH="${SELECTED_ITEM#$TEMP_DIR/}"
    if [[ "$SELECTED_ITEM" != "$TEMP_DIR/"* ]]; then
        error_exit "L'élément doit être dans le dossier du jeu"
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

    # Chemin système indépendant de l'utilisateur
    local SAVES_DIR="/tmp/wgp-saves/$GAME_INTERNAL_NAME"

    # Créer .save dans le WGP
    local SAVE_WGP_ITEM="$TEMP_DIR/.save/$SAVE_REL_PATH"
    mkdir -p "$(dirname "$SAVE_WGP_ITEM")"

    # Copier vers .save (portabilité)
    echo "Copie vers .save pour portabilité..."
    if [ "$SAVE_TYPE" = "dir" ]; then
        cp -a "$SAVE_ITEM_ABSOLUTE"/. "$SAVE_WGP_ITEM"/
    else
        cp "$SAVE_ITEM_ABSOLUTE" "$SAVE_WGP_ITEM"
    fi

    # Créer symlink vers /tmp/wgp-saves (chemin système)
    echo "Création du symlink vers /tmp/wgp-saves..."
    if [ "$SAVE_TYPE" = "dir" ]; then
        rm -rf "$SAVE_ITEM_ABSOLUTE"
        ln -s "$SAVES_DIR/$SAVE_REL_PATH" "$SAVE_ITEM_ABSOLUTE"
    else
        rm -f "$SAVE_ITEM_ABSOLUTE"
        ln -s "$SAVES_DIR/$SAVE_REL_PATH" "$SAVE_ITEM_ABSOLUTE"
    fi

    # Mettre à jour .savepath
    mkdir -p "$(dirname "$TEMP_DIR/.savepath")"
    echo "$SAVE_REL_PATH" >> "$TEMP_DIR/.savepath"
}

# Configure tous les éléments de sauvegarde
configure_saves() {
    echo ""
    echo "=== Gestion des sauvegardes ==="

    # Afficher les sauvegardes actuelles
    if [ -f "$TEMP_DIR/.savepath" ]; then
        echo "Sauvegardes actuelles:"
        while IFS= read -r SAVE_REL_PATH; do
            [ -z "$SAVE_REL_PATH" ] && continue
            echo "  - $SAVE_REL_PATH"
        done < "$TEMP_DIR/.savepath"
    else
        echo "Aucune sauvegarde configurée."
    fi

    while true; do
        add_save_item || break
    done
}

#======================================
# Fonctions de gestion des extras
#======================================

# Demande et configure un fichier/dossier d'extra
add_extra_item() {
    ask_yes_no "Ajouter/modifier un fichier/dossier d'extra ?\n\nLes extras seront stockés dans /tmp/wgp-extra\n\nCe sont des fichiers temporaires (config, cache...)\nperdus après la fermeture du jeu.\nLe WGP utilisera un symlink vers /tmp." || return 1

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
        SELECTED_ITEM=$(select_directory "$TEMP_DIR")
    else
        SELECTED_ITEM=$(select_file "$TEMP_DIR")
    fi

    [ -z "$SELECTED_ITEM" ] && return 1

    # Validation
    local EXTRA_REL_PATH="${SELECTED_ITEM#$TEMP_DIR/}"
    if [[ "$SELECTED_ITEM" != "$TEMP_DIR/"* ]]; then
        error_exit "L'élément doit être dans le dossier du jeu"
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
    local EXTRA_DIR="$EXTRA_BASE/$GAME_INTERNAL_NAME"

    # Créer .extra dans le WGP
    local EXTRA_WGP_ITEM="$TEMP_DIR/.extra/$EXTRA_REL_PATH"
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
    mkdir -p "$(dirname "$TEMP_DIR/.extrapath")"
    echo "$EXTRA_REL_PATH" >> "$TEMP_DIR/.extrapath"
}

# Configure tous les éléments d'extra
configure_extras() {
    echo ""
    echo "=== Gestion des extras ==="

    # Afficher les extras actuels
    if [ -f "$TEMP_DIR/.extrapath" ]; then
        echo "Extras actuels:"
        while IFS= read -r EXTRA_REL_PATH; do
            [ -z "$EXTRA_REL_PATH" ] && continue
            echo "  - $EXTRA_REL_PATH"
        done < "$TEMP_DIR/.extrapath"
    else
        echo "Aucun extra configuré."
    fi

    while true; do
        add_extra_item || break
    done
}

#======================================
# Fonctions de gestion de l'icône
#======================================

# Extrait la plus grande icône PNG depuis un .exe Windows
extract_icon_from_exe() {
    local exe_path="$1"
    local output_dir="$2"

    # Extraire les groupes icônes depuis l'exécutable
    wrestool -x -t14 "$exe_path" -o "$output_dir" 2>/dev/null || return 1

    # Trouver les fichiers .ico extraits
    local ico_files
    ico_files=$(find "$output_dir" -name "*.ico" 2>/dev/null)
    [ -z "$ico_files" ] && return 1

    # Convertir les .ico en .png
    icotool --extract --output="$output_dir" $ico_files 2>/dev/null

    # Retourner le plus grand PNG
    local biggest_png=$(find "$output_dir" -name "*.png" -exec ls -S {} + 2>/dev/null | head -n1)

    if [ -n "$biggest_png" ] && file "$biggest_png" | grep -q "PNG image"; then
        echo "$biggest_png"
        return 0
    fi
    return 1
}

# Convertit un fichier d'icône (.png, .ico, etc.) vers un PNG standardisé
process_icon_to_standard() {
    local input_file="$1"
    local output_file="$2"

    case "${input_file,,}" in
        *.png)
            # Copier le PNG
            cp "$input_file" "$output_file"
            return 0
            ;;
        *.ico)
            # Convertir .ico en PNG
            TEMP_ICO=$(mktemp -d)
            icotool --extract --output="$TEMP_ICO" "$input_file" 2>/dev/null
            BIGGEST_PNG=$(find "$TEMP_ICO" -name "*.png" -exec ls -S {} + 2>/dev/null | head -n1)
            if [ -n "$BIGGEST_PNG" ]; then
                cp "$BIGGEST_PNG" "$output_file"
                rm -rf "$TEMP_ICO"
                return 0
            fi
            rm -rf "$TEMP_ICO"
            return 1
            ;;
        *)
            # Format non supporté
            return 1
            ;;
    esac
}

# Configure une icône custom pour le WGP
configure_custom_icon() {
    echo ""
    echo "=== Gestion de l'icône ==="

    # État actuel
    if [ -f "$TEMP_DIR/.icon.png" ]; then
        echo "Icône custom actuelle: .icon.png"
    else
        echo "Icône: Par défaut (extraite depuis l'exécutable)"
    fi

    ask_yes_no "Voulez-vous changer l'icône ?\n\nVous pouvez choisir:\n- Un fichier .exe pour extraire son icône\n- Un fichier .png\n- Un fichier .ico\n- Supprimer l'icône custom" "Changer" "Garder" || return 0

    local ICON_ACTION
    if command -v kdialog &> /dev/null; then
        ICON_ACTION=$(kdialog --radiolist "Action sur l'icône" \
            "exe" "Extraire depuis un .exe" "on" \
            "png" "Utiliser un fichier .png" "off" \
            "ico" "Utiliser un fichier .ico" "off" \
            "remove" "Supprimer l'icône custom" "off")
    else
        echo "Actions disponibles:"
        echo "  1. Extraire depuis un .exe"
        echo "  2. Utiliser un fichier .png"
        echo "  3. Utiliser un fichier .ico"
        echo "  4. Supprimer l'icône custom"
        read -p "Entrez le numéro (default: 1): " -r
        case "$REPLY" in
            ""|1) ICON_ACTION="exe" ;;
            2) ICON_ACTION="png" ;;
            3) ICON_ACTION="ico" ;;
            4) ICON_ACTION="remove" ;;
            *) error_exit "Choix invalide" ;;
        esac
    fi

    case "$ICON_ACTION" in
        "remove")
            rm -f "$TEMP_DIR/.icon.png"
            echo "Icône custom supprimée"
            return 0
            ;;
    esac

    while true; do
        local icon_file=""
        if command -v kdialog &> /dev/null; then
            if [ "$ICON_ACTION" = "exe" ]; then
                icon_file=$(kdialog --getopenfilename "$TEMP_DIR" "*.exe | Exécutables Windows")
            elif [ "$ICON_ACTION" = "png" ]; then
                icon_file=$(kdialog --getopenfilename "$TEMP_DIR" "*.png | Images PNG")
            elif [ "$ICON_ACTION" = "ico" ]; then
                icon_file=$(kdialog --getopenfilename "$TEMP_DIR" "*.ico | Images ICO")
            fi
        else
            read -p "Entrez le chemin relatif (depuis le dossier extraits): " -r
            if [ -n "$REPLY" ]; then
                icon_file="$TEMP_DIR/$REPLY"
            fi
        fi

        [ -z "$icon_file" ] && return 0

        # Traiter selon le type de fichier
        local icon_name=$(basename "$icon_file")

        case "${icon_name,,}" in
            *.exe)
                echo "Extraction de l'icône depuis: $icon_name"
                TEMP_ICO=$(mktemp -d)
                local extracted_png=$(extract_icon_from_exe "$icon_file" "$TEMP_ICO")
                if [ -n "$extracted_png" ]; then
                    cp "$extracted_png" "$TEMP_DIR/.icon.png"
                    rm -rf "$TEMP_ICO"
                    echo "Icône extraite avec succès: .icon.png"
                    return 0
                else
                    rm -rf "$TEMP_ICO"
                    echo "Erreur: aucune icône trouvée dans l'exécutable"
                fi
                ;;
            *.png)
                if process_icon_to_standard "$icon_file" "$TEMP_DIR/.icon.png"; then
                    echo "Icône PNG copiée avec succès: .icon.png"
                    return 0
                else
                    echo "Erreur: impossible de copier l'icône PNG"
                fi
                ;;
            *.ico)
                if process_icon_to_standard "$icon_file" "$TEMP_DIR/.icon.png"; then
                    echo "Icône ICO convertie avec succès: .icon.png"
                    return 0
                else
                    echo "Erreur: impossible de convertir l'icône ICO"
                fi
                ;;
            *)
                echo "Erreur: format non supporté ($icon_name)"
                ;;
        esac

        # Demander si l'utilisateur veut renouveler
        if ! ask_yes_no "Voulez-vous choisir une autre icône ?"; then
            break
        fi
    done
}

# Affiche le résumé des choix avant création avec confirmation
show_summary_before_build() {
    local EXE_REL_PATH=$(cat "$TEMP_DIR/.launch" 2>/dev/null || echo "Inconnu")
    local BOTTLE_ARGS=""
    [ -f "$TEMP_DIR/.args" ] && BOTTLE_ARGS=$(cat "$TEMP_DIR/.args")
    local FIX_ENABLED=""
    [ -f "$TEMP_DIR/.fix" ] && FIX_ENABLED="Oui"

    # Compression
    local COMP_INFO="sans compression"
    if [ -n "$COMPRESS_CMD" ]; then
        COMP_INFO="zstd $(echo "$COMPRESS_CMD" | grep -o '[0-9]\+' | head -1)"
    fi

    # Sauvegardes
    local SAVES_LIST=""
    if [ -f "$TEMP_DIR/.savepath" ]; then
        local SAVE_COUNT=$(wc -l < "$TEMP_DIR/.savepath")
        SAVES_LIST="$SAVE_COUNT sauvegarde(s)"
    else
        SAVES_LIST="Aucune"
    fi

    # Extras
    local EXTRAS_LIST=""
    if [ -f "$TEMP_DIR/.extrapath" ]; then
        local EXTRA_COUNT=$(wc -l < "$TEMP_DIR/.extrapath")
        EXTRAS_LIST="$EXTRA_COUNT extra(s)"
    else
        EXTRAS_LIST="Aucun"
    fi

    # Icône
    local ICON_INFO="Par défaut (depuis .exe)"
    [ -f "$TEMP_DIR/.icon.png" ] && ICON_INFO="Custom (.icon.png)"

    local MSG="=== Résumé de la modification du WGP ===

WGP source: $WGPACK_FILE
Fichier de sortie: $OUTPUT_FILE

Mode: $(if $FULL_REBUILD; then echo "Rebuild complet"; else echo "Fix simple"; fi)
Exécutable: $EXE_REL_PATH
Arguments: ${BOTTLE_ARGS:-Aucun}
Fix manette: ${FIX_ENABLED:-Non}
Compression: $COMP_INFO
Sauvegardes: $SAVES_LIST
Extras: $EXTRAS_LIST
Icône: $ICON_INFO

Voulez-vous continuer ?"

    if ! ask_yes_no "$MSG"; then
        echo ""
        echo "Annulation demandée."
        return 1
    fi
    return 0
}

#======================================
# Fonctions d'édition du nom du jeu
#======================================

# Demande et modifie le nom du jeu (comme makewgp.sh)
configure_game_name() {
    echo ""
    echo "=== Édition du nom du jeu ==="

    # Afficher le nom actuel
    echo "Nom actuel: $GAME_INTERNAL_NAME"

    if command -v kdialog &> /dev/null; then
        local INPUT
        INPUT=$(kdialog --inputbox "Nom du jeu (sera utilisé pour les sauvegardes et extras)" "$GAME_INTERNAL_NAME")
        if [ -n "$INPUT" ] && [ "$INPUT" != "$GAME_INTERNAL_NAME" ]; then
            echo ""
            echo "Nom modifié: $GAME_INTERNAL_NAME → $INPUT"
            GAME_INTERNAL_NAME="$INPUT"
            # Mettre à jour le nom du fichier de sortie
            local OUTPUT_DIR_NAME
            OUTPUT_DIR_NAME=$(dirname "$OUTPUT_FILE")
            OUTPUT_FILE="$OUTPUT_DIR_NAME/${GAME_INTERNAL_NAME}.wgp"
            echo "Fichier de sortie modifié: $OUTPUT_FILE"
        fi
    else
        read -p "Entrez le nouveau nom du jeu (Entrée pour garder '$GAME_INTERNAL_NAME'): " -r
        if [ -n "$REPLY" ] && [ "$REPLY" != "$GAME_INTERNAL_NAME" ]; then
            echo ""
            echo "Nom modifié: $GAME_INTERNAL_NAME → $REPLY"
            GAME_INTERNAL_NAME="$REPLY"
            # Mettre à jour le nom du fichier de sortie
            local OUTPUT_DIR_NAME
            OUTPUT_DIR_NAME=$(dirname "$OUTPUT_FILE")
            OUTPUT_FILE="$OUTPUT_DIR_NAME/${GAME_INTERNAL_NAME}.wgp"
            echo "Fichier de sortie modifié: $OUTPUT_FILE"
        fi
    fi
}

#======================================
# Exécution de l'édition complète
#======================================

run_full_edit_mode() {
    # Démonter et extraire
    fusermount -u "$MOUNT_DIR" 2>/dev/null || true
    rm -rf "$MOUNT_DIR"
    MOUNT_DIR=""

    select_temp_dir
    mkdir -p "$TEMP_DIR"

    echo ""
    echo "=== Extraction du WGP ==="
    echo "Dossier temporaire: $TEMP_DIR"

    local EXIT_CODE
    if command -v kdialog &> /dev/null; then
        extract_with_dialog
        EXIT_CODE=$?
    else
        extract_console
        EXIT_CODE=$?
    fi

    [ $EXIT_CODE -ne 0 ] && {
        echo "Erreur lors de l'extraction"
        exit 1
    }

    # Extraire l'icône depuis le .exe symlinké si nécessaire
    extract_icon_from_symlink_exe

    # Étapes de configuration
    echo ""
    echo "=== Configuration du WGP ==="
    echo ""

    # Édition du nom du jeu
    configure_game_name
    echo "$GAME_INTERNAL_NAME" > "$TEMP_DIR/.gamename"

    # Correction des symlinks existants si nécessaire
    if [ -f "$TEMP_DIR/.savepath" ]; then
        echo "=== Correction des symlinks de sauvegarde existants ==="
        local NEW_SAVE_BASE="/tmp/wgp-saves/$GAME_INTERNAL_NAME"
        while IFS= read -r SAVE_REL_PATH; do
            [ -z "$SAVE_REL_PATH" ] && continue
            local SYMLINK_PATH="$TEMP_DIR/$SAVE_REL_PATH"
            if [ -L "$SYMLINK_PATH" ]; then
                local current_target
                current_target=$(readlink "$SYMLINK_PATH")
                if [[ "$current_target" != /tmp/wgp-saves/* ]]; then
                    rm -f "$SYMLINK_PATH"
                    ln -s "$NEW_SAVE_BASE/$SAVE_REL_PATH" "$SYMLINK_PATH"
                    echo "Symlink corrigé: $SAVE_REL_PATH"
                fi
            fi
        done < "$TEMP_DIR/.savepath"
    fi

    # Configuration des éléments (compression déjà configurée dans choose_repack_mode)
    select_executable
    configure_arguments
    configure_fix
    configure_saves
    configure_extras
    configure_custom_icon

    # Résumé et confirmation
    if ! show_summary_before_build; then
        cleanup
        exit 0
    fi

    # Re-vérifier si le fichier de sortie existe (au cas où le nom a changé)
    check_existing_output
}

#======================================
# Fonctions d'extraction et correction
#======================================

# Extrait le squashfs avec interface graphique
extract_with_dialog() {
    # Fenêtre d'attente avec bouton Annuler
    kdialog --msgbox "Extraction en cours...\nAppuyez sur Annuler pour arrêter" --ok-label "Annuler" >/dev/null &
    local KDIALOG_PID=$!

    # Lancer unsquashfs en arrière-plan
    unsquashfs -f -d "$TEMP_DIR" -no-xattrs "$WGPACK_FILE" &
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
    echo "Extraction de $WGPACK_FILE..."
    unsquashfs -f -d "$TEMP_DIR" -no-xattrs "$WGPACK_FILE"
}

extract_and_fix() {
    # Démonter le mount
    fusermount -u "$MOUNT_DIR" 2>/dev/null || true
    rm -rf "$MOUNT_DIR"
    MOUNT_DIR=""

    # Sélectionner le dossier temporaire
    select_temp_dir
    mkdir -p "$TEMP_DIR"

    echo ""
    echo "=== Extraction du WGP ==="
    echo "Dossier temporaire: $TEMP_DIR"

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
        exit 1
    fi

    # Extraire l'icône depuis le .exe symlinké si nécessaire
    extract_icon_from_symlink_exe

    # Corriger les symlinks
    echo ""
    echo "=== Correction des symlinks de sauvegarde ==="

    local NEW_SAVE_BASE="/tmp/wgp-saves/$GAME_INTERNAL_NAME"

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SYMLINK_PATH="$TEMP_DIR/$SAVE_REL_PATH"

        if [ -L "$SYMLINK_PATH" ]; then
            local current_target
            current_target=$(readlink "$SYMLINK_PATH")

            if [[ "$current_target" != /tmp/wgp-saves/* ]]; then
                rm -f "$SYMLINK_PATH"
                ln -s "$NEW_SAVE_BASE/$SAVE_REL_PATH" "$SYMLINK_PATH"
                echo "Corrigé: $SAVE_REL_PATH → /tmp/wgp-saves/$GAME_INTERNAL_NAME/$SAVE_REL_PATH"
            fi
        fi
    done < "$TEMP_DIR/.savepath"

    # Proposer de modifier l'icône custom
    configure_custom_icon
}

# Vérifie si le fichier de sortie existe déjà et demande confirmation
check_existing_output() {
    [ ! -f "$OUTPUT_FILE" ] && return 0

    echo ""
    echo "Attention: le fichier de sortie existe déjà"
    echo "Fichier: $OUTPUT_FILE"

    if ask_yes_no "Le fichier de sortie existe déjà.\nVoulez-vous l'écraser ?"; then
        echo "Suppression de l'ancien fichier: $OUTPUT_FILE"
        rm -f "$OUTPUT_FILE"
        return 0
    fi

    # L'utilisateur ne veut pas écraser → annuler et nettoyer
    echo ""
    echo "Annulation demandée."
    cleanup
    exit 0
}

#======================================
# Fonctions de création du squashfs
#======================================

# Crée le squashfs avec interface graphique (annulation possible)
create_squashfs_with_dialog() {
    # Fenêtre informative
    kdialog --msgbox "Recompression en cours pour: $GAME_NAME" --ok-label "Annuler" >/dev/null &
    local KDIALOG_PID=$!

    # Lancer mksquashfs en arrière-plan
    mksquashfs "$TEMP_DIR" "$OUTPUT_FILE" $COMPRESS_CMD -nopad -all-root &
    local MKSQUASH_PID=$!

    # Surveiller jusqu'à ce que mksquashfs se termine
    while kill -0 $MKSQUASH_PID 2>/dev/null; do
        # Annulation si kdialog fermé
        if ! kill -0 $KDIALOG_PID 2>/dev/null; then
            kill -9 $MKSQUASH_PID 2>/dev/null
            pkill -9 mksquashfs 2>/dev/null
            rm -f "$OUTPUT_FILE"
            echo ""
            echo "Recompression annulée"
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
        exit 1
    fi
}

# Crée le squashfs en mode console
create_squashfs_console() {
    echo "Recompression de $WGPACK_FILE en cours..."
    mksquashfs "$TEMP_DIR" "$OUTPUT_FILE" $COMPRESS_CMD -nopad -all-root

    if [ $? -ne 0 ]; then
        echo "Erreur lors de la création du squashfs"
        exit 1
    fi
}

repack_wgp() {
    echo ""
    echo "=== Création du WGP repackagé ==="

    local EXIT_CODE
    if command -v kdialog &> /dev/null; then
        create_squashfs_with_dialog
        EXIT_CODE=$?
    else
        create_squashfs_console
        EXIT_CODE=$?
    fi

    return $EXIT_CODE
}

#======================================
# Fonctions d'affichage final
#======================================

show_success() {
    local SIZE_BEFORE=$(du -s "$WGPACK_FILE" | cut -f1)
    local SIZE_BEFORE_MB=$(echo "scale=1; $SIZE_BEFORE / 1024" | bc)
    local SIZE_AFTER=$(du -s "$OUTPUT_FILE" | cut -f1)
    local SIZE_AFTER_MB=$(echo "scale=1; $SIZE_AFTER / 1024" | bc)

    echo ""
    echo "=== Succès ==="
    echo "WGP repackagé: $OUTPUT_FILE"
    echo "Taille avant: ${SIZE_BEFORE_MB} Mo"
    echo "Taille après: ${SIZE_AFTER_MB} Mo"

    # Fenêtre de succès KDE
    if command -v kdialog &> /dev/null; then
        local MSG="WGP repackagé avec succès!\n\n"
        MSG+="Fichier: $OUTPUT_FILE\n"
        MSG+="Taille avant: ${SIZE_BEFORE_MB} Mo\n"
        MSG+="Taille après: ${SIZE_AFTER_MB} Mo"

        kdialog --title "Succès" --msgbox "$MSG"
    fi
}

#======================================
# Fonction principale
#======================================

main() {
    cat << 'EOF'
╔══════════════════════════════════════════════════════════════╗
║                    Repackaging de WGP                       ║
╚══════════════════════════════════════════════════════════════╝
EOF

    validate_arguments "$@"

    mount_wgp
    check_symlinks
    check_extras

    # Choix du mode de repackage
    choose_repack_mode

    # Vérifier si le fichier de sortie existe déjà
    check_existing_output

    # Selon le mode
    if $FULL_REBUILD; then
        # Mode Complet : rebuilder avec modification
        run_full_edit_mode
    else
        # Mode Fix : extraction et correction simple
        extract_and_fix
    fi

    # Création du WGP repackagé
    repack_wgp
    show_success
}

# Lancement du script
main "$@"
