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
# Normaliser $HOME vers /var/home (chemin réel sur Silverblue/Kinoite)
# $HOME peut être /home/gab ou /var/home/gab selon la configuration
HOME_REAL="$(realpath "$HOME")"
WINDOWS_HOME="$HOME_REAL/Windows/UserData"
SAVES_SYMLINK="/tmp/wgp-saves"
SAVES_REAL="$WINDOWS_HOME/$USER/LocalSavesWGP"

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
        if ! pgrep -f "bwrap.*$MOUNT_DIR" > /dev/null 2>&1; then
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
                PIDs=$(pgrep -f "bwrap.*$MOUNT_DIR" 2>/dev/null)
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
    if mountpoint -q "$MOUNT_DIR" && pgrep -f "bwrap.*$MOUNT_DIR" > /dev/null 2>&1; then
        # Une nouvelle instance a pris la main, ne surtout pas démonter
        echo "Une nouvelle instance de $WGPACK_NAME a pris la main, pas de démontage."
        return 0
    fi

    echo "Démontage de $WGPACK_NAME..."

    # Nettoyer le symlink /tmp/wgp-saves
    cleanup_saves_symlink

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

    # Attendre un peu que le démontage se termine complètement
    sleep 0.5

    # Vérifier et forcer le démontage si encore monté
    if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        umount -f "$MOUNT_DIR" 2>/dev/null || umount -f -l "$MOUNT_DIR" 2>/dev/null
        sleep 0.3
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

    FULL_EXE_PATH="$MOUNT_DIR/$(cat "$LAUNCH_FILE")"

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
            _copy_dir_with_symlinks "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM"
        elif [ -e "$SAVE_WGP_ITEM" ]; then
            mkdir -p "$(dirname "$FINAL_SAVE_ITEM")"
            if [ -L "$SAVE_WGP_ITEM" ]; then
                _copy_symlink_as_abs "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM"
            else
                cp "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM"
            fi
        fi
    done < "$SAVE_FILE"
}

# Copie un symlink en absolu si target dans /tmp/wgpackmount
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

    # Supprimer /.save/ du chemin s'il est présent
    if [[ "$abs_target" == */.save/* ]]; then
        abs_target=$(echo "$abs_target" | sed 's|/.save/|/|g')
    fi

    # Vérifier que le chemin pointe vers le mount
    if [[ "$abs_target" == /tmp/wgpackmount/* ]]; then
        ln -s "$abs_target" "$dst_symlink"
    else
        # Hors du mount : copier le contenu du symlink
        cp -a "$src_symlink" "$dst_symlink"
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
            cp "$item" "$dst_dir/$name"
        elif [ -d "$item" ]; then
            # Dossier : copier récursivement les fichiers normaux
            cp -r --no-preserve=links "$item" "$dst_dir/$name"
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
    for item in "$dst_dir"/*; do
        [ -d "$item" ] || continue  # que les dossiers
        local name
        name=$(basename "$item")
        local rel_src="$src_dir/$name"
        if [ -d "$rel_src" ]; then
            _copy_dir_with_symlinks "$rel_src" "$item"
        fi
    done
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

# Crée le symlink /tmp/wgp-saves/$GAME_INTERNAL_NAME vers UserData
# Un symlink par jeu permet de lancer plusieurs WGP en parallèle
setup_saves_symlink() {
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

# Lance le jeu WGP via Bottles
launch_wgp_game() {
    echo "Lancement de $WGPACK_NAME..."

    apply_padfix_setting

    # Lancer le jeu en arrière-plan
    if [ -n "$args" ]; then
        run_bottles "$FULL_EXE_PATH" " $args" &
    else
        run_bottles "$FULL_EXE_PATH" "" &
    fi

    # Petite pause pour laisser le bwrap se lancer
    sleep 1

    # Attendre que le jeu se termine (surveiller si un bwrap exécute CET exécutable via son chemin complet unique)
    echo "En attente de la fermeture du jeu..."

    while pgrep -f "bwrap.*$FULL_EXE_PATH" > /dev/null 2>&1; do
        sleep 1
    done

    restore_padfix_setting
    cleanup_saves_symlink
}

# Installe les fichiers .reg trouvés dans un dossier via regedit.exe
install_registry_files() {
    local reg_dir="$1"
    local reg_files

    # Chercher les fichiers .reg dans le dossier
    reg_files=()
    while IFS= read -r -d '' file; do
        reg_files+=("$file")
    done < <(find "$reg_dir" -maxdepth 1 -name '*.reg' -print0 2>/dev/null)

    # Si aucun fichier .reg, rien à faire
    [ ${#reg_files[@]} -eq 0 ] && return 0

    # Installer chaque fichier .reg
    for reg_file in "${reg_files[@]}"; do
        local reg_name
        reg_name=$(basename "$reg_file")
        echo "Installation du fichier de registre: $reg_name"
        run_bottles "$HOME_REAL/Windows/WinDrive/windows/regedit.exe" "/S \"$reg_file\""
    done
}

# Fonction principale pour le mode WGP
run_wgp_mode() {
    init_wgp_variables
    mount_wgp

    # Nettoyage en cas d'interruption
    trap cleanup_wgp EXIT

    # Créer le symlink /tmp/wgp-saves AVANT prepare_saves
    setup_saves_symlink

    # IMPORTANT: prepare_saves AVANT read_wgp_config (l'exécutable peut être un symlink vers UserData)
    prepare_saves
    prepare_extras
    read_wgp_config
    # Installer les fichiers .reg dans le dossier de l'exécutable
    install_registry_files "$(dirname "$FULL_EXE_PATH")"
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

    # Installer les fichiers .reg dans le dossier de l'exécutable
    install_registry_files "$(dirname "$new_fullpath")"

    if [ -n "$args" ]; then
        run_bottles "$new_fullpath" " $args"
    else
        run_bottles "$new_fullpath" ""
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
