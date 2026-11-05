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
            *)
                fullpath="$1"
                shift
                ;;
        esac
    done

    [ -z "$fullpath" ] && error_exit "Aucun fichier spécifié"
}

