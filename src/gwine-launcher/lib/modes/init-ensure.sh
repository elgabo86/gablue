#!/bin/bash

################################################################################
# init-ensure.sh - Fonctions de vérification/création du préfixe
################################################################################

ensure_wineprefix() {
    if [ ! -d "$WINEPREFIX" ] || [ ! -f "$WINEPREFIX/system.reg" ]; then
        echo "Préfixe Wine non trouvé, création automatique..."
        
        local DBUS_REF=""
        if command -v kdialog &>/dev/null && [ -n "$(_get_qdbus_cmd)" ]; then
            DBUS_REF=$(progress_create "Création du préfixe Wine" 3 "true" "true")
        fi
        
        progress_update "$DBUS_REF" 1 "Préparation..."
        prepare_local_cache
        
        check_progress_cancelled "$DBUS_REF"
        
        progress_update "$DBUS_REF" 2 "Création du préfixe..."
        if ! wineboot_init_prefix; then
            progress_close "$DBUS_REF"
            echo "Erreur: échec de l'initialisation du préfixe Wine" >&2
            return 1
        fi
        
        check_progress_cancelled "$DBUS_REF"
        
        progress_update "$DBUS_REF" 3 "Installation de Mono et Gecko..."
        if ! install_wine_mono_gecko; then
            echo "Attention: échec de l'installation de Mono/Gecko" >&2
        fi
        
        check_progress_cancelled "$DBUS_REF"
        
        progress_close "$DBUS_REF"
    fi
}

ensure_wineprefix_full() {
    if [ ! -d "$WINEPREFIX" ] || [ ! -f "$WINEPREFIX/system.reg" ]; then
        echo "Préfixe Wine non trouvé, création automatique..."
        
        local TOTAL_STEPS=5
        if is_nvidia_gpu; then
            TOTAL_STEPS=6
        fi
        local CURRENT_STEP=0
        local DBUS_REF=""
        
        if command -v kdialog &>/dev/null && [ -n "$(_get_qdbus_cmd)" ]; then
            DBUS_REF=$(progress_create "Création du préfixe Wine" "$TOTAL_STEPS" "true" "true")
        fi
        
        check_progress_cancelled "$DBUS_REF"
        
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Préparation des composants..."
        prepare_local_cache
        
        check_progress_cancelled "$DBUS_REF"
        
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Création du préfixe..."
        if ! wineboot_init_prefix; then
            progress_close "$DBUS_REF"
            echo "Erreur: échec de l'initialisation du préfixe Wine" >&2
            return 1
        fi
        
        check_progress_cancelled "$DBUS_REF"
        
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Installation de Mono et Gecko..."
        if ! install_wine_mono_gecko; then
            echo "Attention: échec de l'installation de Mono/Gecko" >&2
        fi
        
        check_progress_cancelled "$DBUS_REF"
        
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Installation des composants Windows..."
        install_winetricks_components
        
        check_progress_cancelled "$DBUS_REF"
        
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Installation de DXVK/VKD3D..."
        install_dxvk_vkd3d
        
        check_progress_cancelled "$DBUS_REF"
        
        if is_nvidia_gpu; then
            ((CURRENT_STEP++))
            progress_update "$DBUS_REF" "$CURRENT_STEP" "Installation de DXVK-NVAPI (NVIDIA)..."
            
            local latest_nvapi
            latest_nvapi=$(get_latest_dxvk_nvapi_version)
            local current_nvapi=""
            local nvapi_folder
            nvapi_folder=$(find "$DXVK_NVAPI_CACHE_DIR" -maxdepth 1 -type d -name "dxvk-nvapi-*" 2>/dev/null | sort -V | tail -1)
            [ -n "$nvapi_folder" ] && current_nvapi=$(basename "$nvapi_folder" | sed 's/^dxvk-nvapi-v//; s/^dxvk-nvapi-//')
            
            if [ -z "$current_nvapi" ] || { [ -n "$latest_nvapi" ] && compare_versions "$latest_nvapi" "$current_nvapi"; }; then
                download_dxvk_nvapi --no-confirm 2>/dev/null || true
            fi
            
            install_dxvk_nvapi 2>/dev/null || true
            
            check_progress_cancelled "$DBUS_REF"
        fi
        
        progress_close "$DBUS_REF"
        
        echo "Préfixe créé avec succès"
    fi
}
