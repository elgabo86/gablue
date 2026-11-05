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

# Lance le jeu de manière native (avec kernel overlay si actif)
launch_game() {
    local exe_path="$1"
    local game_args="${2:-}"
    local display_name="${3:-$(basename "$exe_path")}"
    local work_dir="${4:-}"

    echo "Lancement de $display_name..."
    
    local exe_dir
    exe_dir="$(dirname "$exe_path")"
    local exe_name
    exe_name="$(basename "$exe_path")"
    local cd_dir="${work_dir:-$exe_dir}"
    
    local cmd=""
    
    if [[ "$exe_name" == *.sh ]]; then
        cmd="cd \"$cd_dir\" && \"$exe_path\""
    elif [[ "$exe_name" == *.py ]]; then
        cmd="cd \"$cd_dir\" && /usr/bin/python3 \"$exe_path\""
    elif [[ "$exe_name" == *.AppImage ]] || [[ "$exe_name" == *.appimage ]]; then
        cmd="cd \"$cd_dir\" && \"$exe_path\""
    else
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
            cmd="\"$exe_path\""
        fi
    fi
    
    [ -n "$game_args" ] && cmd="$cmd $game_args"
    
    if [ "${_LGP_MODE:-false}" = "true" ]; then
        local kernel_cmd
        kernel_cmd="$(_lgp_kernel_overlay_mount_script)"
        kernel_cmd+="$cmd"
        echo "Lancement dans le namespace isolé..."
        bash -c "$kernel_cmd"
        local game_exit=$?
    else
        echo "Commande: $cmd"
        eval "$cmd"
        local game_exit=$?
    fi
    
    echo "Jeu terminé avec le code: $game_exit"
    return $game_exit
}

