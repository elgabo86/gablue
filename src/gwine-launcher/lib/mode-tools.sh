#!/bin/bash

################################################################################
# mode-tools.sh - Outils Wine (regedit, winecfg, winetricks, cmd)
################################################################################

# =============================================================================
# Outils Wine
# =============================================================================

# Lance regedit avec le préfixe Wine
launch_regedit() {
    echo "Lancement de regedit..."
    
    export WINEPREFIX="$HOME_REAL/Windows/Prefix"
    
    require_wine
    
    ensure_wineprefix
    
    echo "WINEPREFIX: $WINEPREFIX"
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" regedit
    exit $?
}

manage_registry() {
    local reg_subcmd="$1"
    shift
    
    export WINEPREFIX="$HOME_REAL/Windows/Prefix"
    require_wine
    ensure_wineprefix
    
    case "$reg_subcmd" in
        add)
            if [ $# -lt 3 ]; then
                error_exit "Usage: gwine --reg add <clé> /v <valeur> /d <donnée> [/f]"
            fi
            local reg_key="$1"
            shift
            local reg_value=""
            local reg_data=""
            local reg_type=""
            local reg_force=false
            while [ $# -gt 0 ]; do
                case "$1" in
                    /v) reg_value="$2"; shift 2 ;;
                    /ve) reg_value=""; shift ;;
                    /t) reg_type="$2"; shift 2 ;;
                    /d) reg_data="$2"; shift 2 ;;
                    /f) reg_force=true; shift ;;
                    *) shift ;;
                esac
            done
            if [ -z "$reg_value" ] && [ "$reg_force" = false ]; then
                error_exit "Spécifiez /v <valeur> ou /f pour une valeur par défaut"
            fi
            local reg_cmd_args=("$reg_key" /v "$reg_value")
            if [ -n "$reg_type" ]; then
                reg_cmd_args+=(/t "$reg_type")
            fi
            reg_cmd_args+=(/d "$reg_data" /f)
            echo "Ajout: [$reg_key] $reg_value = $reg_data${reg_type:+ ($reg_type)}"
            if WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add "${reg_cmd_args[@]}" >/dev/null 2>&1; then
                echo "Valeur ajoutée avec succès"
            else
                error_exit "Échec de l'ajout de la valeur de registre"
            fi
            ;;
        del)
            if [ $# -lt 1 ]; then
                error_exit "Usage: gwine --reg del <clé> [/v <valeur> | /va] [/f]"
            fi
            local reg_key="$1"
            shift
            local reg_value=""
            local del_all=false
            local reg_force=false
            while [ $# -gt 0 ]; do
                case "$1" in
                    /v) reg_value="$2"; shift 2 ;;
                    /ve) reg_value=""; shift ;;
                    /va) del_all=true; shift ;;
                    /f) reg_force=true; shift ;;
                    *) shift ;;
                esac
            done
            local reg_args=()
            if [ -n "$reg_value" ]; then
                reg_args+=(/v "$reg_value")
            elif [ "$del_all" = true ]; then
                reg_args+=(/va)
            else
                if [ "$reg_force" = false ]; then
                    error_exit "Spécifiez /v <valeur>, /va, ou /f pour supprimer une clé entière"
                fi
            fi
            if [ "$reg_force" = true ]; then
                reg_args+=(/f)
            fi
            if [ "$del_all" = true ]; then
                echo "Suppression de toutes les valeurs sous: $reg_key"
            elif [ -n "$reg_value" ]; then
                echo "Suppression: [$reg_key] $reg_value"
            else
                echo "Suppression de la clé: $reg_key"
            fi
            if WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg delete "$reg_key" "${reg_args[@]}" >/dev/null 2>&1; then
                echo "Suppression réussie"
            else
                error_exit "Échec de la suppression de la valeur de registre"
            fi
            ;;
        get)
            if [ $# -lt 1 ]; then
                error_exit "Usage: gwine --reg get <clé> [/v <valeur>]"
            fi
            local reg_key="$1"
            shift
            local reg_value=""
            while [ $# -gt 0 ]; do
                case "$1" in
                    /v) reg_value="$2"; shift 2 ;;
                    /ve) reg_value=""; shift ;;
                    *) shift ;;
                esac
            done
            if [ -n "$reg_value" ]; then
                WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg query "$reg_key" /v "$reg_value" 2>/dev/null
            else
                WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg query "$reg_key" 2>/dev/null
            fi
            ;;
        *)
            local reg_file_path="$reg_subcmd"
            if [ -z "$reg_file_path" ]; then
                error_exit "Aucun fichier .reg ou sous-commande spécifié"
            fi
            if [ ! -f "$reg_file_path" ]; then
                error_exit "Fichier .reg introuvable: $reg_file_path"
            fi
            echo "Installation du fichier de registre: $reg_file_path"
            ensure_wineprefix_full
            local win_path
            win_path=$(WINEPREFIX="$WINEPREFIX" "$WINE_BIN" winepath -w "$reg_file_path" 2>/dev/null | tr -d '\r')
            if [ -z "$win_path" ]; then
                error_exit "Impossible de convertir le chemin du fichier .reg"
            fi
            echo "Chemin Windows: $win_path"
            echo "WINEPREFIX: $WINEPREFIX"
            if ! WINEPREFIX="$WINEPREFIX" "$WINE_BIN" regedit.exe /S "$win_path" 2>/dev/null; then
                if [ "$_USE_KDIALOG" = "true" ] && command -v kdialog &>/dev/null; then
                    kdialog --title "Erreur" --error "Échec de l'installation du fichier de registre:\n$reg_file_path"
                fi
                error_exit "Échec de l'installation du fichier de registre"
            fi
            echo "Fichier de registre installé avec succès"
            if [ "$_USE_KDIALOG" = "true" ] && command -v kdialog &>/dev/null; then
                kdialog --title "Succès" --msgbox "Le fichier de registre a été installé avec succès:\n$reg_file_path"
            fi
            ;;
    esac
    
    exit 0
}

# Lance winecfg avec le préfixe Wine
launch_winecfg() {
    echo "Lancement de winecfg..."
    
    export WINEPREFIX="$HOME_REAL/Windows/Prefix"
    
    require_wine
    
    ensure_wineprefix
    
    echo "WINEPREFIX: $WINEPREFIX"
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" winecfg
    exit $?
}

# Lance cmd.exe (terminal Wine) dans le dossier courant
# Usage: launch_wine_cmd [commande]
# Sans argument: shell interactif (cmd)
# Avec argument: exécute la commande et sort (cmd /C "commande")
launch_wine_cmd() {
    local cmd_command="${1:-}"
    
    if [ -n "$cmd_command" ]; then
        echo "Exécution de la commande Wine: $cmd_command"
    else
        echo "Lancement du terminal Wine (cmd)..."
    fi
    
    export WINEPREFIX="$HOME_REAL/Windows/Prefix"
    
    require_wine
    
    ensure_wineprefix
    
    local work_dir
    if [ -n "$fullpath" ]; then
        if [ -d "$fullpath" ]; then
            work_dir="$fullpath"
        else
            work_dir="$(dirname "$fullpath")"
        fi
    else
        work_dir="$(pwd)"
    fi
    
    echo "WINEPREFIX: $WINEPREFIX"
    echo "Dossier: $work_dir"
    
    cd "$work_dir" || error_exit "Impossible d'accéder au dossier: $work_dir"
    if [ -n "$cmd_command" ]; then
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" cmd /C "$cmd_command"
    else
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" cmd
    fi
    exit $?
}

# Lance winetricks avec le préfixe Wine
launch_winetricks() {
    echo "Lancement de winetricks..."
    
    export WINEPREFIX="$HOME_REAL/Windows/Prefix"
    
    ensure_winetricks
    local WINETRICKS_BIN
    WINETRICKS_BIN=$(get_winetricks_bin)
    
    require_wine
    
    ensure_wineprefix_full
    
    export WINE="$WINE_BIN"
    
    local WINETRICKS_CACHE="$COMPONENTS_DIR/winetricks-cache"
    if [ -d "$WINETRICKS_CACHE" ]; then
        export W_CACHE="$WINETRICKS_CACHE"
    fi
    
    echo "WINEPREFIX: $WINEPREFIX"
    if [ -n "$winetricks_args" ]; then
        echo "Commande: winetricks $winetricks_args"
        WINEPREFIX="$WINEPREFIX" "$WINETRICKS_BIN" $winetricks_args
    else
        echo "Aucun argument spécifié, lancement de winetricks en mode interactif..."
        WINEPREFIX="$WINEPREFIX" "$WINETRICKS_BIN"
    fi
    exit $?
}

# Lance le test de joypad (joy.cpl) avec le préfixe Wine
launch_joypad_test() {
    echo "Lancement du test de joypad..."
    
    export WINEPREFIX="$HOME_REAL/Windows/Prefix"
    
    require_wine
    
    ensure_wineprefix
    
    apply_padfix_setting
    
    echo "WINEPREFIX: $WINEPREFIX"
    echo "Utilitaire: joy.cpl (Panneau de configuration des manettes)"
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" control joy.cpl
    local exit_code=$?
    
    restore_padfix_setting
    
    exit $exit_code
}
