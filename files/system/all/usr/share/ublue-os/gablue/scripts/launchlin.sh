#!/bin/bash

################################################################################
# launchlin.sh - Script de lancement de jeux Linux
#
# Ce script permet de lancer des jeux/applications Linux via des paquets LGP
# compressés, avec support :
# - Des paquets LGP (.lgp) compressés en squashfs
# - Des exécutables directs (binaires ELF, AppImages, scripts .sh/.py)
# - Script de pré-lancement .script.sh
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
            fusermount -uz "$MOUNT_DIR" 2>/dev/null || umount -f "$MOUNT_DIR" 2>/dev/null
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
                    fusermount -uz "$MOUNT_DIR" 2>/dev/null || umount -f "$MOUNT_DIR" 2>/dev/null
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

    # Vérifier que squashfuse est disponible
    if ! command -v squashfuse &> /dev/null; then
        error_exit "squashfuse n'est pas installé"
    fi

    # Créer et monter le squashfs
    mkdir -p "$MOUNT_DIR"
    echo "Montage de $LGPACK_FILE sur $MOUNT_DIR..."
    squashfuse -r "$LGPACK_FILE" "$MOUNT_DIR"

    if [ $? -ne 0 ]; then
        rmdir "$MOUNT_DIR"
        error_exit "Erreur lors du montage du squashfs"
    fi
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

    # Démontage du squashfs
    if ! fusermount -u "$MOUNT_DIR" 2>/dev/null; then
        # Si échec, tuer le processus squashfuse
        local FUSE_PID=$(fuser -m "$MOUNT_DIR" 2>/dev/null | head -n1)
        if [ -n "$FUSE_PID" ]; then
            kill -9 "$FUSE_PID" 2>/dev/null
            sleep 0.5
        fi
        # Force unmount lazy si nécessaire
        fusermount -uz "$MOUNT_DIR" 2>/dev/null || umount -f -l "$MOUNT_DIR" 2>/dev/null
    fi

    # Attendre un peu que le démontage se termine complètement
    sleep 0.2

    # Vérifier et forcer le démontage si encore monté
    if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        umount -f "$MOUNT_DIR" 2>/dev/null || umount -f -l "$MOUNT_DIR" 2>/dev/null
        sleep 0.1
    fi

    # Supprimer le dossier de montage
    if [ -d "$MOUNT_DIR" ]; then
        rmdir "$MOUNT_DIR" 2>/dev/null
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
            _copy_dir_with_symlinks "$SAVE_LGP_ITEM" "$FINAL_SAVE_ITEM"
        elif [ -e "$SAVE_LGP_ITEM" ]; then
            mkdir -p "$(dirname "$FINAL_SAVE_ITEM")"
            if [ -L "$SAVE_LGP_ITEM" ]; then
                _copy_symlink_as_abs "$SAVE_LGP_ITEM" "$FINAL_SAVE_ITEM"
            else
                cp -n "$SAVE_LGP_ITEM" "$FINAL_SAVE_ITEM"
            fi
        fi
    done < "$SAVE_FILE"
}

# Copie un symlink en absolu si target dans /tmp/lgpackmount
_copy_symlink_as_abs() {
    local src_symlink="$1"
    local dst_symlink="$2"

    # Lire la cible du symlink
    local target
    target=$(readlink "$src_symlink")
    [ -z "$target" ] && return 1

    local abs_target

    # Si c'est déjà un chemin absolu, l'utiliser tel quel
    if [[ "$target" == /* ]]; then
        abs_target="$target"
    else
        # Chemin relatif : résoudre depuis le dossier du symlink source
        abs_target=$(realpath -m "$(dirname "$src_symlink")/$target" 2>/dev/null)
        [ -z "$abs_target" ] && return 1
    fi

    # Supprimer /.save/ ou /.extra/ du chemin s'il est présent
    if [[ "$abs_target" == */.save/* ]]; then
        abs_target=$(echo "$abs_target" | sed 's|/.save/|/|g')
    elif [[ "$abs_target" == */.extra/* ]]; then
        abs_target=$(echo "$abs_target" | sed 's|/.extra/|/|g')
    fi

    # Vérifier que le chemin pointe vers le mount
    if [[ "$abs_target" == /tmp/lgpackmount/* ]]; then
        ln -s "$abs_target" "$dst_symlink"
    else
        # Hors du mount : copier le contenu du symlink
        cp -an "$src_symlink" "$dst_symlink"
    fi
}

# Copie récursive un dossier en convertissant les symlinks relatifs
_copy_dir_with_symlinks() {
    local src_dir="$1"
    local dst_dir="$2"

    mkdir -p "$dst_dir"

    # Étape 1: copier les fichiers normaux et dossiers (pas les symlinks)
    for item in "$src_dir"/*; do
        [ -e "$item" ] || [ -L "$item" ] || continue  # ignorer si le glob ne matche rien
        local name
        name=$(basename "$item")

        if [ -L "$item" ]; then
            # Traiter les symlinks à l'étape 2
            continue
        elif [ -f "$item" ]; then
            # Fichier normal
            cp -n "$item" "$dst_dir/$name"
        elif [ -d "$item" ]; then
            # Dossier : copier récursivement les fichiers normaux
            cp -rn --no-preserve=links "$item" "$dst_dir/$name"
        fi
    done

    # Étape 2: traiter les symlinks à tous les niveaux de la destination
    # D'abord les symlinks au niveau courant
    for item in "$src_dir"/*; do
        [ -e "$item" ] || [ -L "$item" ] || continue
        if [ -L "$item" ]; then
            local name
            name=$(basename "$item")
            _copy_symlink_as_abs "$item" "$dst_dir/$name"
        fi
    done

    # Ensuite les symlinks dans les sous-dossiers
    # IMPORTANT: itérer sur la SOURCE, pas sur la destination
    for item in "$src_dir"/*; do
        [ -d "$item" ] && [ ! -L "$item" ] || continue  # que les vrais dossiers (pas les symlinks)
        local name
        name=$(basename "$item")
        local rel_dst="$dst_dir/$name"
        if [ -d "$rel_dst" ]; then
            _copy_dir_with_symlinks "$item" "$rel_dst"
        fi
    done
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
            _copy_dir_with_symlinks "$EXTRA_LGP_ITEM" "$FINAL_EXTRA_ITEM"
        elif [ -e "$EXTRA_LGP_ITEM" ]; then
            mkdir -p "$(dirname "$FINAL_EXTRA_ITEM")"
            if [ -L "$EXTRA_LGP_ITEM" ]; then
                _copy_symlink_as_abs "$EXTRA_LGP_ITEM" "$FINAL_EXTRA_ITEM"
            else
                cp -n "$EXTRA_LGP_ITEM" "$FINAL_EXTRA_ITEM"
            fi
        fi
    done < "$EXTRAPATH_FILE"

    mkdir -p "$EXTRA_BASE"
    rm -f "$EXTRA_DIR"
    ln -s "$EXTRA_CACHE_DIR" "$EXTRA_DIR"
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

    # IMPORTANT: prepare_saves AVANT read_lgp_config (l'exécutable peut être un symlink vers UserData)
    prepare_saves
    prepare_extras

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
