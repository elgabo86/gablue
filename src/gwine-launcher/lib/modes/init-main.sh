#!/bin/bash

################################################################################
# init-main.sh - Fonctions principales d'initialisation du préfixe
################################################################################

init_prefix_only() {
    local current_runner
    current_runner=$(get_current_runner)
    
    echo "Initialisation du préfixe Wine..."
    echo "Runner utilisé: $current_runner"
    
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
    
    local TOTAL_STEPS=10
    local CURRENT_STEP=0
    local DBUS_REF=""
    
    if [ "$_USE_KDIALOG" = "true" ]; then
        DBUS_REF=$(progress_create "Initialisation du préfixe Wine" "$TOTAL_STEPS")
    fi
    
    echo ""
    echo "Phase 1/2 : Vérification et téléchargement des composants..."
    echo ""
    
    # Vérifier et télécharger/mettre à jour le runner actuel si nécessaire
    ((CURRENT_STEP++))
    progress_update "$DBUS_REF" "$CURRENT_STEP" "Vérification du runner..."
    
    if [ "$OFFLINE_MODE" = "true" ] && ([ ! -d "$WINE_DIR" ] || [ ! -f "$WINE_DIR/bin/wine" ]); then
        # Runner absent mais mode offline : tenter l'installation depuis le cache
        # (le pack cache déployé ne contient que ~/.cache/gwine, pas le runner extrait)
        echo "Runner $current_runner absent, installation depuis le cache offline..."
        if [ "$current_runner" = "proton" ]; then
            install_gwine_proton_from_cache || {
                progress_close "$DBUS_REF"
                error_exit "Mode offline: le runner $current_runner n'est pas installé et aucune archive n'est disponible dans le cache."
            }
        else
            install_gwine_from_cache || {
                progress_close "$DBUS_REF"
                error_exit "Mode offline: le runner $current_runner n'est pas installé et aucune archive n'est disponible dans le cache."
            }
        fi
        update_runner_paths "$current_runner"
    fi
    
    if [ "$OFFLINE_MODE" != "true" ]; then
        if [ "$_USE_KDIALOG" = "true" ] && command -v kdialog &>/dev/null; then
            kdialog --passivepopup "Vérification des mises à jour du runner..." 5
        fi
        # Toujours vérifier les mises à jour, même si le runner est déjà installé
        if [ "$current_runner" = "proton" ]; then
            download_gwine_proton "force"
        else
            download_gwine "force"
        fi
    fi
    
    require_wine
    
    check_progress_cancelled "$DBUS_REF"
    
    if [ "$OFFLINE_MODE" != "true" ]; then
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Téléchargement des composants (DXVK, VKD3D)..."
        auto_update_components
        
        check_progress_cancelled "$DBUS_REF"
        
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Téléchargement des composants Windows..."
        if ! prepare_wincomponents_cache; then
            progress_close "$DBUS_REF"
            error_exit "Échec du téléchargement des composants Windows"
        fi
    fi
    
    check_progress_cancelled "$DBUS_REF"
    
    echo ""
    echo "✓ Tous les composants sont prêts"
    echo ""
    
    echo "Phase 2/2 : Installation dans le préfixe Wine..."
    echo ""
    
    if [ -d "$WINEPREFIX" ]; then
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Sauvegarde de l'ancien préfixe..."
        backup_wineprefix || {
            progress_close "$DBUS_REF"
            error_exit "Échec de la sauvegarde du préfixe"
        }
    fi
    
    restore_backup() {
        if [ -n "$PREFIX_BACKUP_PATH" ] && [ -d "$PREFIX_BACKUP_PATH" ]; then
            restore_wineprefix
        else
            rm -rf "$WINEPREFIX" 2>/dev/null || true
        fi
    }
    
    ((CURRENT_STEP++))
    progress_update "$DBUS_REF" "$CURRENT_STEP" "Création du préfixe Wine..."
    
    if ! wineboot_init_prefix; then
        restore_backup
        progress_close "$DBUS_REF"
        error_exit "Échec de l'initialisation du préfixe (wineboot)"
    fi
    
    check_progress_cancelled "$DBUS_REF" --with-backup
    
    ((CURRENT_STEP++))
    progress_update "$DBUS_REF" "$CURRENT_STEP" "Installation de Wine Mono et Gecko..."
    
    if ! install_wine_mono_gecko; then
        restore_backup
        progress_close "$DBUS_REF"
        error_exit "Échec de l'installation de Mono/Gecko"
    fi
    
    check_progress_cancelled "$DBUS_REF"
    
    ((CURRENT_STEP++))
    
    if ! install_winetricks_components; then
        restore_backup
        progress_close "$DBUS_REF"
        error_exit "Échec de l'installation des composants Windows"
    fi
    
    check_progress_cancelled "$DBUS_REF"
    
    ((CURRENT_STEP++))
    progress_update "$DBUS_REF" "$CURRENT_STEP" "Installation de DXVK et VKD3D..."
    
    if ! install_dxvk_vkd3d; then
        restore_backup
        progress_close "$DBUS_REF"
        error_exit "Échec de l'installation de DXVK/VKD3D"
    fi
    
    check_progress_cancelled "$DBUS_REF"
    
    ((CURRENT_STEP++))
    progress_update "$DBUS_REF" "$CURRENT_STEP" "Configuration de MangoHud..."
    
    if ! copy_mangohud_config; then
        restore_backup
        progress_close "$DBUS_REF"
        error_exit "Échec de la copie de la config MangoHud"
    fi
    
    check_progress_cancelled "$DBUS_REF"
    
    ((CURRENT_STEP++))
    progress_update "$DBUS_REF" "$CURRENT_STEP" "Configuration des dossiers Windows..."
    
    if ! setup_windows_directories "$WINEPREFIX"; then
        restore_backup
        progress_close "$DBUS_REF"
        error_exit "Échec de la configuration des dossiers Windows"
    fi
    
    setup_wine_temp_symlinks "$WINEPREFIX"
    
    configure_sdl_input_registry
    
    check_progress_cancelled "$DBUS_REF"
    
    ((CURRENT_STEP++))
    progress_update "$DBUS_REF" "$CURRENT_STEP" "Configuration des caches..."
    ensure_dirs -s "$SHADER_CACHE_DIR/dxvk" "$SHADER_CACHE_DIR/vkd3d" "$SHADER_CACHE_DIR/nvidia" "$SHADER_CACHE_DIR/mesa"
    
    if progress_is_cancelled "$DBUS_REF"; then
        restore_backup
        progress_close "$DBUS_REF"
        echo "Initialisation annulée par l'utilisateur"
        if [ "$_USE_KDIALOG" = "true" ] && command -v kdialog &>/dev/null; then
            kdialog --title "Initialisation annulée" --msgbox "L'initialisation du préfixe a été annulée.\n\nL'ancien préfixe a été restauré."
        fi
        exit 0
    fi
    
    progress_update "$DBUS_REF" "$TOTAL_STEPS" "Finalisation..."
    
    if [ -n "$PREFIX_BACKUP_PATH" ] && [ -d "$PREFIX_BACKUP_PATH" ]; then
        rm -rf "$PREFIX_BACKUP_PATH"
    fi
    
    progress_close "$DBUS_REF"
    
    # Afficher le mode DXVK configuré
    local current_dxvk_mode
    current_dxvk_mode=$(get_current_dxvk_mode)
    
    echo ""
    echo "Préfixe Wine initialisé avec succès !"
    echo ""
    echo "Configuration effectuée :"
    echo "  ✓ Préfixe Wine créé"
    echo "  ✓ Runner: $current_runner"
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
