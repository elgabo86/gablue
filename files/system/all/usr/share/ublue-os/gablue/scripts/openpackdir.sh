#!/bin/bash

################################################################################
# openpackdir.sh - Ouvre le dossier de sauvegardes ou d'extras d'un pack WGP/LGP
#
# Usage: openpackdir.sh --saves|--extras <fichier.wgp|fichier.lgp>
#
# Lit le marqueur .savepath/.extrapath directement dans le squashfs
# (unsquashfs -cat, aucun montage nécessaire) puis ouvre dans le gestionnaire
# de fichiers le dossier RÉEL utilisé au lancement par le lanceur :
#   - WGP saves  : ~/Windows/UserData/$USER/LocalSavesWGP/<nom interne>  (gwine)
#   - WGP extras : ~/.cache/wgp-extra/<nom interne>                      (gwine)
#   - LGP saves  : ~/.local/share/lgp-saves/<nom interne>                (lgplaunch)
#   - LGP extras : ~/.cache/lgp-extra/<nom interne>                      (lgplaunch)
#
# Le dossier est créé s'il n'existe pas encore (jeu jamais lancé). Pack sans
# sauvegardes/extras déclarés → message kdialog (rien à ouvrir).
################################################################################

#======================================
# Variables globales
#======================================
PACK_FILE=""
PACK_TYPE=""              # wgp | lgp
KIND=""                   # saves | extras
GAME_INTERNAL_NAME=""
TARGET_DIR=""

HOME_REAL="$(realpath "$HOME")"

#======================================
# Fonctions d'affichage et utilitaires
#======================================

# Affiche un message d'erreur et quitte
error_exit() {
    echo "Erreur: $1" >&2
    exit 1
}

# Affiche une boîte de dialogue (kdialog si disponible, sinon console)
# Usage: show_message <titre> <texte>
show_message() {
    local title="$1"
    local text="$2"

    if command -v kdialog &> /dev/null; then
        kdialog --title "$title" --msgbox "$text"
    else
        echo "$title: $text"
    fi
}

#======================================
# Analyse du pack
#======================================

# Vérifie les arguments et identifie le pack
validate_arguments() {
    local kind=""

    while [ $# -gt 0 ]; do
        case "$1" in
            --saves)  kind="saves" ;;
            --extras) kind="extras" ;;
            -*)       error_exit "Option inconnue: $1" ;;
            *)        PACK_FILE="$1" ;;
        esac
        shift
    done

    [ -n "$kind" ] || error_exit "Usage: $0 --saves|--extras <fichier.wgp|fichier.lgp>"
    [ -n "$PACK_FILE" ] || error_exit "Usage: $0 --saves|--extras <fichier.wgp|fichier.lgp>"
    [ -f "$PACK_FILE" ] || error_exit "Le fichier '$PACK_FILE' n'existe pas"

    case "${PACK_FILE,,}" in
        *.wgp) PACK_TYPE="wgp" ;;
        *.lgp) PACK_TYPE="lgp" ;;
        *)     error_exit "Extension non reconnue (attendu .wgp ou .lgp): $PACK_FILE" ;;
    esac

    KIND="$kind"
    PACK_FILE="$(realpath "$PACK_FILE")"
}

# Lit le marqueur demandé (.savepath / .extrapath) dans le squashfs
# Retourne 0 si le marqueur existe dans le pack, 1 sinon
pack_has_marker() {
    local marker="$1"

    command -v unsquashfs &> /dev/null || error_exit "unsquashfs introuvable"

    # unsquashfs -cat sort en erreur (rc 2) si le fichier est absent du pack
    unsquashfs -cat "$PACK_FILE" "$marker" > /dev/null 2>&1
}

# Récupère le nom interne du jeu (.gamename, comme makewgp/makelgp),
# repli sur le nom du fichier sans extension
read_gamename() {
    local name=""

    if command -v unsquashfs &> /dev/null; then
        name="$(unsquashfs -cat "$PACK_FILE" .gamename 2> /dev/null)"
    fi
    name="${name%.}"

    if [ -z "$name" ]; then
        name="$(basename "$PACK_FILE")"
        name="${name%.wgp}"
        name="${name%.lgp}"
    fi

    printf '%s' "$name"
}

# Résout le dossier réel selon le type de pack et le marqueur
resolve_target_dir() {
    case "$KIND/$PACK_TYPE" in
        saves/wgp)  TARGET_DIR="$HOME_REAL/Windows/UserData/${USER:-$(id -un)}/LocalSavesWGP/$GAME_INTERNAL_NAME" ;;
        extras/wgp) TARGET_DIR="$HOME_REAL/.cache/wgp-extra/$GAME_INTERNAL_NAME" ;;
        saves/lgp)  TARGET_DIR="$HOME_REAL/.local/share/lgp-saves/$GAME_INTERNAL_NAME" ;;
        extras/lgp) TARGET_DIR="$HOME_REAL/.cache/lgp-extra/$GAME_INTERNAL_NAME" ;;
    esac
}

#======================================
# Ouverture du dossier
#======================================

# Ouvre le dossier dans le gestionnaire de fichiers (détaché du script)
open_folder() {
    local dir="$1"

    if ! mkdir -p "$dir"; then
        error_exit "Impossible de créer le dossier '$dir'"
    fi

    echo "Dossier: $dir"

    if command -v dolphin &> /dev/null; then
        setsid dolphin "$dir" < /dev/null > /dev/null 2>&1 &
    elif command -v xdg-open &> /dev/null; then
        setsid xdg-open "$dir" < /dev/null > /dev/null 2>&1 &
    else
        show_message "Dossier ouvert" "$dir"
    fi
}

#======================================
# Fonction principale
#======================================

main() {
    local marker=""
    local title_label=""
    local missing_text=""

    validate_arguments "$@"

    case "$KIND" in
        saves)  marker=".savepath";   title_label="sauvegardes"; missing_text="de sauvegardes" ;;
        extras) marker=".extrapath";  title_label="extras/cache"; missing_text="d'extras" ;;
    esac

    if ! pack_has_marker "$marker"; then
        show_message "Dossier des $title_label" \
            "Le pack $(basename "$PACK_FILE") ne contient pas $missing_text."
        echo "Aucun $marker dans le pack: $(basename "$PACK_FILE")"
        exit 0
    fi

    GAME_INTERNAL_NAME="$(read_gamename)"
    resolve_target_dir

    open_folder "$TARGET_DIR"
}

main "$@"
