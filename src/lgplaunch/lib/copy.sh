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

    # Retirer le suffixe $UID pour des chemins portables dans le cache
    for prefix in lgp-saves lgp-extra lgp-temp lgpackmount edenln \
                  lgp-temp-upper lgp-temp-work \
                  lgp-full-overlay lgp-full-overlay-upper lgp-full-overlay-work; do
        abs_target="${abs_target/\/tmp\/$prefix-$UID\//\/tmp\/$prefix/}"
    done

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

    # dotglob : inclure les fichiers cachés (sinon perdus silencieusement à
    # l'amorçage des saves, ex: .mkw_recompiled_managed_nand) ; état sauvegardé
    # et restauré pour ne pas fuiter hors de la fonction
    local _saved_shopt
    _saved_shopt="$(shopt -p dotglob nullglob)"
    shopt -s dotglob nullglob

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

    eval "$_saved_shopt"
}

# Copie récursive un dossier en réécrivant les symlinks externes pour pointer vers MOUNT_DIR
# Usage: _copy_dir_rewrite_symlinks <src_dir> <dst_dir>
_copy_dir_rewrite_symlinks() {
    local src_dir="$1"
    local dst_dir="$2"

    # dotglob : inclure les fichiers cachés (sinon perdus silencieusement à
    # l'amorçage des saves) ; état sauvegardé et restauré pour ne pas fuiter
    local _saved_shopt
    _saved_shopt="$(shopt -p dotglob nullglob)"
    shopt -s dotglob nullglob

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

    eval "$_saved_shopt"
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

