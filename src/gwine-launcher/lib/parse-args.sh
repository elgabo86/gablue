#!/bin/bash

################################################################################
# parse-args.sh - Analyse des arguments CLI et aide
################################################################################

show_help() {
    echo "Gwine - Lanceur de jeux Windows avec Wine"
    echo ""
    echo "Usage: gwine [OPTION] [CHEMIN]"
    echo ""
    echo "Options:"
    echo "  --help            Afficher cette aide"
    echo "  --init            Initialiser/réinitialiser le préfixe Wine"
    echo "  --offline         Mode offline pour --init (vérifie que les composants sont en cache)"
    echo "  --update          Mettre à jour les composants (DXVK, VKD3D, gwine)"
    echo "  --kdialog         Activer les dialogues graphiques kdialog pour --init et --update"
    echo "  --fix             Activer le mode fix manette"
    echo "  --xbox            Émulation manettes Sony en Xbox 360 via ds2xbox"
    echo "  --xbox-ds4        Émulation DualShock 4 uniquement en Xbox 360"
    echo "  --xbox-dualsense  Émulation DualSense uniquement en Xbox 360"
    echo "  --xbox-on         Activer le mode xbox par défaut pour tous les lancements"
    echo "  --xbox-off        Désactiver le mode xbox par défaut"
    echo "  --reset           Réinitialiser toutes les options par défaut"
    echo "  --nofix           Ignorer le fichier .fix d'un jeu"
    echo "  --exewgp          Mode interactif pour choisir l'exécutable dans un WGP"
    echo "  --dxvk            Utiliser DXVK standard (défaut)"
    echo "  --dxvk-async      Utiliser DXVK-GPLAsync (asynchrone avec DXVK_ASYNC=1)"
    echo "  --nosandbox       Désactiver le sandboxing"
    echo "  --regedit         Lancer l'éditeur de registre"
    echo "  --reg fichier.reg Installer un fichier de registre"
    echo "  --reg add         Ajouter une valeur de registre"
    echo "  --reg del         Supprimer une valeur de registre"
    echo "  --reg get         Consulter une clé/valeur de registre"
    echo "  --winecfg         Lancer la configuration Wine"
    echo "  --winetricks      Lancer winetricks"
    echo "  --joytest         Lancer le test de joypad (joy.cpl)"
    echo "  --download-components  Télécharger tous les composants pour le mode offline"
    echo "  --cachepack       Créer un pack du cache pour installation offline"
    echo "  --cmd             Ouvrir un terminal Wine"
    echo "  --cmd \"<cmd>\"     Exécuter une commande dans Wine (cmd /C)"
    echo "  --use-ln-mounts   Utiliser des liens symboliques au lieu de bind mounts (fallback pour compatibilité)"
    echo "  --x11             Utiliser X11/XWayland comme mode d'affichage (défaut)"
    echo "  --wayland         Utiliser Wayland natif (désactive XWayland)"
    echo "  --env VAR=VAL     Passer une variable d'environnement au jeu (peut être utilisé plusieurs fois)"
    echo "  --args            Arguments à passer au jeu"
    echo "  --kill            Forcer l'arrêt de tous les processus Gwine en cours"
    echo "  --dir             Gérer les répertoires bindés dans le sandbox (add/del/list/reset)"
    echo ""
    echo "Options --dir:"
    echo "  gwine --dir add <chemin>    Ajouter un répertoire aux bind mounts"
    echo "  gwine --dir del <chemin>    Supprimer un répertoire des bind mounts"
    echo "  gwine --dir list            Lister tous les répertoires bindés"
    echo "  gwine --dir reset           Réinitialiser la liste (supprime tout)"
    echo ""
    echo "Exemples:"
    echo "  gwine ~/Jeux/monjeu.wgp"
    echo "  gwine ~/Jeux/monjeu.exe"
    echo "  gwine --init"
    echo "  gwine --init --dxvk-async   # Init avec DXVK-GPLAsync"
    echo "  gwine --dxvk                # Utiliser DXVK standard"
    echo "  gwine --dxvk-async          # Utiliser DXVK-GPLAsync (DXVK_ASYNC=1)"
    echo "  gwine --x11                 # Utiliser X11/XWayland comme mode d'affichage"
    echo "  gwine --wayland             # Utiliser Wayland natif comme mode d'affichage"
    echo "  gwine --fix ~/Jeux/monjeu.wgp"
    echo "  gwine --reg config.reg"
    echo "  gwine --reg add 'HKCU\Software\Wine\DllOverrides' /v ddraw /d native,builtin /f"
    echo "  gwine --reg del 'HKCU\Software\Wine\DllOverrides' /v ddraw /f"
    echo "  gwine --reg get 'HKCU\Software\Wine\DllOverrides' /v ddraw"
    echo "  gwine --kill"
}

# Options valides reconnues par gwine
VALID_OPTIONS=("--help" "-h" "--fix" "--xbox" "--xbox-ds4" "--xbox-dualsense" "--xbox-on" "--xbox-off" "--reset" "--nofix" "--exewgp" "--init" "--offline" "--update" "--kdialog" "--cmd" "--regedit" "--reg" "--winecfg" "--winetricks" "--winetrick" "--download-components" "--cachepack" "--nosandbox" "--joytest" "--args" "--gameid" "--use-ln-mounts" "--kill" "--dir" "--x11" "--wayland" "--env" "--dxvk" "--dxvk-async")

# Vérifie si une option est valide
is_valid_option() {
    local option="$1"
    for valid in "${VALID_OPTIONS[@]}"; do
        if [[ "$option" == "$valid" ]]; then
            return 0
        fi
    done
    return 1
}

parse_arguments() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --help|-h)
                show_help
                exit 0
                ;;
            --fix)
                fix_mode=true
                shift
                ;;
            --xbox)
                xbox_mode=true
                xbox_filter="all"
                shift
                ;;
            --xbox-ds4)
                xbox_mode=true
                xbox_filter="ds4"
                shift
                ;;
            --xbox-dualsense)
                xbox_mode=true
                xbox_filter="dualsense"
                shift
                ;;
            --xbox-on)
                xbox_on_mode=true
                shift
                ;;
            --xbox-off)
                xbox_off_mode=true
                shift
                ;;
            --reset)
                reset_mode=true
                shift
                ;;
            --nofix)
                nofix_mode=true
                shift
                ;;
            --exewgp)
                exewgp_mode=true
                shift
                ;;
            --init)
                export init_mode=true
                shift
                ;;
            --offline)
                OFFLINE_MODE=true
                shift
                ;;
            --update)
                update_mode=true
                shift
                ;;
            --kdialog)
                _USE_KDIALOG=true
                shift
                ;;
            --cmd)
                cmd_mode=true
                if [ -n "${2:-}" ] && [[ "$2" != --* ]]; then
                    cmd_command="$2"
                    shift 2
                else
                    shift
                fi
                ;;
            --use-ln-mounts)
                USE_BIND_MOUNTS=false
                export USE_BIND_MOUNTS
                shift
                ;;
            --dxvk)
                dxvk_mode=true
                shift
                ;;
            --dxvk-async)
                dxvk_async_mode=true
                shift
                ;;
            --x11)
                x11_mode=true
                shift
                ;;
            --wayland)
                wayland_mode=true
                shift
                ;;
            --env)
                if [ -n "${2:-}" ]; then
                    # Ajouter la variable à la liste des variables personnalisées
                    if [ -z "${GWINE_CUSTOM_VARS:-}" ]; then
                        GWINE_CUSTOM_VARS="$2"
                    else
                        GWINE_CUSTOM_VARS="$GWINE_CUSTOM_VARS
$2"
                    fi
                    export GWINE_CUSTOM_VARS
                    shift 2
                else
                    echo "Erreur: --env nécessite une valeur (VAR=VAL)" >&2
                    exit 1
                fi
                ;;
            --regedit)
                regedit_mode=true
                shift
                ;;
            --reg)
                reg_mode=true
                shift
                reg_args=()
                while [ $# -gt 0 ] && [[ "$1" != --* ]]; do
                    reg_args+=("$1")
                    shift
                done
                ;;
            --winecfg)
                winecfg_mode=true
                shift
                ;;
            --winetricks|--winetrick)
                winetricks_mode=true
                shift
                winetricks_args="$*"
                shift $#
                ;;
            --download-components)
                download_wincomponents_mode=true
                shift
                ;;
            --cachepack)
                cachepack_mode=true
                shift
                ;;
            --nosandbox)
                nosandbox_mode=true
                shift
                ;;
            --kill)
                echo "Arrêt forcé de tous les processus Gwine..."
                pgrep -u "$USER" -f -i "\.exe|\.bat" | grep -v $$ | xargs -r kill -9 2>/dev/null
                kill_wineserver
                echo "Terminé"
                exit 0
                ;;
            --joytest)
                joytest_mode=true
                shift
                ;;
            --args)
                args="$2"
                shift 2
                ;;
            --gameid)
                shift 2
                ;;
            --dir)
                dir_mode=true
                if [ -n "${2:-}" ]; then
                    dir_subcommand="$2"
                    shift
                fi
                if [ -n "${2:-}" ]; then
                    dir_path="$2"
                    shift
                fi
                shift
                ;;
            *)
                # Vérifier si c'est une option invalide (commence par -- mais n'est pas reconnu)
                if [[ "$1" == --* ]]; then
                    if ! is_valid_option "$1"; then
                        echo "Erreur: Option invalide '$1'" >&2
                        echo "Utilisez --help pour voir les options disponibles" >&2
                        exit 1
                    fi
                fi
                fullpath="$1"
                # Convertir en chemin absolu si c'est un chemin relatif
                if [ -n "$fullpath" ] && [[ "$fullpath" != /* ]]; then
                    fullpath="$(pwd)/$fullpath"
                fi
                shift
                ;;
        esac
    done
}
