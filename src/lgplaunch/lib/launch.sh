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
    # MangoHud : le wrapper `mangohud` (LD_PRELOAD → Vulkan ET OpenGL), pas
    # seulement MANGOHUD=1 qui ne couvre que la couche Vulkan. MANGOHUD=0
    # (option lgplaunch ou .env du pack) coupe l'overlay.
    local mh=""
    if [ "${MANGOHUD:-}" != "0" ] && [ -n "${MANGOHUD_BIN:-}" ] && \
       [ -x "${MANGOHUD_BIN:-}" ]; then
        mh="$MANGOHUD_BIN "
    fi

    if [[ "$exe_name" == *.sh ]]; then
        cmd="cd \"$cd_dir\" && ${mh}\"$exe_path\""
    elif [[ "$exe_name" == *.py ]]; then
        cmd="cd \"$cd_dir\" && ${mh}/usr/bin/python3 \"$exe_path\""
    elif [[ "$exe_name" == *.AppImage ]] || [[ "$exe_name" == *.appimage ]]; then
        cmd="cd \"$cd_dir\" && ${mh}\"$exe_path\""
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
            cmd="cd \"$cd_dir\" && ${mh}\"$exe_path\""
        else
            cmd="${mh}\"$exe_path\""
        fi
    fi
    
    [ -n "$game_args" ] && cmd="$cmd $game_args"

    # gamescope : compositeur imbriqué (même rôle que `gwine --gamescope`).
    # `--resolution` force la taille interne ; sans elle, `-f` seul laisse
    # gamescope prendre la taille de l'écran. On enveloppe la commande ENTIÈRE
    # (`cd` compris) : `printf %q` la protège pour le `eval` plus bas et pour
    # le `bash -c` du namespace overlay.
    if [ "${gamescope_opt:-false}" = "true" ]; then
        if ! command -v gamescope >/dev/null 2>&1; then
            error_exit "gamescope n'est pas installé"
        fi
        local gs_args="-f"
        if [ -n "${resolution_opt:-}" ]; then
            local gs_w
            case "$resolution_opt" in
                1080) gs_w=1920 ;;
                900)  gs_w=1600 ;;
                720)  gs_w=1280 ;;
                480)  gs_w=854 ;;
            esac
            gs_args="-w $gs_w -h $resolution_opt -f"
        fi
        echo "Lancement sous gamescope: $gs_args"
        cmd="gamescope $gs_args -- bash -c $(printf '%q' "$cmd")"
    fi

    # Overlay manette : nom et icône du PAQUET pendant la partie. Le jeu tourne
    # en avant-plan sous ce processus (directement, ou via le `bash -c` de
    # l'overlay kernel) : publier `$$` suffit, gpad descend l'arbre. Le logo
    # manquant est demandé à glauncher en fond (il écrit dans son cache, jamais
    # l'overlay).
    #
    # Nom publié : celui du FICHIER de pack (sans `.lgp`), comme gwine — le nom
    # interne (`.gamename`, « projectmidas ») est technique et ne dit rien à
    # l'utilisateur. En mode classique (exécutable seul) LGPACK_FILE est vide :
    # on garde le nom de l'exécutable.
    local running_nom="$display_name"
    if [ "${_LGP_MODE:-false}" = "true" ] && [ -n "${LGPACK_FILE:-}" ]; then
        running_nom="$(basename "$LGPACK_FILE")"
        running_nom="${running_nom%.lgp}"
    fi
    running_publish "$running_nom" "${LGPACK_FILE:-}" "$$"
    running_fetch_art "${LGPACK_FILE:-}"

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
    # plus de jeu : l'overlay ne doit plus afficher ce paquet
    running_clear
    return $game_exit
}

