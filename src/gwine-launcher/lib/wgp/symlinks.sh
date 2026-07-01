#!/bin/bash

################################################################################
# wgp-symlinks.sh - Gestion des symlinks pour saves, extras et données de jeu
################################################################################

# =============================================================================
# Préparation des sauvegardes
# =============================================================================

prepare_saves() {
    local SAVE_FILE="$MOUNT_DIR/.savepath"
    local SAVE_WGP_DIR="$MOUNT_DIR/.save"
    local SAVES_DIR="$SAVES_REAL/$GAME_INTERNAL_NAME"

    [ -f "$SAVE_FILE" ] || return 0

    # S'assurer que le repertoire de sauvegarde existe
    ensure_dir -s "$SAVES_DIR"

    # S'assurer que le repertoire parent du symlink existe
    ensure_dir -s "$SAVES_SYMLINK"

    if [ -d "$SAVES_DIR" ]; then
        if [ -n "$(find "$SAVES_DIR" -mindepth 1 -maxdepth 1 2>/dev/null)" ]; then
            # Les sauvegardes existent deja, pas besoin de copier
            return 0
        fi
    fi

    echo "Copie des sauvegardes depuis .save..."

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SAVE_WGP_ITEM="$SAVE_WGP_DIR/$SAVE_REL_PATH"
        local FINAL_SAVE_ITEM="$SAVES_DIR/$SAVE_REL_PATH"

        if [ -d "$SAVE_WGP_ITEM" ]; then
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

# =============================================================================
# Préparation des extras
# =============================================================================

prepare_extras() {
    local EXTRAPATH_FILE="$MOUNT_DIR/.extrapath"
    local EXTRA_WGP_DIR="$MOUNT_DIR/.extra"
    local EXTRA_CACHE_DIR="$EXTRA_REAL/$GAME_INTERNAL_NAME"

    [ -f "$EXTRAPATH_FILE" ] || return 0

    # S'assurer que le repertoire de cache existe
    ensure_dir -s "$EXTRA_CACHE_DIR"

    # S'assurer que le repertoire parent du symlink existe
    ensure_dir -s "$EXTRA_SYMLINK"

    local EXTRA_DIR="$EXTRA_SYMLINK/$GAME_INTERNAL_NAME"

    if [ -d "$EXTRA_DIR" ] && [ ! -L "$EXTRA_DIR" ]; then
        rm -rf "$EXTRA_DIR"
    fi

    if [ -d "$EXTRA_CACHE_DIR" ] && [ -n "$(find "$EXTRA_CACHE_DIR" -mindepth 1 -maxdepth 1 2>/dev/null)" ]; then
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

    rm -f "$EXTRA_DIR"
    ln -s "$EXTRA_CACHE_DIR" "$EXTRA_DIR"
}

# =============================================================================
# Fonctions utilitaires pour la gestion des symlinks
# =============================================================================

# Vérifie si un fichier de config existe
# Usage: has_config_file <filename>
has_config_file() {
    [ -f "$MOUNT_DIR/$1" ]
}

# Crée un symlink pour un type de données (saves, extras, etc.)
# Usage: setup_game_symlink <type> <real_dir> <symlink_dir>
setup_game_symlink() {
    local type_name="$1"
    local real_dir="$2"
    local symlink_dir="$3"
    local game_dir="$real_dir/$GAME_INTERNAL_NAME"
    local game_symlink="$symlink_dir/$GAME_INTERNAL_NAME"

    # S'assurer que les repertoires existent
    ensure_dirs -s "$game_dir" "$symlink_dir"

    if [ -L "$game_symlink" ]; then
        rm -f "$game_symlink"
    fi

    ln -s "$game_dir" "$game_symlink"
    echo "Symlink créé: $game_symlink -> $game_dir"
}

# Nettoie un symlink de jeu
# Usage: cleanup_game_symlink <symlink_dir>
cleanup_game_symlink() {
    local game_symlink="$1/$GAME_INTERNAL_NAME"
    [ -L "$game_symlink" ] && rm -f "$game_symlink"
    return 0
}

# Gère un symlink de jeu (création ou suppression)
# Usage: manage_game_symlink <action> <type>
#   action: "setup" ou "cleanup"
#   type: "saves" ou "extras"
manage_game_symlink() {
    local action="$1"
    local type="$2"
    local real_dir symlink_dir
    
    case "$type" in
        saves)
            [ "$action" = "setup" ] && ! has_config_file ".savepath" && return 0
            real_dir="$SAVES_REAL"
            symlink_dir="$SAVES_SYMLINK"
            ;;
        extras)
            [ "$action" = "setup" ] && ! has_config_file ".extrapath" && return 0
            real_dir="$EXTRA_REAL"
            symlink_dir="$EXTRA_SYMLINK"
            ;;
        *)
            return 1
            ;;
    esac
    
    case "$action" in
        setup)
            setup_game_symlink "$type" "$real_dir" "$symlink_dir"
            ;;
        cleanup)
            cleanup_game_symlink "$symlink_dir"
            ;;
    esac
}

# Wrappers pour compatibilité (déléguent vers manage_game_symlink)
setup_saves_symlink() { manage_game_symlink setup saves; }
cleanup_saves_symlink() { manage_game_symlink cleanup saves; }
setup_extras_symlink() { manage_game_symlink setup extras; }
cleanup_extras_symlink() { manage_game_symlink cleanup extras; }
