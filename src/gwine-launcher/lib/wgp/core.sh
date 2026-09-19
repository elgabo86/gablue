#!/bin/bash

################################################################################
# wgp-core.sh - Variables globales et fonctions utilitaires WGP
################################################################################

# Variables globales WGP
WGPACK_FILE=""
WGPACK_NAME=""
FULL_EXE_PATH=""

# =============================================================================
# Initialisation des variables WGP
# =============================================================================

init_wgp_variables() {
    WGPACK_FILE="$(realpath "$fullpath")"
    GAME_INTERNAL_NAME=""

    local GAMENAME_CONTENT
    local UNSQUASHFS_BIN
    UNSQUASHFS_BIN=$(get_system_tool unsquashfs)
    GAMENAME_CONTENT=$("$UNSQUASHFS_BIN" -cat "$WGPACK_FILE" ".gamename" 2>/dev/null)
    if [ -n "$GAMENAME_CONTENT" ]; then
        GAME_INTERNAL_NAME="$GAMENAME_CONTENT"
        GAME_INTERNAL_NAME="${GAME_INTERNAL_NAME%.}"
    fi

    if [ -z "$GAME_INTERNAL_NAME" ]; then
        GAME_INTERNAL_NAME="$(basename "$WGPACK_FILE" .wgp)"
        GAME_INTERNAL_NAME="${GAME_INTERNAL_NAME%.}"
    fi

    WGPACK_NAME="$GAME_INTERNAL_NAME"

    # Utiliser le répertoire /tmp partagé pour le montage WGP
    # Ce répertoire est déjà bindé dans le sandbox via SHARED_TMP_DIR
    MOUNT_BASE="$SHARED_TMP_DIR/wgpackmount"
    MOUNT_DIR="$MOUNT_BASE/$WGPACK_NAME"
    # Garder les chemins /tmp/ traditionnels - ils seront visibles via le bind de /tmp
    EXTRA_BASE="/tmp/wgp-extra"
    EXTRA_DIR="$EXTRA_BASE/$WGPACK_NAME"
}

# =============================================================================
# Détection du fichier .gamescope (pack WGP ou sidecar)
# =============================================================================

# Sonde la configuration gamescope du fichier lancé ($fullpath), sans rien monter :
#   - .wgp : fichier .gamescope à la racine du squashfs (unsquashfs -cat, comme
#     .gamename), sinon fallback sur un sidecar .gamescope à côté du .wgp
#   - autres (.exe/.bat/...) : sidecar .gamescope à côté du fichier lancé
#     (même convention que .env/.args)
# Appelé depuis main() AVANT le re-exec gamescope (rien n'est monté à ce stade)
# Définit :
#   GAMESCOPE_FILE_ARGS   - arguments gamescope du fichier ("" = défaut gwine)
#   GAMESCOPE_FILE_OFF    - "true" si le fichier désactive gamescope pour ce jeu
#   GAMESCOPE_FILE_SOURCE - emplacement lu (message utilisateur)
# Retourne 0 si un fichier .gamescope a été trouvé, 1 sinon
peek_gamescope_config() {
    GAMESCOPE_FILE_ARGS=""
    GAMESCOPE_FILE_OFF=false
    GAMESCOPE_FILE_SOURCE=""

    [ -n "$fullpath" ] || return 1

    local content=""
    local found=false

    # Racine du pack WGP (prioritaire sur le sidecar : choix de l'auteur du pack)
    if [[ "$fullpath" == *.wgp ]]; then
        # unsquashfs est requis par init_wgp_variables de toute façon :
        # absent ici = sonde pack skippée, le sidecar reste consulté
        local UNSQUASHFS_BIN=""
        UNSQUASHFS_BIN=$(command -v unsquashfs 2>/dev/null) || UNSQUASHFS_BIN=""
        if [ -n "$UNSQUASHFS_BIN" ]; then
            # exit 0 = fichier présent dans le pack (même vide), != 0 = absent
            if content=$("$UNSQUASHFS_BIN" -cat "$fullpath" ".gamescope" 2>/dev/null); then
                found=true
                GAMESCOPE_FILE_SOURCE=".gamescope du pack"
            fi
        fi
    fi

    # Sidecar à côté du fichier lancé (fallback pour un .wgp sans .gamescope
    # interne, source principale pour .exe/.bat)
    if [ "$found" = false ]; then
        local sidecar
        sidecar="$(dirname "$fullpath")/.gamescope"
        if [ -f "$sidecar" ]; then
            content=$(cat "$sidecar") || return 1
            found=true
            GAMESCOPE_FILE_SOURCE="$sidecar"
        fi
    fi

    [ "$found" = true ] || return 1

    # Normalisation : CR supprimés, retours à la ligne -> espaces
    # (les args sont passés non-quotés à exec gamescope : le word splitting s'applique)
    content="${content//$'\r'/}"
    content="${content//$'\n'/ }"

    # Compactage pour les tests : fichier vide/blanc = défaut gwine,
    # mot-clé off = désactivation pour ce jeu (prioritaire sur le défaut stocké,
    # mais pas sur les options gamescope explicites de la ligne de commande)
    local compact="${content//[[:space:]]/}"
    if [ -z "$compact" ]; then
        return 0
    fi

    case "${compact,,}" in
        off|none|disable|disabled|no)
            GAMESCOPE_FILE_OFF=true
            ;;
        *)
            GAMESCOPE_FILE_ARGS="$content"
            ;;
    esac
    return 0
}

# =============================================================================
# Lecture de la configuration du WGP
# =============================================================================

read_wgp_config() {
    local LAUNCH_FILE="$MOUNT_DIR/.launch"
    if [ ! -f "$LAUNCH_FILE" ]; then
        echo "Erreur: fichier .launch introuvable dans le pack" >&2
        cleanup_wgp
        exit 1
    fi

    local launch_content
    launch_content="$(cat "$LAUNCH_FILE" | tr -d '\n\r')"
    FULL_EXE_PATH="$MOUNT_DIR/$launch_content"

    if [ ! -e "$FULL_EXE_PATH" ]; then
        echo "Erreur: exécutable introuvable: $(cat "$LAUNCH_FILE")" >&2
        cleanup_wgp
        exit 1
    fi

    local ARGS_FILE="$MOUNT_DIR/.args"
    if [ -f "$ARGS_FILE" ]; then
        local wgp_args
        wgp_args=$(cat "$ARGS_FILE")
        if [ -n "$wgp_args" ]; then
            args="$wgp_args"
        fi
    fi

    local FIX_FILE="$MOUNT_DIR/.fix"
    if [ -f "$FIX_FILE" ]; then
        if [ "$nofix_mode" = true ]; then
            echo "Note: fichier .fix ignoré car --nofix actif"
        else
            fix_mode=true
        fi
    fi
}

read_wgp_xbox_config() {
    local XBOX_FILE="$MOUNT_DIR/.xbox"
    if [ -f "$XBOX_FILE" ] && [ "$xbox_mode" != true ]; then
        local wgp_xbox_filter
        wgp_xbox_filter=$(cat "$XBOX_FILE" | tr -d '\n\r')
        xbox_mode=true
        if [ "$wgp_xbox_filter" = "ds4" ] || [ "$wgp_xbox_filter" = "dualsense" ]; then
            xbox_filter="$wgp_xbox_filter"
        else
            xbox_filter="all"
        fi
        echo "Mode xbox activé depuis le WGP (filtre: $xbox_filter)"
    fi
}

# =============================================================================
# Fonctions utilitaires pour la copie avec symlinks
# =============================================================================

_copy_symlink_as_abs() {
    local src_symlink="$1"
    local dst_symlink="$2"
    local src_base_dir="${3:-}"
    local dst_base_dir="${4:-}"

    local target
    target=$(readlink "$src_symlink")
    [ -z "$target" ] && return 1

    local abs_target
    if [[ "$target" == /* ]]; then
        abs_target="$target"
    else
        abs_target=$(realpath -m "$(dirname "$src_symlink")/$target" 2>/dev/null)
    fi

    if [[ "$abs_target" == */.save/* ]]; then
        abs_target=$(echo "$abs_target" | sed 's|/.save/|/|g')
    elif [[ "$abs_target" == */.extra/* ]]; then
        abs_target=$(echo "$abs_target" | sed 's|/.extra/|/|g')
    elif [[ "$abs_target" == */.temp/* ]]; then
        abs_target=$(echo "$abs_target" | sed 's|/.temp/|/|g')
    fi

    if [ -n "$src_base_dir" ] && [ -n "$dst_base_dir" ]; then
        if [[ "$abs_target" == "$src_base_dir"* ]]; then
            local rel_path="${abs_target#$src_base_dir/}"
            abs_target="$dst_base_dir/$rel_path"
        fi
    fi

    if [ -n "$abs_target" ]; then
        ln -s "$abs_target" "$dst_symlink"
    else
        ln -s "$target" "$dst_symlink"
    fi
}

_copy_dir_with_symlinks() {
    local src_dir="$1"
    local dst_dir="$2"
    local src_base_dir="${3:-}"
    local dst_base_dir="${4:-}"

    ensure_dir -s "$dst_dir"

    for item in "$src_dir"/*; do
        [ -e "$item" ] || [ -L "$item" ] || continue
        local name
        name=$(basename "$item")
        local dst_item="$dst_dir/$name"

        if [ -L "$item" ]; then
            _copy_symlink_as_abs "$item" "$dst_item" "$src_base_dir" "$dst_base_dir"
        elif [ -f "$item" ]; then
            cp -n "$item" "$dst_item"
        elif [ -d "$item" ]; then
            _copy_dir_with_symlinks "$item" "$dst_item" "$src_base_dir" "$dst_base_dir"
        fi
    done
}

_copy_dir_rewrite_symlinks() {
    local src_dir="$1"
    local dst_dir="$2"

    ensure_dir -s "$dst_dir"

    for item in "$src_dir"/*; do
        [ -e "$item" ] || [ -L "$item" ] || continue
        local name
        name=$(basename "$item")
        local dst_item="$dst_dir/$name"

        if [ -L "$item" ]; then
            _copy_symlink_rewrite "$item" "$dst_item"
        elif [ -f "$item" ]; then
            cp -n "$item" "$dst_item"
        elif [ -d "$item" ]; then
            _copy_dir_rewrite_symlinks "$item" "$dst_item"
        fi
    done
}

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
