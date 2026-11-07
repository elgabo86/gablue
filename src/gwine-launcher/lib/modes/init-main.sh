#!/bin/bash

################################################################################
# init-main.sh - Fonctions principales d'initialisation du préfixe
#
# Progression : `progress_step` (ui.sh) publie « étape n/total » — lisible en
# terminal, et repris par la fenêtre de progression de Gablue Manager quand
# gwine est lancé par un .exe/.wgp (plus de kdialog).
################################################################################

init_prefix_only() {

    # `gwine --init` est la voie de mise à jour (gablue-manager, poste local
    # ou distant) : sans nouveau composant et avec un préfixe sain, ne rien
    # refaire. `--force` réinitialise le préfixe à la demande, et `--offline`
    # reste une init explicite depuis le cache (pas de version à comparer).
    # `--reset` et les modes DXVK explicites changent les DLL du préfixe :
    # l'init complète s'impose même sans nouvelle version.
    local explicit_reinit=false
    if [ "${force_init:-false}" = "true" ] || [ "${reset_mode:-false}" = "true" ] || \
       [ "${dxvk_mode:-false}" = "true" ] || [ "${dxvk_async_mode:-false}" = "true" ]; then
        explicit_reinit=true
    fi
    if [ "${OFFLINE_MODE:-false}" != "true" ] && [ "$explicit_reinit" != "true" ] \
       && [ -d "$WINEPREFIX" ] && [ -f "$WINEPREFIX/system.reg" ] \
       && ! components_update_available; then
        echo "Tous les composants sont déjà à jour, le préfixe Wine est conservé."
        echo "Utilisez 'gwine --init --force' pour réinitialiser le préfixe."
        progress_done "Tous les composants sont déjà à jour — préfixe Wine conservé."
        exit 0
    fi

    echo "Initialisation du préfixe Wine..."
    echo "Runner utilisé: gwine"

    if [ "$OFFLINE_MODE" = "true" ]; then
        echo "Mode offline activé"
        if ! check_wincomponents_cache; then
            echo ""
            echo "Erreur: Mode offline impossible - certains composants sont manquants dans le cache"
            echo "Lancez 'gwine --init' sans l'option --offline pour télécharger les composants manquants"
            exit 1
        fi
        echo "✓ Tous les composants sont présents dans le cache"
    fi

    export WINEPREFIX="$HOME_REAL/Windows/Prefix"
    export WINEARCH="win64"

    # Liste des étapes : elle suit exactement le déroulé ci-dessous, pour que la
    # barre ne dépasse jamais 100 % (l'ancien total fixe se trompait dès qu'un
    # préfixe existait : l'étape de sauvegarde n'était pas comptée).
    local -a INIT_STEPS=()
    INIT_STEPS+=("Vérification du runner Wine")
    if [ "$OFFLINE_MODE" != "true" ]; then
        INIT_STEPS+=("Téléchargement des composants graphiques")
        INIT_STEPS+=("Téléchargement des composants Windows")
    fi
    [ -d "$WINEPREFIX" ] && INIT_STEPS+=("Sauvegarde de l'ancien préfixe")
    INIT_STEPS+=("Création du préfixe Wine")
    INIT_STEPS+=("Installation de Wine Mono et Gecko")
    INIT_STEPS+=("Installation des composants Windows")
    INIT_STEPS+=("Installation de DXVK et VKD3D")
    INIT_STEPS+=("Installation de D7VK")
    INIT_STEPS+=("Configuration de MangoHud")
    INIT_STEPS+=("Configuration des dossiers Windows")
    INIT_STEPS+=("Initialisation des caches de shaders")
    INIT_STEPS+=("Finalisation")

    local TOTAL_STEPS=${#INIT_STEPS[@]}
    local CURRENT_STEP=0

    next_init_step() {
        CURRENT_STEP=$((CURRENT_STEP + 1))
        progress_step "$CURRENT_STEP" "$TOTAL_STEPS" \
            "${INIT_STEPS[$((CURRENT_STEP - 1))]}"
    }

    echo ""
    echo "Phase 1/2 : Vérification et téléchargement des composants..."
    echo ""

    next_init_step

    if [ "$OFFLINE_MODE" = "true" ] && ([ ! -d "$WINE_DIR" ] || [ ! -f "$WINE_DIR/bin/wine" ]); then
        # Runner absent mais mode offline : tenter l'installation depuis le cache
        # (le pack cache déployé ne contient que ~/.cache/gwine, pas le runner extrait)
        echo "Runner gwine absent, installation depuis le cache offline..."
        install_gwine_from_cache || {
            error_exit "Mode offline: le runner gwine n'est pas installé et aucune archive n'est disponible dans le cache."
        }
    fi

    if [ "$OFFLINE_MODE" != "true" ]; then
        progress_busy "Vérification des mises à jour du runner…"
        # --force réinstalle le runner même à version égale (réparation) ;
        # sinon seul un runner plus récent est téléchargé.
        local runner_force="false"
        [ "${force_init:-false}" = "true" ] && runner_force="true"
        download_gwine "$runner_force"
    fi

    require_wine

    if [ "$OFFLINE_MODE" != "true" ]; then
        next_init_step
        auto_update_components

        next_init_step
        if ! prepare_wincomponents_cache; then
            error_exit "Échec du téléchargement des composants Windows"
        fi
    fi

    echo ""
    echo "✓ Tous les composants sont prêts"
    echo ""

    echo "Phase 2/2 : Installation dans le préfixe Wine..."
    echo ""

    # Restauration de l'ancien préfixe sur interruption (SIGTERM de Gablue
    # Manager, Ctrl-C). Le dialogue de progression envoie SIGTERM avant tout
    # SIGKILL, précisément pour laisser ce nettoyage se faire.
    restore_backup() {
        if [ -n "$PREFIX_BACKUP_PATH" ] && [ -d "$PREFIX_BACKUP_PATH" ]; then
            restore_wineprefix
        else
            rm -rf "$WINEPREFIX" 2>/dev/null || true
        fi
    }

    _init_interrupted() {
        trap - INT TERM
        echo "" >&2
        echo "Initialisation interrompue : restauration de l'ancien préfixe…" >&2
        progress_busy "Annulation : restauration de l'ancien préfixe…"
        restore_backup
        progress_done "Initialisation annulée."
        exit 130
    }
    trap _init_interrupted INT TERM

    if [ -d "$WINEPREFIX" ]; then
        next_init_step
        backup_wineprefix || {
            error_exit "Échec de la sauvegarde du préfixe"
        }
    fi

    next_init_step
    if ! wineboot_init_prefix; then
        restore_backup
        error_exit "Échec de l'initialisation du préfixe (wineboot)"
    fi

    next_init_step
    if ! install_wine_mono_gecko; then
        restore_backup
        error_exit "Échec de l'installation de Mono/Gecko"
    fi
    trap _init_interrupted INT TERM        # mono.sh a posé le sien

    next_init_step
    if ! install_winetricks_components; then
        restore_backup
        error_exit "Échec de l'installation des composants Windows"
    fi
    trap _init_interrupted INT TERM        # winetricks.sh a posé le sien

    next_init_step
    if ! install_dxvk_vkd3d; then
        restore_backup
        error_exit "Échec de l'installation de DXVK/VKD3D"
    fi

    next_init_step
    if ! install_d7vk; then
        restore_backup
        error_exit "Échec de l'installation de D7VK"
    fi

    next_init_step
    if ! copy_mangohud_config; then
        restore_backup
        error_exit "Échec de la copie de la config MangoHud"
    fi

    next_init_step
    if ! setup_windows_directories "$WINEPREFIX"; then
        restore_backup
        error_exit "Échec de la configuration des dossiers Windows"
    fi

    setup_wine_temp_symlinks "$WINEPREFIX"
    configure_sdl_input_registry

    next_init_step
    ensure_dirs -s "$SHADER_CACHE_DIR/dxvk" "$SHADER_CACHE_DIR/vkd3d" "$SHADER_CACHE_DIR/nvidia" "$SHADER_CACHE_DIR/mesa"

    next_init_step
    if [ -n "$PREFIX_BACKUP_PATH" ] && [ -d "$PREFIX_BACKUP_PATH" ]; then
        rm -rf "$PREFIX_BACKUP_PATH"
    fi
    trap - INT TERM
    progress_done "Préfixe Wine initialisé."

    # Afficher le mode DXVK configuré
    local current_dxvk_mode
    current_dxvk_mode=$(get_current_dxvk_mode)

    echo ""
    echo "Préfixe Wine initialisé avec succès !"
    echo ""
    echo "Configuration effectuée :"
    echo "  ✓ Préfixe Wine créé"
    echo "  ✓ Runner: gwine"
    if [ "$current_dxvk_mode" = "dxvk-async" ]; then
        echo "  ✓ DXVK-GPLAsync et VKD3D-Proton configurés"
        echo "  ✓ DXVK_ASYNC=1 sera défini automatiquement"
    else
        echo "  ✓ DXVK et VKD3D-Proton configurés"
    fi
    echo "  ✓ Wine Mono et Gecko installés"
    echo "  ✓ Composants Windows (winetricks) installés"
    echo "  ✓ Configuration MangoHud copiée"
    echo "  ✓ Dossiers Windows configurés"
    echo "  ✓ Caches shaders initialisés"
    echo ""
    echo "Emplacement: $WINEPREFIX"
}

init_wineprefix() {
    local needs_init=false

    if [ "$init_mode" = true ]; then
        echo "Mode --init: réinitialisation du préfixe demandée"
        if [ -d "$WINEPREFIX" ]; then
            echo "Suppression de l'ancien préfixe..."
            rm -rf "$WINEPREFIX"
        fi
        needs_init=true
    elif [ ! -d "$WINEPREFIX" ] || [ ! -f "$WINEPREFIX/system.reg" ]; then
        echo "Préfixe Wine non trouvé, création automatique..."
        needs_init=true
    fi

    if [ "$needs_init" = true ]; then
        prepare_local_cache

        echo "Création du préfixe Wine..."
        if ! wineboot_init_prefix; then
            echo "Erreur: échec de l'initialisation du préfixe Wine"
            return 1
        fi

        if ! install_wine_mono_gecko; then
            echo "Erreur: échec de l'installation de Mono/Gecko"
        fi

        export _NEEDS_WINETRICKS_INIT=1
    fi

    if [ -d "$WINEPREFIX" ] && [ -f "$WINEPREFIX/system.reg" ]; then
        setup_windows_directories "$WINEPREFIX"

        setup_wine_temp_symlinks "$WINEPREFIX"
    fi
}
