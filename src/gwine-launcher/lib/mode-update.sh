#!/bin/bash

################################################################################
# mode-update.sh - Mode --update : Mise à jour des composants
################################################################################

# =============================================================================
# Mode --update : Mise à jour des composants
# =============================================================================

update_components() {
    echo "Mise à jour des composants..."
    echo ""
    
    # Vérifier la connexion réseau
    if ! curl -s --max-time 5 https://github.com > /dev/null 2>&1; then
        error_exit "Pas de connexion internet - mise à jour impossible"
    fi
    
    # Obtenir le runner actuel
    local current_runner
    current_runner=$(get_current_runner)
    
    echo "Runner actuel: $current_runner"
    echo ""
    
    local has_updates=false
    local needs_reinstall_dlls=false
    local CURRENT_STEP=0
    local DBUS_REF=""
    
    # Variables pour stocker les mises à jour effectuées
    local updated_runner=""
    local updated_dxvk=""
    local updated_vkd3d=""
    local updated_dlls=false
    
    # Vérifier le runner actuel (wine ou proton)
    local latest_runner
    local current_runner_version
    
    if [ "$current_runner" = "proton" ]; then
        latest_runner=$(get_latest_gwine_proton_version)
        current_runner_version=$(cat "$PROTON_RUNNER_VERSION_FILE" 2>/dev/null || echo "")
        
        echo "gwine-proton:"
        echo "  Version actuelle: ${current_runner_version:-Aucune}"
        echo "  Dernière version: ${latest_runner:-Inconnue}"
        
        if [ -n "$latest_runner" ] && [ -n "$current_runner_version" ]; then
            if compare_versions "$latest_runner" "$current_runner_version"; then
                has_updates=true
                echo "  → Mise à jour disponible"
            else
                echo "  → Déjà à jour"
            fi
        elif [ -n "$latest_runner" ]; then
            has_updates=true
            echo "  → Installation nécessaire"
        fi
    else
        # Runner wine par défaut
        latest_runner=$(get_latest_gwine_version)
        current_runner_version=$(cat "$WINE_RUNNER_VERSION_FILE" 2>/dev/null || echo "")
        
        echo "gwine:"
        echo "  Version actuelle: ${current_runner_version:-Aucune}"
        echo "  Dernière version: ${latest_runner:-Inconnue}"
        
        if [ -n "$latest_runner" ] && [ -n "$current_runner_version" ]; then
            if compare_versions "$latest_runner" "$current_runner_version"; then
                has_updates=true
                echo "  → Mise à jour disponible"
            else
                echo "  → Déjà à jour"
            fi
        elif [ -n "$latest_runner" ]; then
            has_updates=true
            echo "  → Installation nécessaire"
        fi
    fi
    
    # Vérifier DXVK/VKD3D (standard ou async selon le mode)
    local current_dxvk_mode
    current_dxvk_mode=$(get_current_dxvk_mode)
    
    local latest_dxvk=""
    local latest_vkd3d
    latest_vkd3d=$(get_latest_vkd3d_version)
    local current_dxvk=""
    local current_vkd3d=""
    local needs_dxvk_async_update=false
    local latest_dxvk_async=""
    local current_dxvk_async=""
    
    local vkd3d_folder
    vkd3d_folder=$(find "$VKD3D_CACHE_DIR" -maxdepth 1 -type d -name "vkd3d-proton-*" 2>/dev/null | sort -V | tail -1)
    [ -n "$vkd3d_folder" ] && current_vkd3d=$(basename "$vkd3d_folder" | sed 's/^vkd3d-proton-//')
    
    if [ "$current_dxvk_mode" = "dxvk-async" ]; then
        # Mode dxvk-async: vérifier dxvk-gplasync
        local dxvk_async_folder
        dxvk_async_folder=$(find "$DXVK_ASYNC_CACHE_DIR" -maxdepth 1 -type d -name "dxvk*gplasync*" 2>/dev/null | sort -V | tail -1)
        [ -n "$dxvk_async_folder" ] && current_dxvk_async=$(basename "$dxvk_async_folder" | sed 's/^dxvk-gplasync-//')
        
        latest_dxvk_async=$(get_latest_dxvk_async_version)
        
        echo ""
        echo "DXVK-GPLAsync:"
        echo "  Version actuelle: ${current_dxvk_async:-Aucune}"
        echo "  Dernière version: ${latest_dxvk_async:-Inconnue}"
        
        if [ -n "$latest_dxvk_async" ]; then
            if [ -z "$current_dxvk_async" ] || compare_versions "$latest_dxvk_async" "$current_dxvk_async"; then
                has_updates=true
                needs_dxvk_async_update=true
                needs_reinstall_dlls=true
                echo "  → Mise à jour disponible"
            else
                echo "  → Déjà à jour"
            fi
        fi
    else
        # Mode standard: vérifier dxvk normal
        latest_dxvk=$(get_latest_dxvk_version)
        
        local dxvk_folder
        dxvk_folder=$(find "$DXVK_CACHE_DIR" -maxdepth 1 -type d -name "dxvk-*" 2>/dev/null | grep -v "gplasync\|nvapi" | sort -V | tail -1)
        [ -n "$dxvk_folder" ] && current_dxvk=$(basename "$dxvk_folder" | sed 's/^dxvk-//')
        
        echo ""
        echo "DXVK:"
        echo "  Version actuelle: ${current_dxvk:-Aucune}"
        echo "  Dernière version: ${latest_dxvk:-Inconnue}"
        
        if [ -n "$latest_dxvk" ]; then
            if [ -z "$current_dxvk" ] || compare_versions "$latest_dxvk" "$current_dxvk"; then
                has_updates=true
                needs_reinstall_dlls=true
                echo "  → Mise à jour disponible"
            else
                echo "  → Déjà à jour"
            fi
        fi
    fi
    
    echo ""
    echo "VKD3D-Proton:"
    echo "  Version actuelle: ${current_vkd3d:-Aucune}"
    echo "  Dernière version: ${latest_vkd3d:-Inconnue}"
    
    if [ -n "$latest_vkd3d" ]; then
        if [ -z "$current_vkd3d" ] || compare_versions "$latest_vkd3d" "$current_vkd3d"; then
            has_updates=true
            needs_reinstall_dlls=true
            echo "  → Mise à jour disponible"
        else
            echo "  → Déjà à jour"
        fi
    fi
    
    # Vérifier DXVK-NVAPI (NVIDIA uniquement)
    local needs_nvapi_update=false
    local updated_nvapi=""
    local latest_nvapi=""
    
    if is_nvidia_gpu; then
        echo ""
        echo "DXVK-NVAPI (NVIDIA):"
        latest_nvapi=$(get_latest_dxvk_nvapi_version)
        local current_nvapi=""
        local nvapi_folder
        nvapi_folder=$(find "$DXVK_NVAPI_CACHE_DIR" -maxdepth 1 -type d -name "dxvk-nvapi-*" 2>/dev/null | sort -V | tail -1)
        [ -n "$nvapi_folder" ] && current_nvapi=$(basename "$nvapi_folder" | sed 's/^dxvk-nvapi-v//; s/^dxvk-nvapi-//')
        
        echo "  Version actuelle: ${current_nvapi:-Aucune}"
        echo "  Dernière version: ${latest_nvapi:-Inconnue}"
        
        if [ -n "$latest_nvapi" ]; then
            if [ -z "$current_nvapi" ] || compare_versions "$latest_nvapi" "$current_nvapi"; then
                has_updates=true
                needs_nvapi_update=true
                echo "  → Mise à jour disponible"
            else
                echo "  → Déjà à jour"
            fi
        fi
    fi
    
    echo ""
    
    # Si pas de mises à jour disponibles
    if [ "$has_updates" = false ]; then
        echo "Tous les composants sont déjà à jour !"
        echo ""
        echo "Versions installées :"
        if [ "$current_runner" = "proton" ]; then
            echo "  - gwine-proton: ${current_runner_version:-Inconnue}"
        else
            echo "  - gwine: ${current_runner_version:-Inconnue}"
        fi
        # Afficher le bon DXVK selon le mode
        if [ "$current_dxvk_mode" = "dxvk-async" ]; then
            echo "  - DXVK-GPLAsync: ${current_dxvk_async:-Inconnue}"
        else
            echo "  - DXVK: ${current_dxvk:-Inconnue}"
        fi
        echo "  - VKD3D-Proton: ${current_vkd3d:-Inconnue}"
        if is_nvidia_gpu; then
            echo "  - DXVK-NVAPI: ${current_nvapi:-Inconnue}"
        fi
        echo ""
        exit 0
    fi
    
    # Compter le nombre total d'étapes
    local TOTAL_STEPS=0
    [ -n "$latest_gwine" ] && ((TOTAL_STEPS++))
    [ -n "$latest_dxvk" ] || [ -n "$latest_vkd3d" ] && ((TOTAL_STEPS++))
    [ "$needs_dxvk_async_update" = true ] && ((TOTAL_STEPS++))
    [ "$needs_nvapi_update" = true ] && ((TOTAL_STEPS++))
    [ "$needs_reinstall_dlls" = true ] && ((TOTAL_STEPS++))
    
    # Créer la barre de progression (sans bouton Annuler pour --update, uniquement avec --kdialog)
    if [ "$_USE_KDIALOG" = "true" ] && [ $TOTAL_STEPS -gt 0 ] && command -v kdialog &>/dev/null && command -v qdbus &>/dev/null; then
        DBUS_REF=$(progress_create "Mise à jour des composants" "$TOTAL_STEPS" "false")
    fi
    
    # Effectuer les mises à jour
    echo "Installation des mises à jour..."
    
    # Mettre à jour le runner actuel (wine ou proton)
    if [ -n "$latest_runner" ]; then
        if [ -z "$current_runner_version" ] || compare_versions "$latest_runner" "$current_runner_version"; then
            ((CURRENT_STEP++))
            
            if [ "$current_runner" = "proton" ]; then
                progress_update "$DBUS_REF" "$CURRENT_STEP" "Téléchargement de gwine-proton..."
                
                if progress_is_cancelled "$DBUS_REF"; then
                    progress_close "$DBUS_REF"
                    echo "Mise à jour annulée par l'utilisateur"
                    exit 0
                fi
                
                if ! download_gwine_proton "false" "true"; then
                    progress_close "$DBUS_REF"
                    error_exit "Échec de la mise à jour de gwine-proton"
                fi
                
                updated_runner="gwine-proton $latest_runner"
            else
                progress_update "$DBUS_REF" "$CURRENT_STEP" "Téléchargement de gwine..."
                
                if progress_is_cancelled "$DBUS_REF"; then
                    progress_close "$DBUS_REF"
                    echo "Mise à jour annulée par l'utilisateur"
                    exit 0
                fi
                
                if ! download_gwine "false" "true"; then
                    progress_close "$DBUS_REF"
                    error_exit "Échec de la mise à jour de gwine"
                fi
                
                updated_runner="gwine $latest_runner"
            fi
            
            # Vérifier annulation après téléchargement
            if progress_is_cancelled "$DBUS_REF"; then
                progress_close "$DBUS_REF"
                echo "Mise à jour annulée par l'utilisateur"
                exit 0
            fi
        fi
    fi
    
    # Mettre à jour DXVK (standard ou async selon le mode) et VKD3D
    if [ "$needs_dxvk_async_update" = true ]; then
        # Mettre à jour DXVK-GPLAsync
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Téléchargement de DXVK-GPLAsync..."
        
        if progress_is_cancelled "$DBUS_REF"; then
            progress_close "$DBUS_REF"
            echo "Mise à jour annulée par l'utilisateur"
            exit 0
        fi
        
        if ! download_dxvk_async "false" "true"; then
            progress_close "$DBUS_REF"
            error_exit "Échec de la mise à jour de DXVK-GPLAsync"
        fi
        
        updated_dxvk="DXVK-GPLAsync $latest_dxvk_async"
        
        # Vérifier annulation après téléchargement
        if progress_is_cancelled "$DBUS_REF"; then
            progress_close "$DBUS_REF"
            echo "Mise à jour annulée par l'utilisateur"
            exit 0
        fi
    elif [ -n "$latest_dxvk" ] || [ -n "$latest_vkd3d" ]; then
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Téléchargement de DXVK/VKD3D-Proton..."
        
        if progress_is_cancelled "$DBUS_REF"; then
            progress_close "$DBUS_REF"
            echo "Mise à jour annulée par l'utilisateur"
            exit 0
        fi
        
        if ! download_updated_dxvk_vkd3d --no-confirm; then
            progress_close "$DBUS_REF"
            error_exit "Échec de la mise à jour de DXVK/VKD3D"
        fi
        
        # Marquer comme mis à jour
        [ -n "$latest_dxvk" ] && updated_dxvk="$latest_dxvk"
        [ -n "$latest_vkd3d" ] && updated_vkd3d="$latest_vkd3d"
        
        # Vérifier annulation après téléchargement
        if progress_is_cancelled "$DBUS_REF"; then
            progress_close "$DBUS_REF"
            echo "Mise à jour annulée par l'utilisateur"
            exit 0
        fi
    fi
    
    # Mettre à jour DXVK-NVAPI si nécessaire (NVIDIA)
    if [ "$needs_nvapi_update" = true ]; then
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Téléchargement de DXVK-NVAPI..."
        
        if progress_is_cancelled "$DBUS_REF"; then
            progress_close "$DBUS_REF"
            echo "Mise à jour annulée par l'utilisateur"
            exit 0
        fi
        
        if ! download_dxvk_nvapi --no-confirm; then
            progress_close "$DBUS_REF"
            echo "Warning: Échec de la mise à jour de DXVK-NVAPI"
        else
            updated_nvapi="$latest_nvapi"
        fi
        
        # Vérifier annulation après téléchargement
        if progress_is_cancelled "$DBUS_REF"; then
            progress_close "$DBUS_REF"
            echo "Mise à jour annulée par l'utilisateur"
            exit 0
        fi
    fi
    
    # Réinstaller les DLLs dans le préfixe si nécessaire
    if [ "$needs_reinstall_dlls" = true ] && [ -d "$HOME_REAL/Windows/Prefix" ]; then
        ((CURRENT_STEP++))
        progress_update "$DBUS_REF" "$CURRENT_STEP" "Réinstallation des DLLs..."
        
        if progress_is_cancelled "$DBUS_REF"; then
            progress_close "$DBUS_REF"
            echo "Mise à jour annulée par l'utilisateur"
            exit 0
        fi
        
        export WINEPREFIX="$HOME_REAL/Windows/Prefix"
        if ! install_dxvk_vkd3d; then
            progress_close "$DBUS_REF"
            error_exit "Échec de la réinstallation des DLLs"
        fi
        
        updated_dlls=true
    fi
    
    # Finaliser
    progress_update "$DBUS_REF" "$TOTAL_STEPS" "Finalisation..."
    progress_close "$DBUS_REF"
    
    # Afficher le résumé des mises à jour
    echo ""
    echo "Mise à jour terminée avec succès !"
    echo ""
    echo "Résumé des mises à jour effectuées :"
    if [ -n "$updated_runner" ]; then
        echo "  ✓ $updated_runner mis à jour"
    fi
    if [ -n "$updated_dxvk" ]; then
        echo "  ✓ DXVK mis à jour vers $updated_dxvk"
    fi
    if [ -n "$updated_vkd3d" ]; then
        echo "  ✓ VKD3D-Proton mis à jour vers $updated_vkd3d"
    fi
    if [ -n "$updated_nvapi" ]; then
        echo "  ✓ DXVK-NVAPI mis à jour vers $updated_nvapi"
    fi
    if [ "$updated_dlls" = true ]; then
        echo "  ✓ DLLs réinstallés dans le préfixe Wine"
    fi
    echo ""
}
