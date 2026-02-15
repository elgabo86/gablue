#!/bin/bash

################################################################################
# launchlin.sh - Script de lancement de jeux Linux
#
# Ce script permet de lancer des jeux/applications Linux via des paquets LGP
# compressés, avec support :
# - Des paquets LGP (.lgp) compressés en squashfs (montage kernel natif)
# - Des exécutables directs (binaires ELF, AppImages, scripts .sh/.py)
# - Script de pré-lancement .script.sh
# - Overlayfs kernel natif pour les fichiers temporaires (performances optimales)
#
# NOTE: Utilise mount -t squashfs et mount -t overlay kernel natif
# au lieu de squashfuse + fuse-overlayfs pour de meilleures performances
################################################################################

#======================================
# Variables globales
#======================================
args=""
fullpath=""
# Normaliser $HOME vers /var/home (chemin réel sur Silverblue/Kinoite)
HOME_REAL="$(realpath "$HOME")"
SAVES_SYMLINK="/tmp/lgp-saves"
SAVES_REAL="$HOME_REAL/.local/share/lgp-saves"
EXTRA_SYMLINK="/tmp/lgp-extra"
EXTRA_REAL="$HOME_REAL/.cache/lgp-extra"
TEMP_SYMLINK="/tmp/lgp-temp"
TEMP_REAL="/tmp/lgp-temp"

# Variables LGP
LGPACK_NAME=""
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
# Fonctions de gestion des LGP
#======================================

# Initialise les variables pour le mode LGP
init_lgp_variables() {
    LGPACK_FILE="$(realpath "$fullpath")"
    GAME_INTERNAL_NAME=""

    # Lire le fichier .gamename depuis le lgp pour avoir le nom du jeu pour le montage
    local GAMENAME_CONTENT
    GAMENAME_CONTENT=$(unsquashfs -cat "$LGPACK_FILE" ".gamename" 2>/dev/null)
    if [ -n "$GAMENAME_CONTENT" ]; then
        GAME_INTERNAL_NAME="$GAMENAME_CONTENT"
        # Nettoyer les points et espaces terminaux
        GAME_INTERNAL_NAME="${GAME_INTERNAL_NAME%.}"
    fi

    # Fichier .gamename absent ou vide : utiliser le nom du fichier .lgp
    if [ -z "$GAME_INTERNAL_NAME" ]; then
        GAME_INTERNAL_NAME="$(basename "$LGPACK_FILE" .lgp)"
        GAME_INTERNAL_NAME="${GAME_INTERNAL_NAME%.}"
    fi

    LGPACK_NAME="$GAME_INTERNAL_NAME"

    MOUNT_BASE="/tmp/lgpackmount"
    MOUNT_DIR="$MOUNT_BASE/$LGPACK_NAME"
    EXTRA_BASE="/tmp/lgp-extra"
    EXTRA_DIR="$EXTRA_BASE/$LGPACK_NAME"
}

# Monte le squashfs du paquet LGP
mount_lgp() {
    mkdir -p "$MOUNT_BASE"

    # Vérifier si déjà monté
    if mountpoint -q "$MOUNT_DIR"; then
        # Vérifier si un processus utilise encore le montage
        if ! lsof +D "$MOUNT_DIR" > /dev/null 2>&1; then
            # Pas de processus actif : montage orphelin, nettoyer automatiquement
            echo "Montage orphelin détecté pour $LGPACK_NAME, nettoyage..."
            sudo umount -f "$MOUNT_DIR" 2>/dev/null || sudo umount -f -l "$MOUNT_DIR" 2>/dev/null || true
        else
            # Le jeu tourne vraiment, demander à l'utilisateur
            local QUESTION="$LGPACK_NAME est déjà lancé.\n\nVoulez-vous arrêter l'instance en cours et relancer le jeu ?"
            local RELAUNCH=false

            if command -v kdialog &> /dev/null; then
                kdialog --warningyesno "$QUESTION" --yes-label "Oui, relancer" --no-label "Non, annuler" && RELAUNCH=true
            else
                read -p "$QUESTION (o/N): " -r
                [[ "$REPLY" =~ ^[oOyY]$ ]] && RELAUNCH=true
            fi

            if [ "$RELAUNCH" = true ]; then
                echo "Arrêt de l'instance en cours..."
                # Trouver et tuer les processus utilisant ce mount
                local PIDs
                PIDs=$(lsof +D "$MOUNT_DIR" 2>/dev/null | awk 'NR>1 {print $2}' | sort -u)
                if [ -n "$PIDs" ]; then
                    for pid in $PIDs; do
                        echo "Arrêt du processus $pid"
                        kill -9 "$pid" 2>/dev/null
                    done
                    sleep 1
                fi
                # Vérifier si toujours monté et forcer le démontage
                if mountpoint -q "$MOUNT_DIR"; then
                    sudo umount -f "$MOUNT_DIR" 2>/dev/null || sudo umount -f -l "$MOUNT_DIR" 2>/dev/null || true
                fi
            else
                error_exit "$LGPACK_NAME est déjà en cours d'exécution"
            fi
        fi
    fi

    # Si le dossier existe mais n'est pas monté et n'est pas vide, le nettoyer
    if [ -d "$MOUNT_DIR" ] && ! mountpoint -q "$MOUNT_DIR"; then
        if [ -n "$(ls -A "$MOUNT_DIR" 2>/dev/null)" ]; then
            echo "Dossier de montage existant et non vide détecté, nettoyage..."
            rm -rf "$MOUNT_DIR"/* "$MOUNT_DIR"/.* 2>/dev/null || true
        fi
        # Supprimer le dossier vide
        rmdir "$MOUNT_DIR" 2>/dev/null || rm -rf "$MOUNT_DIR" 2>/dev/null || true
    fi

    # Vérifier que mount est disponible
    if ! command -v mount &> /dev/null; then
        error_exit "mount n'est pas disponible"
    fi

    # Créer et monter le squashfs via le kernel (loopback)
    mkdir -p "$MOUNT_DIR"
    echo "Montage de $LGPACK_FILE sur $MOUNT_DIR (kernel squashfs)..."
    
    # Utiliser sudo pour monter le squashfs en tant que root
    # L'option loop permet d'utiliser un fichier comme périphérique bloc
    if ! sudo mount -t squashfs -o ro,nodev,nosuid "$LGPACK_FILE" "$MOUNT_DIR" 2>&1; then
        rmdir "$MOUNT_DIR" 2>/dev/null || true
        error_exit "Erreur lors du montage du squashfs kernel"
    fi
    
    echo "Squashfs monté avec succès via le kernel"
}

# Nettoie en démontant le LGP et les extras
cleanup_lgp() {
    # Vérifier si le mount est encore utilisé par un nouveau processus
    if mountpoint -q "$MOUNT_DIR" && lsof +D "$MOUNT_DIR" > /dev/null 2>&1; then
        # Une nouvelle instance a pris la main, ne surtout pas démonter
        echo "Une nouvelle instance de $LGPACK_NAME a pris la main, pas de démontage."
        return 0
    fi

    echo "Démontage de $LGPACK_NAME..."

    # Nettoyer les symlinks /tmp/lgp-saves et /tmp/lgp-extra
    cleanup_saves_symlink
    cleanup_extras_symlink
    cleanup_temp_symlink

    # Démontage du squashfs kernel natif
    if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        # Utiliser sudo pour démonter (le mount kernel nécessite root)
        if ! sudo umount "$MOUNT_DIR" 2>/dev/null; then
            # Si échec, force unmount lazy
            sudo umount -f -l "$MOUNT_DIR" 2>/dev/null || true
        fi
        
        # Attendre un peu que le démontage se termine
        sleep 0.2
        
        # Vérifier si encore monté
        if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
            sudo umount -f "$MOUNT_DIR" 2>/dev/null || sudo umount -f -l "$MOUNT_DIR" 2>/dev/null || true
        fi
    fi

    # Supprimer le dossier de montage
    if [ -d "$MOUNT_DIR" ]; then
        rmdir "$MOUNT_DIR" 2>/dev/null || true
    fi
}

# Lit les fichiers de configuration du LGP
read_lgp_config() {
    # Fichier .launch
    local LAUNCH_FILE="$MOUNT_DIR/.launch"
    if [ ! -f "$LAUNCH_FILE" ]; then
        echo "Erreur: fichier .launch introuvable dans le pack" >&2
        cleanup_lgp
        exit 1
    fi

    local launch_content
    launch_content="$(cat "$LAUNCH_FILE" | tr -d '\n\r')"
    FULL_EXE_PATH="$MOUNT_DIR/$launch_content"

    # Vérifier existence (fichier ou symlink - même cassé car .script.sh peut le réparer)
    if [ ! -e "$FULL_EXE_PATH" ] && [ ! -L "$FULL_EXE_PATH" ]; then
        echo "Erreur: exécutable introuvable: $(cat "$LAUNCH_FILE")" >&2
        cleanup_lgp
        exit 1
    fi

    # Si c'est un symlink, résoudre le chemin réel
    if [ -L "$FULL_EXE_PATH" ]; then
        REAL_EXE_PATH="$(realpath "$FULL_EXE_PATH")"
        echo "Symlink détecté: $FULL_EXE_PATH -> $REAL_EXE_PATH"
    else
        REAL_EXE_PATH="$FULL_EXE_PATH"
    fi

    # Fichier .args (surcharge les arguments en ligne de commande)
    local ARGS_FILE="$MOUNT_DIR/.args"
    if [ -f "$ARGS_FILE" ]; then
        local lgp_args
        lgp_args=$(cat "$ARGS_FILE")
        if [ -n "$lgp_args" ]; then
            args="$lgp_args"
        fi
    fi
}

# Prépare les sauvegardes depuis UserData
prepare_saves() {
    local SAVE_FILE="$MOUNT_DIR/.savepath"
    local SAVE_LGP_DIR="$MOUNT_DIR/.save"
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

        local SAVE_LGP_ITEM="$SAVE_LGP_DIR/$SAVE_REL_PATH"
        local FINAL_SAVE_ITEM="$SAVES_DIR/$SAVE_REL_PATH"

        if [ -d "$SAVE_LGP_ITEM" ]; then
            # Copier récursivement en traitant tous les symlinks
            _copy_dir_with_symlinks "$SAVE_LGP_ITEM" "$FINAL_SAVE_ITEM" "$SAVE_LGP_DIR" "$SAVES_DIR"
        elif [ -e "$SAVE_LGP_ITEM" ]; then
            mkdir -p "$(dirname "$FINAL_SAVE_ITEM")"
            if [ -L "$SAVE_LGP_ITEM" ]; then
                _copy_symlink_as_abs "$SAVE_LGP_ITEM" "$FINAL_SAVE_ITEM" "$SAVE_LGP_DIR" "$SAVES_DIR"
            else
                cp -n "$SAVE_LGP_ITEM" "$FINAL_SAVE_ITEM"
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
            # La cible est dans le dossier source, la convertir vers la destination
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
                # Extraire le nom du jeu du chemin externe (dernier composant avant le sous-dossier)
                # Ex: /run/media/.../lgp/DRAGON QUEST VII Reimagined/registered-user/00000002
                # Devient: /tmp/lgpackmount/DRAGON QUEST VII Reimagined/registered-user/00000002
                local game_name="$GAME_INTERNAL_NAME"
                local rewritten_target="$MOUNT_DIR"
                
                # Chercher le nom du jeu dans le chemin externe et récupérer tout après
                if [[ "$abs_target" == */lgp/"$game_name"/* ]]; then
                    # Extraire la partie après le nom du jeu
                    local rel_path="${abs_target##*/lgp/$game_name/}"
                    rewritten_target="$MOUNT_DIR/$rel_path"
                elif [[ "$abs_target" == */"$game_name"/* ]]; then
                    # Fallback: chercher juste le nom du jeu
                    local rel_path="${abs_target##*/$game_name/}"
                    rewritten_target="$MOUNT_DIR/$rel_path"
                fi
                
                # Vérifier que la cible réécrite existe dans le mount
                if [ -e "$rewritten_target" ]; then
                    ln -s "$rewritten_target" "$dst_item"
                    echo "Symlink réécrit: $dst_item -> $rewritten_target"
                else
                    # La cible n'existe pas dans le mount, copier le contenu
                    local real_target
                    real_target=$(realpath "$item" 2>/dev/null)
                    if [ -f "$real_target" ]; then
                        cp -n "$real_target" "$dst_item"
                    fi
                fi
            else
                # Cible interne au mount, copier le symlink tel quel
                ln -s "$target" "$dst_item"
            fi
        elif [ -f "$item" ]; then
            # Fichier normal
            cp -n "$item" "$dst_item"
        elif [ -d "$item" ]; then
            # Dossier : traiter récursivement
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
    
    # Convertir en chemin absolu
    if [[ "$target" == /* ]]; then
        abs_target="$target"
    else
        abs_target=$(realpath -m "$(dirname "$src_symlink")/$target" 2>/dev/null)
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
        
        # Vérifier que la cible réécrite existe
        if [ -e "$rewritten_target" ]; then
            ln -s "$rewritten_target" "$dst_symlink"
            echo "Symlink réécrit: $dst_symlink -> $rewritten_target"
        else
            # Copier le contenu
            local real_target
            real_target=$(realpath "$src_symlink" 2>/dev/null)
            if [ -f "$real_target" ]; then
                cp -n "$real_target" "$dst_symlink"
            fi
        fi
    else
        # Cible interne, copier tel quel
        ln -s "$target" "$dst_symlink"
    fi
}

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

        local EXTRA_LGP_ITEM="$EXTRA_LGP_DIR/$EXTRA_REL_PATH"
        local FINAL_EXTRA_ITEM="$EXTRA_CACHE_DIR/$EXTRA_REL_PATH"

        if [ -d "$EXTRA_LGP_ITEM" ]; then
            # Copier récursivement en traitant tous les symlinks
            _copy_dir_with_symlinks "$EXTRA_LGP_ITEM" "$FINAL_EXTRA_ITEM" "$EXTRA_LGP_DIR" "$EXTRA_CACHE_DIR"
        elif [ -e "$EXTRA_LGP_ITEM" ]; then
            mkdir -p "$(dirname "$FINAL_EXTRA_ITEM")"
            if [ -L "$EXTRA_LGP_ITEM" ]; then
                _copy_symlink_as_abs "$EXTRA_LGP_ITEM" "$FINAL_EXTRA_ITEM" "$EXTRA_LGP_DIR" "$EXTRA_CACHE_DIR"
            else
                cp -n "$EXTRA_LGP_ITEM" "$FINAL_EXTRA_ITEM"
            fi
        fi
    done < "$EXTRAPATH_FILE"

    mkdir -p "$EXTRA_BASE"
    rm -f "$EXTRA_DIR"
    ln -s "$EXTRA_CACHE_DIR" "$EXTRA_DIR"
}

# Monte l'overlayfs pour les fichiers temporaires
# lowerdir = .temp (lecture seule depuis le LGP)
# upperdir = /tmp/lgp-temp-upper (couche d'écriture)
# workdir = /tmp/lgp-temp-work (dossier de travail overlayfs)
prepare_temps() {
    local TEMPPATH_FILE="$MOUNT_DIR/.temppath"
    local TEMP_LGP_DIR="$MOUNT_DIR/.temp"
    local TEMP_GAME_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"
    # Utiliser l'ID sans espaces stocké par setup_temp_symlink
    local GAME_ID="${_GAME_TEMP_ID:-lgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')}"
    local TEMP_UPPER="/tmp/lgp-temp-upper/$GAME_ID"
    local TEMP_WORK="/tmp/lgp-temp-work/$GAME_ID"

    [ -f "$TEMPPATH_FILE" ] || return 0

    # Vérifier que le dossier .temp existe dans le LGP
    if [ ! -d "$TEMP_LGP_DIR" ]; then
        echo "Dossier .temp non trouvé dans le LGP, skip overlay"
        return 0
    fi

    echo "Montage de l'overlayfs pour les fichiers temporaires..."

    # Vérifier que les dossiers existent
    if [ ! -d "$TEMP_GAME_DIR" ] || [ ! -d "$TEMP_UPPER" ] || [ ! -d "$TEMP_WORK" ]; then
        error_exit "Dossiers overlay manquants pour les fichiers temporaires"
    fi

    # Vérifier que le point de montage est vide (requis par overlayfs)
    if [ -n "$(ls -A "$TEMP_GAME_DIR" 2>/dev/null)" ]; then
        echo "Nettoyage du point de montage non vide..."
        rm -rf "$TEMP_GAME_DIR"/* "$TEMP_GAME_DIR"/.* 2>/dev/null || true
    fi

    # Vérifier que le point de montage n'est pas déjà utilisé
    if mountpoint -q "$TEMP_GAME_DIR" 2>/dev/null; then
        echo "Démontage de l'overlay existant..."
        sudo umount "$TEMP_GAME_DIR" 2>/dev/null || true
    fi

    # S'assurer que le lowerdir est accessible
    if [ ! -r "$TEMP_LGP_DIR" ]; then
        error_exit "Dossier .temp inaccessible en lecture"
    fi

    # Funionfs (unionfs-fuse) - union filesystem en userspace
    # Fonctionne avec squashfs et ne fait pas de copy-up complet au open()
    # Syntaxe: funionfs <upperdir> <mountpoint> -o dirs=<lowerdir>=ro
    # upperdir = premier argument positionnel (rw par défaut)
    # lowerdir = spécifié dans l'option dirs= avec =ro
    echo "Montage funionfs (unionfs-fuse)..."
    echo "  lowerdir: $TEMP_LGP_DIR"
    echo "  upperdir: $TEMP_UPPER"
    
    # Vérifier que funionfs est disponible
    if ! command -v funionfs &> /dev/null; then
        error_exit "funionfs n'est pas installé (Installation: sudo dnf5 install funionfs)"
    fi
    
    # Utiliser funionfs avec copy-on-write
    # upperdir en premier argument (rw), lowerdir dans dirs= avec =ro
    if ! funionfs "${TEMP_UPPER}" "${TEMP_GAME_DIR}" -o "dirs=${TEMP_LGP_DIR}=ro,delete=all" 2>&1; then
        error_exit "Échec du montage funionfs pour les fichiers temporaires"
    fi

    echo "Funionfs monté avec succès: $TEMP_GAME_DIR"
}

# Vérifie si le LGP contient des fichiers de sauvegarde
has_saves() {
    [ -f "$MOUNT_DIR/.savepath" ]
}

# Crée le symlink /tmp/lgp-saves/$GAME_INTERNAL_NAME vers ~/.local/share/lgp-saves
# Un symlink par jeu permet de lancer plusieurs LGP en parallèle
setup_saves_symlink() {
    # Ne rien faire si le LGP n'a pas de saves
    has_saves || return 0

    local GAME_SAVES_DIR="$SAVES_REAL/$GAME_INTERNAL_NAME"
    local GAME_SAVES_SYMLINK="$SAVES_SYMLINK/$GAME_INTERNAL_NAME"

    # Créer le dossier de sauvegardes réel pour ce jeu
    mkdir -p "$GAME_SAVES_DIR"

    # Créer le dossier /tmp/lgp-saves si nécessaire
    mkdir -p "$SAVES_SYMLINK"

    # Supprimer l'ancien symlink du jeu s'il existe
    if [ -L "$GAME_SAVES_SYMLINK" ]; then
        rm -f "$GAME_SAVES_SYMLINK"
    fi

    # Créer le symlink pour ce jeu
    ln -s "$GAME_SAVES_DIR" "$GAME_SAVES_SYMLINK"
    echo "Symlink créé: $GAME_SAVES_SYMLINK -> $GAME_SAVES_DIR"
}

# Supprime le symlink /tmp/lgp-saves/$GAME_INTERNAL_NAME
cleanup_saves_symlink() {
    local GAME_SAVES_SYMLINK="$SAVES_SYMLINK/$GAME_INTERNAL_NAME"
    [ -L "$GAME_SAVES_SYMLINK" ] && rm -f "$GAME_SAVES_SYMLINK"
}

# Vérifie si le LGP contient des fichiers d'extra
has_extras() {
    [ -f "$MOUNT_DIR/.extrapath" ]
}

# Vérifie si le LGP contient des fichiers temporaires
has_temps() {
    [ -f "$MOUNT_DIR/.temppath" ]
}

# Crée le symlink /tmp/lgp-extra/$GAME_INTERNAL_NAME vers ~/.cache/lgp-extra
# Un symlink par jeu permet de lancer plusieurs LGP en parallèle
setup_extras_symlink() {
    # Ne rien faire si le LGP n'a pas d'extras
    has_extras || return 0

    local GAME_EXTRAS_DIR="$EXTRA_REAL/$GAME_INTERNAL_NAME"
    local GAME_EXTRAS_SYMLINK="$EXTRA_SYMLINK/$GAME_INTERNAL_NAME"

    # Créer le dossier d'extras réel pour ce jeu
    mkdir -p "$GAME_EXTRAS_DIR"

    # Créer le dossier /tmp/lgp-extra si nécessaire
    mkdir -p "$EXTRA_SYMLINK"

    # Supprimer l'ancien symlink du jeu s'il existe
    if [ -L "$GAME_EXTRAS_SYMLINK" ]; then
        rm -f "$GAME_EXTRAS_SYMLINK"
    fi

    # Créer le symlink pour ce jeu
    ln -s "$GAME_EXTRAS_DIR" "$GAME_EXTRAS_SYMLINK"
    echo "Symlink créé: $GAME_EXTRAS_SYMLINK -> $GAME_EXTRAS_DIR"
}

# Supprime le symlink /tmp/lgp-extra/$GAME_INTERNAL_NAME
cleanup_extras_symlink() {
    local GAME_EXTRAS_SYMLINK="$EXTRA_SYMLINK/$GAME_INTERNAL_NAME"
    [ -L "$GAME_EXTRAS_SYMLINK" ] && rm -f "$GAME_EXTRAS_SYMLINK"
}

# Prépare les dossiers pour le montage overlay des fichiers temporaires
# Crée upperdir et workdir pour l'overlayfs
setup_temp_symlink() {
    # Ne rien faire si le LGP n'a pas de temps
    has_temps || return 0

    # Utiliser un ID unique sans espaces pour les chemins de travail
    local GAME_ID="lgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')"
    local GAME_TEMP_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"
    local GAME_TEMP_UPPER="/tmp/lgp-temp-upper/$GAME_ID"
    local GAME_TEMP_WORK="/tmp/lgp-temp-work/$GAME_ID"
    
    # Stocker les chemins réels pour le nettoyage
    export _GAME_TEMP_ID="$GAME_ID"

    # Nettoyer les anciens dossiers s'ils existent
    if [ -d "$GAME_TEMP_UPPER" ]; then
        rm -rf "$GAME_TEMP_UPPER"
    fi
    if [ -d "$GAME_TEMP_WORK" ]; then
        rm -rf "$GAME_TEMP_WORK"
    fi

    # Créer les dossiers pour l'overlay avec permissions utilisateur
    mkdir -p "$GAME_TEMP_DIR"
    mkdir -p "$GAME_TEMP_UPPER"
    mkdir -p "$GAME_TEMP_WORK"
    
    # Définir les permissions pour l'overlay (l'utilisateur doit pouvoir écrire)
    chmod 777 "$GAME_TEMP_UPPER"
    chmod 777 "$GAME_TEMP_WORK"
    
    echo "Dossiers overlay préparés pour: $GAME_TEMP_DIR"
    echo "  upperdir: $GAME_TEMP_UPPER"
    echo "  workdir: $GAME_TEMP_WORK"
}

# Démonte l'overlayfs et nettoie les dossiers temporaires
cleanup_temp_symlink() {
    local GAME_TEMP_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"
    # Utiliser l'ID sans espaces stocké par setup_temp_symlink
    local GAME_ID="${_GAME_TEMP_ID:-lgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')}" 
    local GAME_TEMP_UPPER="/tmp/lgp-temp-upper/$GAME_ID"
    local GAME_TEMP_WORK="/tmp/lgp-temp-work/$GAME_ID"
    
    # Démonter funionfs si monté
    if mountpoint -q "$GAME_TEMP_DIR" 2>/dev/null; then
        echo "Démontage de funionfs..."
        if ! fusermount -u "$GAME_TEMP_DIR" 2>/dev/null; then
            # Si fusermount échoue, essayer umount
            sudo umount "$GAME_TEMP_DIR" 2>/dev/null || sudo umount -f "$GAME_TEMP_DIR" 2>/dev/null || true
        fi
    fi
    
    # Nettoyer les dossiers temporaires
    if [ -d "$GAME_TEMP_DIR" ]; then
        echo "Suppression du point de montage..."
        rm -rf "$GAME_TEMP_DIR"
    fi
    
    if [ -d "$GAME_TEMP_UPPER" ]; then
        echo "Suppression du dossier upperdir..."
        rm -rf "$GAME_TEMP_UPPER"
    fi
    
    if [ -d "$GAME_TEMP_WORK" ]; then
        echo "Suppression du dossier workdir..."
        rm -rf "$GAME_TEMP_WORK"
    fi
}

# Exécute le script de pré-lancement s'il existe
execute_prelaunch_script() {
    local SCRIPT_FILE="$MOUNT_DIR/.script.sh"
    
    if [ -f "$SCRIPT_FILE" ]; then
        echo "Exécution du script de pré-lancement..."
        # Rendre le script exécutable si nécessaire
        chmod +x "$SCRIPT_FILE" 2>/dev/null
        # Exécuter le script dans le dossier du jeu
        (cd "$MOUNT_DIR" && "$SCRIPT_FILE")
        local script_exit=$?
        if [ $script_exit -ne 0 ]; then
            echo "Avertissement: le script de pré-lancement a retourné le code $script_exit"
        fi
    fi
}

# Lance le jeu de manière native
launch_game() {
    local exe_path="$1"
    local game_args="${2:-}"
    local display_name="${3:-$(basename "$exe_path")}"
    local work_dir="${4:-}"

    echo "Lancement de $display_name..."
    
    # Déterminer comment exécuter le fichier selon son type
    local exe_dir
    exe_dir="$(dirname "$exe_path")"
    local exe_name
    exe_name="$(basename "$exe_path")"
    
    # Utiliser le répertoire de travail spécifié, sinon celui de l'exécutable
    local cd_dir="${work_dir:-$exe_dir}"
    
    # Construire la commande
    local cmd=""
    
    # Vérifier le type de fichier
    if [[ "$exe_name" == *.sh ]]; then
        # Script shell
        cmd="cd \"$cd_dir\" && \"$exe_path\""
    elif [[ "$exe_name" == *.py ]]; then
        # Script Python
        cmd="cd \"$cd_dir\" && /usr/bin/python3 \"$exe_path\""
    elif [[ "$exe_name" == *.AppImage ]] || [[ "$exe_name" == *.appimage ]]; then
        # AppImage
        cmd="cd \"$cd_dir\" && \"$exe_path\""
    else
        # Binaire ELF ou autre
        # Vérifier si c'est un binaire ELF
        local is_elf=false
        if [ -f "$exe_path" ]; then
            local magic
            magic=$(head -c 4 "$exe_path")
            if [ "$magic" = $'\x7fELF' ]; then
                is_elf=true
            fi
        fi
        
        if [ "$is_elf" = true ]; then
            cmd="cd \"$cd_dir\" && \"$exe_path\""
        else
            # Essayer de l'exécuter directement
            cmd="\"$exe_path\""
        fi
    fi
    
    # Ajouter les arguments
    if [ -n "$game_args" ]; then
        cmd="$cmd $game_args"
    fi
    
    echo "Commande: $cmd"
    
    # Exécuter
    eval "$cmd"
    local game_exit=$?
    
    echo "Jeu terminé avec le code: $game_exit"
    return $game_exit
}

# Fonction principale pour le mode LGP
run_lgp_mode() {
    init_lgp_variables
    mount_lgp

    # Nettoyage en cas d'interruption
    trap cleanup_lgp EXIT

    # Créer le symlink /tmp/lgp-saves AVANT prepare_saves
    setup_saves_symlink

    # Créer le symlink /tmp/lgp-extra AVANT prepare_extras
    setup_extras_symlink

    # Créer le symlink /tmp/lgp-temp AVANT prepare_temps
    setup_temp_symlink

    # IMPORTANT: prepare_saves AVANT read_lgp_config (l'exécutable peut être un symlink vers UserData)
    prepare_saves
    prepare_extras
    prepare_temps

    # Exécuter le script de pré-lancement AVANT read_lgp_config
    # car il peut créer des symlinks nécessaires pour l'exécutable
    execute_prelaunch_script

    # Lire la configuration
    read_lgp_config

    # Lancer le jeu (avec le répertoire de travail = dossier du LGP)
    launch_game "$REAL_EXE_PATH" "$args" "$LGPACK_NAME" "$MOUNT_DIR"

    # Nettoyage des symlinks saves et extras (le trap fera le reste)
    cleanup_saves_symlink
    cleanup_extras_symlink
    cleanup_temp_symlink
}

# Fonction principale pour le mode classique (exécutable direct)
run_classic_mode() {
    local dirpath
    local filename

    dirpath=$(dirname "$fullpath")
    filename=$(basename "$fullpath")

    # Résoudre le symlink si nécessaire
    local real_path="$fullpath"
    if [ -L "$fullpath" ]; then
        real_path="$(realpath "$fullpath")"
        echo "Symlink détecté: $fullpath -> $real_path"
    fi

    # Exécuter le script de pré-lancement si existe
    local script_file="$dirpath/.script.sh"
    if [ -f "$script_file" ]; then
        echo "Exécution du script de pré-lancement..."
        chmod +x "$script_file" 2>/dev/null
        (cd "$dirpath" && "$script_file")
    fi

    # Lancer le jeu
    launch_game "$real_path" "$args" "$filename"
}

#======================================
# Fonction principale
#======================================

main() {
    parse_arguments "$@"

    # Déterminer le mode
    if [[ "$fullpath" == *.lgp ]]; then
        run_lgp_mode
    else
        run_classic_mode
    fi
}

# Lancement du script
main "$@"
