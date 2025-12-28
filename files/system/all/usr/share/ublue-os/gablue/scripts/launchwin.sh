#!/bin/bash

################################################################################
# launchwin.sh - Script de lancement de jeux Windows
#
# Ce script permet de lancer des jeux Windows via Bottles, avec support :
# - Des paquets WGP (.wgp) compressés
# - Des exécutables directs (.exe)
# - Du fix manette (optionnel)
################################################################################

#======================================
# Variables globales
#======================================
fix_mode=false
args=""
fullpath=""

# Variables WGP
WGPACK_NAME=""
GAME_INTERNAL_NAME=""
MOUNT_DIR=""

#======================================
# Fonctions d'affichage et utilitaires
#======================================

# Affiche un message d'erreur et quitte
error_exit() {
    echo "Erreur: $1" >&2
    exit 1
}

#======================================
# Fonctions d'analyse des paramètres
#======================================

# Analyse les arguments de la ligne de commande
parse_arguments() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --fix)
                fix_mode=true
                shift
                ;;
            --args)
                args="$2"
                shift 2
                ;;
            *)
                fullpath="$1"
                shift
                ;;
        esac
    done

    [ -z "$fullpath" ] && error_exit "Aucun fichier spécifié"
}

#======================================
# Fonctions de fix manette (DisableHidraw)
#======================================

# Configure le registre pour le mode fixmanette
apply_padfix_setting() {
    local SYSTEM_REG="$HOME/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg"

    if [ "$fix_mode" = true ]; then
        # Mode fix: désactiver DisableHidraw
        sed -i 's/"DisableHidraw"=dword:00000001/"DisableHidraw"=dword:00000000/' "$SYSTEM_REG"
    else
        # Mode normal: activer DisableHidraw
        sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' "$SYSTEM_REG"
    fi
}

# Restaurer DisableHidraw après le lancement (mode fix)
restore_padfix_setting() {
    [ "$fix_mode" != true ] && return 0

    sleep 2
    local SYSTEM_REG="$HOME/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg"
    sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' "$SYSTEM_REG"
}

#======================================
# Fonctions de gestion des WGP
#======================================

# Initialise les variables pour le mode WGP
init_wgp_variables() {
    WGPACK_FILE="$(realpath "$fullpath")"
    WGPACK_NAME="$(basename "$WGPACK_FILE" .wgp)"

    # Nettoyer les points et espaces terminaux (Wine n'aime pas)
    WGPACK_NAME="${WGPACK_NAME%.}"

    MOUNT_BASE="/tmp/wgpackmount"
    MOUNT_DIR="$MOUNT_BASE/$WGPACK_NAME"
    EXTRA_BASE="/tmp/wgp-extra"
    EXTRA_DIR="$EXTRA_BASE/$WGPACK_NAME"  # Sera mis à jour après lecture de .gamename
}

# Monte le squashfs du paquet WGP
mount_wgp() {
    mkdir -p "$MOUNT_BASE"

    # Vérifier si déjà monté
    if mountpoint -q "$MOUNT_DIR"; then
        error_exit "$WGPACK_NAME est déjà monté"
    fi

    # Vérifier que squashfuse est disponible
    if ! command -v squashfuse &> /dev/null; then
        error_exit "squashfuse n'est pas installé (Installation: paru -S squashfuse)"
    fi

    # Créer et monter le squashfs
    mkdir -p "$MOUNT_DIR"
    echo "Montage de $WGPACK_FILE sur $MOUNT_DIR..."
    squashfuse -r "$WGPACK_FILE" "$MOUNT_DIR"

    if [ $? -ne 0 ]; then
        rmdir "$MOUNT_DIR"
        error_exit "Erreur lors du montage du squashfs"
    fi
}

# Nettoie en démontant le WGP et les extras
cleanup_wgp() {
    echo "Démontage de $WGPACK_NAME..."

    # Nettoyer le dossier temporaire d'extra
    if [ -d "$EXTRA_DIR" ]; then
        rm -rf "$EXTRA_DIR"
        echo "Dossier temporaire d'extra nettoyé: $EXTRA_DIR"
    fi

    # Démontage du squashfs
    if ! fusermount -u "$MOUNT_DIR" 2>/dev/null; then
        # Si échec, tuer le processus squashfuse
        local FUSE_PID=$(fuser -m "$MOUNT_DIR" 2>/dev/null | head -n1)
        if [ -n "$FUSE_PID" ]; then
            kill -9 "$FUSE_PID" 2>/dev/null
            sleep 0.5
        fi
        # Force unmount lazy si nécessaire
        fusermount -uz "$MOUNT_DIR" 2>/dev/null
    fi

    # Nettoyer le dossier s'il existe et n'est plus monté
    if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        umount -f "$MOUNT_DIR" 2>/dev/null
    fi
    rmdir "$MOUNT_DIR" 2>/dev/null
}

# Lit les fichiers de configuration du WGP
read_wgp_config() {
    # Fichier .gamename (nom interne du jeu pour extras et saves)
    local GAMENAME_FILE="$MOUNT_DIR/.gamename"
    if [ -f "$GAMENAME_FILE" ]; then
        GAME_INTERNAL_NAME=$(cat "$GAMENAME_FILE")
        # Mettre à jour EXTRA_DIR avec le nom interne du jeu
        EXTRA_DIR="$EXTRA_BASE/$GAME_INTERNAL_NAME"
    else
        # Fallback: utiliser le nom du fichier .wgp
        GAME_INTERNAL_NAME="$WGPACK_NAME"
    fi

    # Fichier .launch
    local LAUNCH_FILE="$MOUNT_DIR/.launch"
    if [ ! -f "$LAUNCH_FILE" ]; then
        echo "Erreur: fichier .launch introuvable dans le pack" >&2
        cleanup_wgp
        exit 1
    fi

    FULL_EXE_PATH="$MOUNT_DIR/$(cat "$LAUNCH_FILE")"

    if [ ! -f "$FULL_EXE_PATH" ]; then
        echo "Erreur: exécutable introuvable: $(cat "$LAUNCH_FILE")" >&2
        cleanup_wgp
        exit 1
    fi

    # Fichier .args (surcharge les arguments en ligne de commande)
    local ARGS_FILE="$MOUNT_DIR/.args"
    if [ -f "$ARGS_FILE" ]; then
        local wgp_args
        wgp_args=$(cat "$ARGS_FILE")
        if [ -n "$wgp_args" ]; then
            args="$wgp_args"
        fi
    fi

    # Fichier .fix (active le fix manette)
    local FIX_FILE="$MOUNT_DIR/.fix"
    if [ -f "$FIX_FILE" ]; then
        fix_mode=true
    fi
}

# Prépare les sauvegardes depuis UserData
prepare_saves() {
    local SAVE_FILE="$MOUNT_DIR/.savepath"
    local SAVE_WGP_DIR="$MOUNT_DIR/.save"
    local WINDOWS_HOME="$HOME/Windows/UserData"
    local SAVES_BASE="$WINDOWS_HOME/$USER/LocalSavesWGP"
    local SAVES_DIR="$SAVES_BASE/$GAME_INTERNAL_NAME"

    [ -f "$SAVE_FILE" ] || return 0

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SAVE_WGP_ITEM="$SAVE_WGP_DIR/$SAVE_REL_PATH"
        local FINAL_SAVE_ITEM="$SAVES_DIR/$SAVE_REL_PATH"

        if [ -d "$SAVE_WGP_ITEM" ]; then
            # Dossier: copier depuis .save uniquement si n'existe pas dans UserData
            if [ ! -d "$FINAL_SAVE_ITEM" ]; then
                echo "Copie des sauvegardes: $SAVE_REL_PATH"
                mkdir -p "$FINAL_SAVE_ITEM"
                cp -a "$SAVE_WGP_ITEM"/. "$FINAL_SAVE_ITEM/"
            fi
        elif [ -e "$SAVE_WGP_ITEM" ]; then
            # Fichier: copier depuis .save uniquement si n'existe pas dans UserData
            if [ ! -e "$FINAL_SAVE_ITEM" ]; then
                echo "Copie des sauvegardes: $SAVE_REL_PATH"
                mkdir -p "$(dirname "$FINAL_SAVE_ITEM")"
                cp "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM"
            fi
        fi
    done < "$SAVE_FILE"
}

# Prépare les fichiers d'extra depuis .extra vers /tmp
prepare_extras() {
    local EXTRAPATH_FILE="$MOUNT_DIR/.extrapath"
    local EXTRA_WGP_DIR="$MOUNT_DIR/.extra"

    [ -f "$EXTRAPATH_FILE" ] || return 0

    mkdir -p "$EXTRA_DIR"

    while IFS= read -r EXTRA_REL_PATH; do
        [ -z "$EXTRA_REL_PATH" ] && continue

        local EXTRA_WGP_ITEM="$EXTRA_WGP_DIR/$EXTRA_REL_PATH"
        local FINAL_EXTRA_ITEM="$EXTRA_DIR/$EXTRA_REL_PATH"

        if [ -d "$EXTRA_WGP_ITEM" ]; then
            # Dossier
            mkdir -p "$FINAL_EXTRA_ITEM"
            echo "Copie des extras: $EXTRA_REL_PATH"
            cp -a "$EXTRA_WGP_ITEM"/. "$FINAL_EXTRA_ITEM/"
        elif [ -f "$EXTRA_WGP_ITEM" ]; then
            # Fichier
            mkdir -p "$(dirname "$FINAL_EXTRA_ITEM")"
            echo "Copie des extras: $EXTRA_REL_PATH"
            cp "$EXTRA_WGP_ITEM" "$FINAL_EXTRA_ITEM"
        fi
    done < "$EXTRAPATH_FILE"
}

# Lance le jeu WGP via Bottles
launch_wgp_game() {
    echo "Lancement de $WGPACK_NAME..."

    apply_padfix_setting

    if [ -n "$args" ]; then
        /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$FULL_EXE_PATH" --args " $args"
    else
        /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$FULL_EXE_PATH"
    fi

    restore_padfix_setting
}

# Fonction principale pour le mode WGP
run_wgp_mode() {
    init_wgp_variables
    mount_wgp

    # Nettoyage en cas d'interruption
    trap cleanup_wgp EXIT

    read_wgp_config
    prepare_saves
    prepare_extras
    launch_wgp_game

    # Nettoyage automatique (le trap EXIT le fera aussi)
    cleanup_wgp
}

#======================================
# Fonctions de gestion des chemins avec accents
#======================================

# Translittère les caractères accentués en ASCII
transliterate() {
    local input="$1"
    echo "$input" | iconv -f UTF-8 -t ASCII//TRANSLIT | sed 's/[^a-zA-Z0-9_-]/_/g'
}

# Crée un chemin temporaire sans accents
create_temp_path() {
    local path="$1"
    local temp_base="/tmp/game_launcher_$(date +%s)"
    local new_path="$temp_base"
    local current_path=""
    local IFS='/'

    # Parcourir tous les segments du chemin
    read -ra segments <<< "$path"
    for segment in "${segments[@]}"; do
        if [ -n "$segment" ]; then
            current_path="$current_path/$segment"
            local clean_segment
            clean_segment=$(transliterate "$segment")
            new_path="$new_path/$clean_segment"
            mkdir -p "$new_path"
        fi
    done

    # Créer un lien symbolique pour le contenu du dossier parent final
    ln -sf "$(realpath "$path")"/* "$new_path/"

    echo "$new_path"
}

# Lance le jeu en mode classique (.exe)
run_classic_mode() {
    local dirpath
    local filename

    dirpath=$(dirname "$fullpath")
    filename=$(basename "$fullpath")

    local new_fullpath="$fullpath"
    local temp_base=""

    # Vérifier si le chemin contient des accents
    if echo "$dirpath" | grep -P '[^\x00-\x7F]' > /dev/null; then
        local new_dirpath
        new_dirpath=$(create_temp_path "$dirpath")
        new_fullpath="$new_dirpath/$filename"
        temp_base=$(echo "$new_dirpath" | grep -o "/tmp/game_launcher_[0-9]*")
    fi

    # Nettoyage du dossier temporaire en cas d'interruption
    [ -n "$temp_base" ] && trap 'rm -rf "$temp_base"' EXIT

    apply_padfix_setting

    if [ -n "$args" ]; then
        /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$new_fullpath" --args " $args"
    else
        /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$new_fullpath"
    fi

    restore_padfix_setting

    # Nettoyer le dossier temporaire si créé
    [ -n "$temp_base" ] && rm -rf "$temp_base"
}

#======================================
# Fonction principale
#======================================

main() {
    parse_arguments "$@"

    # Déterminer le mode
    if [[ "$fullpath" == *.wgp ]]; then
        run_wgp_mode
    else
        run_classic_mode
    fi
}

# Lancement du script
main "$@"
