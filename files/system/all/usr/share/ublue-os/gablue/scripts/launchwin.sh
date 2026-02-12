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
exewgp_mode=false
args=""
fullpath=""
# Normaliser $HOME vers /var/home (chemin réel sur Silverblue/Kinoite)
# $HOME peut être /home/gab ou /var/home/gab selon la configuration
HOME_REAL="$(realpath "$HOME")"
WINDOWS_HOME="$HOME_REAL/Windows/UserData"
SAVES_SYMLINK="/tmp/wgp-saves"
SAVES_REAL="$WINDOWS_HOME/$USER/LocalSavesWGP"
EXTRA_SYMLINK="/tmp/wgp-extra"
EXTRA_REAL="$HOME/.cache/wgp-extra"
TEMP_SYMLINK="/tmp/wgp-temp"
TEMP_REAL="/tmp/wgp-temp"

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

# Échappe une chaîne pour être utilisée dans args de flatpak (--args)
escape_args() {
    # Remplacer les guillemets doubles par \"
    local escaped="$1"
    escaped="${escaped//\\/\\\\}"   # Échapper les backslashes d'abord
    escaped="${escaped//\"/\\\"}"    # Échapper les guillemets doubles
    echo "$escaped"
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
            --exewgp)
                exewgp_mode=true
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

    sleep 0.5
    local SYSTEM_REG="$HOME/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg"
    sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' "$SYSTEM_REG"
}

#======================================
# Fonctions de gestion des WGP
#======================================

# Initialise les variables pour le mode WGP
init_wgp_variables() {
    WGPACK_FILE="$(realpath "$fullpath")"
    GAME_INTERNAL_NAME=""

    # Lire le fichier .gamename depuis le wgp pour avoir le nom du jeu pour le montage
    local GAMENAME_CONTENT
    GAMENAME_CONTENT=$(unsquashfs -cat "$WGPACK_FILE" ".gamename" 2>/dev/null)
    if [ -n "$GAMENAME_CONTENT" ]; then
        GAME_INTERNAL_NAME="$GAMENAME_CONTENT"
        # Nettoyer les points et espaces terminaux (Wine n'aime pas)
        GAME_INTERNAL_NAME="${GAME_INTERNAL_NAME%.}"
    fi

    # Fichier .gamename absent ou vide : utiliser le nom du fichier .wgp
    if [ -z "$GAME_INTERNAL_NAME" ]; then
        GAME_INTERNAL_NAME="$(basename "$WGPACK_FILE" .wgp)"
        GAME_INTERNAL_NAME="${GAME_INTERNAL_NAME%.}"
    fi

    WGPACK_NAME="$GAME_INTERNAL_NAME"

    MOUNT_BASE="/tmp/wgpackmount"
    MOUNT_DIR="$MOUNT_BASE/$WGPACK_NAME"
    EXTRA_BASE="/tmp/wgp-extra"
    EXTRA_DIR="$EXTRA_BASE/$WGPACK_NAME"
}

# Monte le squashfs du paquet WGP
mount_wgp() {
    mkdir -p "$MOUNT_BASE"

    # Vérifier si déjà monté
    if mountpoint -q "$MOUNT_DIR"; then
        # Vérifier si un bwrap est actif (montage en cours d'utilisation)
        if ! pgrep -f "bwrap.*$(printf '%s' "$MOUNT_DIR" | sed 's/[[\.*^$()+?{|\\]/\\&/g')" > /dev/null 2>&1; then
            # Pas de bwrap actif : montage orphelin, nettoyer automatiquement
            echo "Montage orphelin détecté pour $WGPACK_NAME, nettoyage..."
            fusermount -uz "$MOUNT_DIR" 2>/dev/null
            # Continuer vers le montage normal après le nettoyage
        else
            # Le jeu tourne vraiment, demander à l'utilisateur
            local QUESTION="$WGPACK_NAME est déjà lancé.\n\nVoulez-vous arrêter l'instance en cours et relancer le jeu ?"
            local RELAUNCH=false

            if command -v kdialog &> /dev/null; then
                kdialog --warningyesno "$QUESTION" --yes-label "Oui, relancer" --no-label "Non, annuler" && RELAUNCH=true
            else
                read -p "$QUESTION (o/N): " -r
                [[ "$REPLY" =~ ^[oOyY]$ ]] && RELAUNCH=true
            fi

            if [ "$RELAUNCH" = true ]; then
                echo "Arrêt de l'instance en cours..."
                # Trouver et tuer les bwrap utilisant ce mount
                local PIDs
                PIDs=$(pgrep -f "bwrap.*$(printf '%s' "$MOUNT_DIR" | sed 's/[[\.*^$()+?{|\\]/\\&/g')" 2>/dev/null)
                if [ -n "$PIDs" ]; then
                    for pid in $PIDs; do
                        echo "Arrêt du processus $pid (bwrap)"
                        kill -9 "$pid" 2>/dev/null
                    done
                    sleep 1
                fi
                # Vérifier si toujours monté et forcer le démontage
                if mountpoint -q "$MOUNT_DIR"; then
                    fusermount -uz "$MOUNT_DIR" 2>/dev/null
                fi
            else
                error_exit "$WGPACK_NAME est déjà en cours d'exécution"
            fi
        fi
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
    # Vérifier si le mount est encore utilisé par un nouveau bwrap (nouvelle instance)
    if mountpoint -q "$MOUNT_DIR" && pgrep -f "bwrap.*$(printf '%s' "$MOUNT_DIR" | sed 's/[[\.*^$()+?{|\\]/\\&/g')" > /dev/null 2>&1; then
        # Une nouvelle instance a pris la main, ne surtout pas démonter
        echo "Une nouvelle instance de $WGPACK_NAME a pris la main, pas de démontage."
        return 0
    fi

    echo "Démontage de $WGPACK_NAME..."

    # Nettoyer les symlinks /tmp/wgp-saves et /tmp/wgp-extra
    cleanup_saves_symlink
    cleanup_extras_symlink
    cleanup_temp_symlink

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

    # Attendre un peu que le démontage se termine complètement
    sleep 0.2

    # Vérifier et forcer le démontage si encore monté
    if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        umount -f "$MOUNT_DIR" 2>/dev/null || umount -f -l "$MOUNT_DIR" 2>/dev/null
        sleep 0.1
    fi

    # Supprimer le dossier de montage (rmdir, pas rm -rf pour squashfs read-only)
    if [ -d "$MOUNT_DIR" ]; then
        rmdir "$MOUNT_DIR" 2>/dev/null
    fi
}

# Lit les fichiers de configuration du WGP
read_wgp_config() {
    # Fichier .launch
    local LAUNCH_FILE="$MOUNT_DIR/.launch"
    if [ ! -f "$LAUNCH_FILE" ]; then
        echo "Erreur: fichier .launch introuvable dans le pack" >&2
        cleanup_wgp
        exit 1
    fi

    local launch_content
    launch_content="$(cat "$LAUNCH_FILE" | tr -d '\n\r')"
    FULL_EXE_PATH="$MOUNT_DIR/$launch_content"

    # Vérifier existence (fichier ou symlink)
    if [ ! -e "$FULL_EXE_PATH" ]; then
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
    local SAVES_DIR="$SAVES_REAL/$GAME_INTERNAL_NAME"

    [ -f "$SAVE_FILE" ] || return 0

    # Vérifier si le dossier existe et contient du contenu
    if [ -d "$SAVES_DIR" ]; then
        # Dossier existe vérifier s'il a du contenu
        if [ -n "$(find "$SAVES_DIR" -mindepth 1 -maxdepth 1 2>/dev/null)" ]; then
            # Dossier a du contenu ne rien copier
            return 0
        fi
    fi
    # Dossier n'existe pas ou est vide copier tout depuis .save

    echo "Copie des sauvegardes depuis .save..."

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SAVE_WGP_ITEM="$SAVE_WGP_DIR/$SAVE_REL_PATH"
        local FINAL_SAVE_ITEM="$SAVES_DIR/$SAVE_REL_PATH"

        if [ -d "$SAVE_WGP_ITEM" ]; then
            # Copier récursivement en traitant tous les symlinks
            _copy_dir_with_symlinks "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM" "$SAVE_WGP_DIR" "$SAVES_DIR"
        elif [ -e "$SAVE_WGP_ITEM" ]; then
            mkdir -p "$(dirname "$FINAL_SAVE_ITEM")"
            if [ -L "$SAVE_WGP_ITEM" ]; then
                _copy_symlink_as_abs "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM" "$SAVE_WGP_DIR" "$SAVES_DIR"
            else
                cp -n "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM"
            fi
        fi
    done < "$SAVE_FILE"
}

# Copie un symlink en convertissant sa cible en chemin absolu
# Tous les symlinks relatifs sont convertis en absolus
# Si la cible pointe vers le mount temporaire, elle est convertie vers la destination réelle
# Usage: _copy_symlink_as_abs <src_symlink> <dst_symlink> <src_base_dir> <dst_base_dir>
_copy_symlink_as_abs() {
    local src_symlink="$1"
    local dst_symlink="$2"
    local src_base_dir="${3:-}"
    local dst_base_dir="${4:-}"

    # Lire la cible du symlink
    local target
    target=$(readlink "$src_symlink")
    [ -z "$target" ] && return 1

    # Si c'est un chemin relatif, résoudre le chemin absolu
    local abs_target
    if [[ "$target" == /* ]]; then
        abs_target="$target"
    else
        # Chemin relatif : résoudre depuis le dossier du symlink source
        abs_target=$(realpath -m "$(dirname "$src_symlink")/$target" 2>/dev/null)
    fi

    # Supprimer /.save/ ou /.extra/ ou /.temp/ du chemin s'il est présent (car on copie vers le dossier de save réel)
    if [[ "$abs_target" == */.save/* ]]; then
        abs_target=$(echo "$abs_target" | sed 's|/.save/|/|g')
    elif [[ "$abs_target" == */.extra/* ]]; then
        abs_target=$(echo "$abs_target" | sed 's|/.extra/|/|g')
    elif [[ "$abs_target" == */.temp/* ]]; then
        abs_target=$(echo "$abs_target" | sed 's|/.temp/|/|g')
    fi

    # Si la cible est dans le dossier source (.save/.extra/.temp), la convertir vers la destination réelle
    if [ -n "$src_base_dir" ] && [ -n "$dst_base_dir" ]; then
        if [[ "$abs_target" == "$src_base_dir"* ]]; then
            # La cible est dans le dossier source (.save/.extra), la convertir vers la destination
            local rel_path="${abs_target#$src_base_dir/}"
            abs_target="$dst_base_dir/$rel_path"
        fi
    fi

    # Créer toujours un symlink avec une cible absolue
    # Si abs_target est vide (erreur realpath), utiliser la cible originale
    if [ -n "$abs_target" ]; then
        ln -s "$abs_target" "$dst_symlink"
    else
        ln -s "$target" "$dst_symlink"
    fi
}

# Copie récursive un dossier en convertissant les symlinks relatifs
# Usage: _copy_dir_with_symlinks <src_dir> <dst_dir> <src_base_dir> <dst_base_dir>
_copy_dir_with_symlinks() {
    local src_dir="$1"
    local dst_dir="$2"
    local src_base_dir="${3:-}"
    local dst_base_dir="${4:-}"

    mkdir -p "$dst_dir"

    # Copier chaque élément individuellement
    for item in "$src_dir"/*; do
        [ -e "$item" ] || [ -L "$item" ] || continue  # ignorer si le glob ne matche rien
        local name
        name=$(basename "$item")
        local dst_item="$dst_dir/$name"

        if [ -L "$item" ]; then
            # C'est un symlink : le traiter avec _copy_symlink_as_abs
            _copy_symlink_as_abs "$item" "$dst_item" "$src_base_dir" "$dst_base_dir"
        elif [ -f "$item" ]; then
            # Fichier normal
            cp -n "$item" "$dst_item"
        elif [ -d "$item" ]; then
            # Dossier : traiter récursivement
            _copy_dir_with_symlinks "$item" "$dst_item" "$src_base_dir" "$dst_base_dir"
        fi
    done
}

# Copie récursive un dossier en réécrivant les symlinks externes pour pointer vers MOUNT_DIR
# Usage: _copy_dir_rewrite_symlinks <src_dir> <dst_dir>
_copy_dir_rewrite_symlinks() {
    local src_dir="$1"
    local dst_dir="$2"

    mkdir -p "$dst_dir"

    # Copier chaque élément individuellement
    for item in "$src_dir"/*; do
        [ -e "$item" ] || [ -L "$item" ] || continue  # ignorer si le glob ne matche rien
        local name
        name=$(basename "$item")
        local dst_item="$dst_dir/$name"

        if [ -L "$item" ]; then
            # C'est un symlink : lire la cible et réécrire si externe
            local target
            target=$(readlink "$item")
            local abs_target
            
            # Convertir en chemin absolu
            if [[ "$target" == /* ]]; then
                abs_target="$target"
            else
                abs_target=$(realpath -m "$(dirname "$item")/$target" 2>/dev/null)
            fi
            
            # Vérifier si la cible est externe (hors de MOUNT_DIR)
            if [[ -n "$abs_target" ]] && [[ "$abs_target" != "$MOUNT_DIR"* ]]; then
                # Cible externe : réécrire pour pointer vers MOUNT_DIR
                local game_name="$GAME_INTERNAL_NAME"
                local rewritten_target="$MOUNT_DIR"
                
                if [[ "$abs_target" == */lgp/"$game_name"/* ]]; then
                    local rel_path="${abs_target##*/lgp/$game_name/}"
                    rewritten_target="$MOUNT_DIR/$rel_path"
                elif [[ "$abs_target" == */"$game_name"/* ]]; then
                    local rel_path="${abs_target##*/$game_name/}"
                    rewritten_target="$MOUNT_DIR/$rel_path"
                fi
                
                if [ -e "$rewritten_target" ]; then
                    ln -s "$rewritten_target" "$dst_item"
                    echo "Symlink réécrit: $dst_item -> $rewritten_target"
                else
                    local real_target
                    real_target=$(realpath "$item" 2>/dev/null)
                    if [ -f "$real_target" ]; then
                        cp -n "$real_target" "$dst_item"
                    fi
                fi
            else
                ln -s "$target" "$dst_item"
            fi
        elif [ -f "$item" ]; then
            cp -n "$item" "$dst_item"
        elif [ -d "$item" ]; then
            _copy_dir_rewrite_symlinks "$item" "$dst_item"
        fi
    done
}

# Copie un fichier symlink en réécrivant la cible si externe
# Usage: _copy_symlink_rewrite <src_symlink> <dst_symlink>
_copy_symlink_rewrite() {
    local src_symlink="$1"
    local dst_symlink="$2"
    
    local target
    target=$(readlink "$src_symlink")
    local abs_target
    
    if [[ "$target" == /* ]]; then
        abs_target="$target"
    else
        abs_target=$(realpath -m "$(dirname "$src_symlink")/$target" 2>/dev/null)
    fi
    
    if [[ -n "$abs_target" ]] && [[ "$abs_target" != "$MOUNT_DIR"* ]]; then
        local game_name="$GAME_INTERNAL_NAME"
        local rewritten_target="$MOUNT_DIR"
        
        if [[ "$abs_target" == */lgp/"$game_name"/* ]]; then
            local rel_path="${abs_target##*/lgp/$game_name/}"
            rewritten_target="$MOUNT_DIR/$rel_path"
        elif [[ "$abs_target" == */"$game_name"/* ]]; then
            local rel_path="${abs_target##*/$game_name/}"
            rewritten_target="$MOUNT_DIR/$rel_path"
        fi
        
        if [ -e "$rewritten_target" ]; then
            ln -s "$rewritten_target" "$dst_symlink"
            echo "Symlink réécrit: $dst_symlink -> $rewritten_target"
        else
            local real_target
            real_target=$(realpath "$src_symlink" 2>/dev/null)
            if [ -f "$real_target" ]; then
                cp -n "$real_target" "$dst_symlink"
            fi
        fi
    else
        ln -s "$target" "$dst_symlink"
    fi
}

# Prépare les fichiers d'extra depuis .extra vers ~/.cache/wgp
prepare_extras() {
    local EXTRAPATH_FILE="$MOUNT_DIR/.extrapath"
    local EXTRA_WGP_DIR="$MOUNT_DIR/.extra"
    local EXTRA_CACHE_DIR="$EXTRA_REAL/$GAME_INTERNAL_NAME"

    [ -f "$EXTRAPATH_FILE" ] || return 0

    # Supprimer l'ancien EXTRA_DIR s'il existe et n'est pas un symlink
    if [ -d "$EXTRA_DIR" ] && [ ! -L "$EXTRA_DIR" ]; then
        rm -rf "$EXTRA_DIR"
    fi

    # Vérifier si les données existent déjà dans le cache
    if [ -d "$EXTRA_CACHE_DIR" ] && [ -n "$(find "$EXTRA_CACHE_DIR" -mindepth 1 -maxdepth 1 2>/dev/null)" ]; then
        mkdir -p "$EXTRA_BASE"
        rm -f "$EXTRA_DIR"
        ln -s "$EXTRA_CACHE_DIR" "$EXTRA_DIR"
        return 0
    fi

    echo "Copie des extras depuis .extra..."

    while IFS= read -r EXTRA_REL_PATH; do
        [ -z "$EXTRA_REL_PATH" ] && continue

        local EXTRA_WGP_ITEM="$EXTRA_WGP_DIR/$EXTRA_REL_PATH"
        local FINAL_EXTRA_ITEM="$EXTRA_CACHE_DIR/$EXTRA_REL_PATH"

        if [ -d "$EXTRA_WGP_ITEM" ]; then
            # Copier récursivement en traitant tous les symlinks
            _copy_dir_with_symlinks "$EXTRA_WGP_ITEM" "$FINAL_EXTRA_ITEM" "$EXTRA_WGP_DIR" "$EXTRA_CACHE_DIR"
        elif [ -e "$EXTRA_WGP_ITEM" ]; then
            mkdir -p "$(dirname "$FINAL_EXTRA_ITEM")"
            if [ -L "$EXTRA_WGP_ITEM" ]; then
                _copy_symlink_as_abs "$EXTRA_WGP_ITEM" "$FINAL_EXTRA_ITEM" "$EXTRA_WGP_DIR" "$EXTRA_CACHE_DIR"
            else
                cp -n "$EXTRA_WGP_ITEM" "$FINAL_EXTRA_ITEM"
            fi
        fi
    done < "$EXTRAPATH_FILE"

    mkdir -p "$EXTRA_BASE"
    rm -f "$EXTRA_DIR"
    ln -s "$EXTRA_CACHE_DIR" "$EXTRA_DIR"
}

# Prépare les fichiers temporaires depuis .temp vers /tmp/wgp-temp
prepare_temps() {
    local TEMPPATH_FILE="$MOUNT_DIR/.temppath"
    local TEMP_WGP_DIR="$MOUNT_DIR/.temp"
    local TEMP_GAME_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"

    [ -f "$TEMPPATH_FILE" ] || return 0

    echo "Préparation des fichiers temporaires..."

    # Nettoyer l'ancien dossier temporaire s'il existe
    if [ -d "$TEMP_GAME_DIR" ]; then
        rm -rf "$TEMP_GAME_DIR"
    fi

    # Créer le dossier temporaire pour ce jeu
    mkdir -p "$TEMP_GAME_DIR"

    while IFS= read -r TEMP_REL_PATH; do
        [ -z "$TEMP_REL_PATH" ] && continue

        local TEMP_WGP_ITEM="$TEMP_WGP_DIR/$TEMP_REL_PATH"
        local FINAL_TEMP_ITEM="$TEMP_GAME_DIR/$TEMP_REL_PATH"

        if [ -d "$TEMP_WGP_ITEM" ]; then
            # Copier récursivement en réécrivant les symlinks externes
            _copy_dir_rewrite_symlinks "$TEMP_WGP_ITEM" "$FINAL_TEMP_ITEM"
        elif [ -e "$TEMP_WGP_ITEM" ]; then
            mkdir -p "$(dirname "$FINAL_TEMP_ITEM")"
            if [ -L "$TEMP_WGP_ITEM" ]; then
                # C'est un symlink : réécrire si externe
                _copy_symlink_rewrite "$TEMP_WGP_ITEM" "$FINAL_TEMP_ITEM"
            else
                cp -n "$TEMP_WGP_ITEM" "$FINAL_TEMP_ITEM"
            fi
        fi
    done < "$TEMPPATH_FILE"
}

# Vérifie si le WGP contient des fichiers de sauvegarde
has_saves() {
    [ -f "$MOUNT_DIR/.savepath" ]
}

# Crée le symlink /tmp/wgp-saves/$GAME_INTERNAL_NAME vers UserData
# Un symlink par jeu permet de lancer plusieurs WGP en parallèle
setup_saves_symlink() {
    # Ne rien faire si le WGP n'a pas de saves
    has_saves || return 0

    local GAME_SAVES_DIR="$SAVES_REAL/$GAME_INTERNAL_NAME"
    local GAME_SAVES_SYMLINK="$SAVES_SYMLINK/$GAME_INTERNAL_NAME"

    # Créer le dossier de sauvegardes réel pour ce jeu
    mkdir -p "$GAME_SAVES_DIR"

    # Créer le dossier /tmp/wgp-saves si nécessaire
    mkdir -p "$SAVES_SYMLINK"

    # Supprimer l'ancien symlink du jeu s'il existe
    if [ -L "$GAME_SAVES_SYMLINK" ]; then
        rm -f "$GAME_SAVES_SYMLINK"
    fi

    # Créer le symlink pour ce jeu
    ln -s "$GAME_SAVES_DIR" "$GAME_SAVES_SYMLINK"
    echo "Symlink créé: $GAME_SAVES_SYMLINK -> $GAME_SAVES_DIR"
}

# Supprime le symlink /tmp/wgp-saves/$GAME_INTERNAL_NAME
cleanup_saves_symlink() {
    local GAME_SAVES_SYMLINK="$SAVES_SYMLINK/$GAME_INTERNAL_NAME"
    [ -L "$GAME_SAVES_SYMLINK" ] && rm -f "$GAME_SAVES_SYMLINK"
}

# Vérifie si le WGP contient des fichiers d'extra
has_extras() {
    [ -f "$MOUNT_DIR/.extrapath" ]
}

# Vérifie si le WGP contient des fichiers temporaires
has_temps() {
    [ -f "$MOUNT_DIR/.temppath" ]
}

# Crée le symlink /tmp/wgp-extra/$GAME_INTERNAL_NAME vers ~/.cache/wgp
# Un symlink par jeu permet de lancer plusieurs WGP en parallèle
setup_extras_symlink() {
    # Ne rien faire si le WGP n'a pas d'extras
    has_extras || return 0

    local GAME_EXTRAS_DIR="$EXTRA_REAL/$GAME_INTERNAL_NAME"
    local GAME_EXTRAS_SYMLINK="$EXTRA_SYMLINK/$GAME_INTERNAL_NAME"

    # Créer le dossier d'extras réel pour ce jeu
    mkdir -p "$GAME_EXTRAS_DIR"

    # Créer le dossier /tmp/wgp-extra si nécessaire
    mkdir -p "$EXTRA_SYMLINK"

    # Supprimer l'ancien symlink du jeu s'il existe
    if [ -L "$GAME_EXTRAS_SYMLINK" ]; then
        rm -f "$GAME_EXTRAS_SYMLINK"
    fi

    # Créer le symlink pour ce jeu
    ln -s "$GAME_EXTRAS_DIR" "$GAME_EXTRAS_SYMLINK"
    echo "Symlink créé: $GAME_EXTRAS_SYMLINK -> $GAME_EXTRAS_DIR"
}

# Supprime le symlink /tmp/wgp-extra/$GAME_INTERNAL_NAME
cleanup_extras_symlink() {
    local GAME_EXTRAS_SYMLINK="$EXTRA_SYMLINK/$GAME_INTERNAL_NAME"
    [ -L "$GAME_EXTRAS_SYMLINK" ] && rm -f "$GAME_EXTRAS_SYMLINK"
}

# Crée le dossier /tmp/wgp-temp/$GAME_INTERNAL_NAME pour les fichiers temporaires
# Note: Pas besoin de symlink pour les temps car ils sont déjà dans /tmp
setup_temp_symlink() {
    # Ne rien faire si le WGP n'a pas de temps
    has_temps || return 0

    local GAME_TEMP_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"

    # Créer le dossier temporaire pour ce jeu
    mkdir -p "$GAME_TEMP_DIR"
    echo "Dossier temporaire créé: $GAME_TEMP_DIR"
}

# Nettoie les fichiers temporaires /tmp/wgp-temp/$GAME_INTERNAL_NAME
cleanup_temp_symlink() {
    local GAME_TEMP_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"
    
    # Nettoyer les fichiers temporaires
    if [ -d "$GAME_TEMP_DIR" ]; then
        echo "Nettoyage des fichiers temporaires..."
        rm -rf "$GAME_TEMP_DIR"
    fi
}

# Construit et exécute la commande bottles avec ou sans mesa-git
run_bottles() {
    local exe="$1"
    local cmd_args="$2"

    # Vérifier si mesa-git est demandé
    local MESA_CONFIG="$HOME_REAL/.config/.mesa-git"
    if [ -f "$MESA_CONFIG" ]; then
        echo "Utilisation de Mesa-Git (détecté: $MESA_CONFIG)"
        FLATPAK_GL_DRIVERS=mesa-git /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$exe" --args "$cmd_args"
    else
        /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$exe" --args "$cmd_args"
    fi
}

# Lance le jeu via Bottles avec surveillance bwrap
# Usage: launch_bottles_game <chemin_exe> [args]
launch_bottles_game() {
    local exe_path="$1"
    local game_args="${2:-}"
    local display_name="${3:-$(basename "$exe_path")}"

    echo "Lancement de $display_name..."

    # Définir TERM qui est nécessaire pour certains jeux Unity
    [ -z "$TERM" ] && export TERM=linux

    apply_padfix_setting

    # Lancer en arrière-plan et surveiller
    local FLATPAK_PID
    if [ -n "$game_args" ]; then
        run_bottles "$exe_path" " $game_args" &
    else
        run_bottles "$exe_path" "" &
    fi
    FLATPAK_PID=$!

    echo "En attente de la fermeture du jeu..."

    # Attendre le processus flatpak (bloquant, 0% CPU)
    wait "$FLATPAK_PID" 2>/dev/null

    # Vérification rapide de sécurité : attendre que bwrap se termine vraiment
    local bwrap_pattern
    bwrap_pattern=$(printf '%s' "$exe_path" | sed 's/[[\.*^$()+?{|\\]/\\&/g')
    local timeout=50  # 5 secondes max (50 * 0.1s)
    while [ $timeout -gt 0 ] && pgrep -f "bwrap.*$bwrap_pattern" > /dev/null 2>&1; do
        sleep 0.1
        ((timeout--))
    done

    restore_padfix_setting
}

# Configure le symlink ProgramData via fichier .pds
setup_pds_symlink() {
    local exe_dir="$1"
    local PDS_FILE="$exe_dir/.pds"

    [ -f "$PDS_FILE" ] || return 0

    local game_name
    game_name=$(cat "$PDS_FILE" 2>/dev/null)
    [ -n "$game_name" ] || return 0

    echo "Configuration ProgramData pour: $game_name"

    local progdata_saves_dir="$WINDOWS_HOME/$USER/ProgramDataSaves/$game_name"
    local progdata_symlink="$HOME_REAL/Windows/WinDrive/ProgramData/$game_name"

    # Créer le dossier de sauvegardes s'il n'existe pas
    if [ ! -d "$progdata_saves_dir" ]; then
        mkdir -p "$progdata_saves_dir"
        echo "Dossier créé: $progdata_saves_dir"
    fi

    # Créer le dossier parent du symlink si nécessaire
    mkdir -p "$(dirname "$progdata_symlink")"

    # Vérifier s'il existe déjà quelque chose à cet emplacement
    if [ -e "$progdata_symlink" ]; then
        if [ -d "$progdata_symlink" ] && [ ! -L "$progdata_symlink" ]; then
            # Un vrai dossier existe déjà : ne rien faire
            echo "Dossier existant dans ProgramData: $progdata_symlink (pas de modification)"
            return 0
        elif [ -L "$progdata_symlink" ]; then
            # Un symlink existe déjà : vérifier s'il pointe vers la bonne destination
            local current_target
            current_target=$(readlink "$progdata_symlink")
            if [ "$current_target" = "$progdata_saves_dir" ]; then
                echo "Symlink déjà existant et correct: $progdata_symlink -> $progdata_saves_dir"
                return 0
            else
                # Symlink existant mais pointe ailleurs : le supprimer
                echo "Symlink existant mais incorrect, remplacement..."
                rm -f "$progdata_symlink"
            fi
        fi
    fi

    # Créer le symlink
    ln -s "$progdata_saves_dir" "$progdata_symlink"
    echo "Symlink créé: $progdata_symlink -> $progdata_saves_dir"
}

# Installe les fichiers .reg trouvés dans un dossier via regedit.exe
install_registry_files() {
    local reg_dir="$1"
    local reg_files

    # Résoudre le chemin réel (suivre les symlinks) pour find
    local real_reg_dir
    real_reg_dir="$(realpath "$reg_dir" 2>/dev/null || echo "$reg_dir")"

    # Chercher les fichiers .reg dans le dossier
    reg_files=()
    while IFS= read -r -d '' file; do
        reg_files+=("$file")
    done < <(find "$real_reg_dir" -maxdepth 1 -name '*.reg' -print0 2>/dev/null)

    # Si aucun fichier .reg, rien à faire
    [ ${#reg_files[@]} -eq 0 ] && return 0

    # Exécuter chaque fichier .reg individuellement
    # Solution: copier dans C:\windows\temp\ pour éviter les problèmes de chemins avec espaces
    local bottle_c="$HOME_REAL/.var/app/com.usebottles.bottles/data/bottles/bottles/def/drive_c"
    local temp_dir="$bottle_c/windows/temp"
    mkdir -p "$temp_dir"

    for reg_file in "${reg_files[@]}"; do
        local reg_name
        reg_name=$(basename "$reg_file")
        
        # Calculer le hash md5 du contenu (rapide)
        local reg_hash
        reg_hash=$(md5sum "$reg_file" | cut -d' ' -f1)
        
        # Nom du fichier dans temp: hash_nomdufichier.reg
        # Cela permet d'avoir le même fichier de différents jeux sans collision
        local dest_file="$temp_dir/${reg_hash}_${reg_name}"
        
        # Vérifier si le fichier existe déjà avec le même contenu
        if [ -f "$dest_file" ]; then
            # Vérifier que le hash correspond bien (sécurité)
            local existing_hash
            existing_hash=$(md5sum "$dest_file" | cut -d' ' -f1)
            if [ "$reg_hash" = "$existing_hash" ]; then
                echo "Fichier .reg déjà installé (ignoré): $reg_name"
                continue
            fi
        fi
        
        echo "Installation du fichier de registre: $reg_name"

        # Copier le fichier dans C:\windows\temp\ avec le hash dans le nom
        cp "$reg_file" "$dest_file"

        # Exécuter avec chemin Windows simple (C:\windows\temp\...)
        run_bottles "$HOME_REAL/Windows/WinDrive/windows/regedit.exe" "/S C:\\\\windows\\\\temp\\\\$(basename "$dest_file")"
        
        # Le fichier est conservé dans temp pour les prochains lancements
    done
}

# Affiche un menu interactif pour choisir un .exe dans le WGP (mode --exewgp)
select_exe_from_wgp() {
    echo "Recherche des exécutables dans le pack..."

    # Lister les fichiers .exe dans le pack
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

    # Construire le menu kdialog
    local menu_args=("Choisissez un exécutable à lancer :")

    for exe in "${exe_array[@]}"; do
        local rel_path="${exe#$MOUNT_DIR/}"
        menu_args+=("$rel_path" "$rel_path")
    done

    # Afficher le menu
    local EXE_REL_PATH
    EXE_REL_PATH=$(kdialog --menu "${menu_args[@]}")
    local exit_status=$?

    if [ $exit_status -ne 0 ] || [ -z "$EXE_REL_PATH" ]; then
        echo "Annulé par l'utilisateur"
        cleanup_wgp
        exit 0
    fi

    # Chemin complet de l'exécutable
    FULL_EXE_PATH="$MOUNT_DIR/$EXE_REL_PATH"

    if [ ! -f "$FULL_EXE_PATH" ]; then
        echo "Erreur: exécutable introuvable: $FULL_EXE_PATH"
        cleanup_wgp
        exit 1
    fi

    echo "Exécutable sélectionné: $EXE_REL_PATH"
}

# Fonction principale pour le mode WGP
run_wgp_mode() {
    init_wgp_variables
    mount_wgp

    # Nettoyage en cas d'interruption
    trap cleanup_wgp EXIT

    # Créer le symlink /tmp/wgp-saves AVANT prepare_saves
    setup_saves_symlink

    # Créer le symlink /tmp/wgp-extra AVANT prepare_extras
    setup_extras_symlink

    # Créer le symlink /tmp/wgp-temp AVANT prepare_temps
    setup_temp_symlink

    # IMPORTANT: prepare_saves AVANT read_wgp_config (l'exécutable peut être un symlink vers UserData)
    prepare_saves
    prepare_extras
    prepare_temps

    # Mode --exewgp : choisir l'exécutable interactif, sinon lire depuis .launch
    if [ "$exewgp_mode" = true ]; then
        select_exe_from_wgp
    else
        read_wgp_config
    fi

    # Configurer le symlink ProgramData si fichier .pds présent
    setup_pds_symlink "$(dirname "$FULL_EXE_PATH")"
    # Installer les fichiers .reg dans le dossier de l'exécutable
    # Gérer le cas où l'exe est un symlink : chercher dans les deux dossiers
    local exe_dir="$(dirname "$FULL_EXE_PATH")"
    install_registry_files "$exe_dir"
    # Si c'est un symlink, chercher aussi dans le dossier cible
    if [ -L "$FULL_EXE_PATH" ]; then
        local real_exe_path
        real_exe_path="$(realpath "$FULL_EXE_PATH")"
        local real_exe_dir="$(dirname "$real_exe_path")"
        if [ "$real_exe_dir" != "$exe_dir" ]; then
            install_registry_files "$real_exe_dir"
        fi
    fi

    # Lancer le jeu avec surveillance bwrap
    launch_bottles_game "$FULL_EXE_PATH" "$args" "$WGPACK_NAME"

    # Nettoyage des symlinks saves et extras (le trap fera le reste)
    cleanup_saves_symlink
    cleanup_extras_symlink
    cleanup_temp_symlink
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

    # Créer des liens symboliques pour tout le contenu du dossier
    local real_path
    real_path="$(realpath "$path")"
    for item in "$real_path"/* "$real_path"/.*; do
        # Ignorer . et ..
        [[ "$(basename "$item")" == "." ]] && continue
        [[ "$(basename "$item")" == ".." ]] && continue
        [ -e "$item" ] || [ -L "$item" ] && ln -sf "$item" "$new_path/" 2>/dev/null
    done

    echo "$new_path"
}

# Lance le jeu en mode classique (.exe direct)
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

    # Configurer le symlink ProgramData si fichier .pds présent
    setup_pds_symlink "$(dirname "$new_fullpath")"
    # Installer les fichiers .reg dans le dossier de l'exécutable
    # Gérer le cas où l'exe est un symlink : chercher dans les deux dossiers
    local exe_dir="$(dirname "$new_fullpath")"
    install_registry_files "$exe_dir"
    # Si c'est un symlink, chercher aussi dans le dossier cible
    if [ -L "$new_fullpath" ]; then
        local real_exe_path
        real_exe_path="$(realpath "$new_fullpath")"
        local real_exe_dir="$(dirname "$real_exe_path")"
        if [ "$real_exe_dir" != "$exe_dir" ]; then
            install_registry_files "$real_exe_dir"
        fi
    fi

    # Lancer le jeu avec surveillance bwrap
    launch_bottles_game "$new_fullpath" "$args" "$filename"

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
