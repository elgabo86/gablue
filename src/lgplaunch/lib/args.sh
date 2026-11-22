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
            --exe)
                SELECT_EXE_MODE=true
                shift
                ;;
            --exe=*)
                SELECT_EXE_MODE=true
                SELECT_EXE_PATH="${1#--exe=}"
                shift
                ;;
            --mangohud)
                if [ -n "${2:-}" ]; then
                    mangohud_opt="$2"
                    shift 2
                else
                    error_exit "--mangohud nécessite 'on' ou 'off'"
                fi
                ;;
            --mangohud-layout)
                if [ -n "${2:-}" ]; then
                    mangohud_layout_opt="$2"
                    shift 2
                else
                    error_exit "--mangohud-layout nécessite 'horizontal' ou 'rectangle'"
                fi
                ;;
            --fps-limit)
                if [ -n "${2:-}" ]; then
                    mangohud_fps_limit_opt="$2"
                    shift 2
                else
                    error_exit "--fps-limit nécessite une valeur (0 = illimitée)"
                fi
                ;;
            --gamescope)
                gamescope_opt=true
                shift
                ;;
            --resolution)
                if [ -n "${2:-}" ]; then
                    case "$2" in
                        1080|900|720|480) resolution_opt="$2" ;;
                        *) error_exit "--resolution attend 1080, 900, 720 ou 480" ;;
                    esac
                    gamescope_opt=true
                    shift 2
                else
                    error_exit "--resolution nécessite une hauteur (1080, 900, 720 ou 480)"
                fi
                ;;
            *)
                fullpath="$1"
                shift
                ;;
        esac
    done

    # Un réglage seul (--mangohud, --mangohud-layout, --fps-limit) est valide
    # sans fichier : il ne fait que modifier les options globales.
    if [ -z "$fullpath" ] && [ -z "${mangohud_opt:-}" ] && \
       [ -z "${mangohud_layout_opt:-}" ] && [ -z "${mangohud_fps_limit_opt:-}" ]; then
        error_exit "Aucun fichier spécifié"
    fi
}

