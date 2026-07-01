#!/bin/bash

################################################################################
# dxvk-vkd3d.sh - Téléchargement et mise à jour de DXVK et VKD3D-Proton
################################################################################

download_updated_dxvk_vkd3d() {
    local current_dxvk=""
    local current_vkd3d=""
    local latest_dxvk=""
    local latest_vkd3d=""
    local no_confirm=false
    
    if [ "$1" = "--no-confirm" ]; then
        no_confirm=true
    fi
    
    ensure_dirs "$DXVK_CACHE_DIR" "$VKD3D_CACHE_DIR"
    
    local dxvk_folder
    dxvk_folder=$(find "$DXVK_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "dxvk-*" 2>/dev/null | grep -v "gplasync\|nvapi" | sort -V | tail -1)
    [ -n "$dxvk_folder" ] && current_dxvk=$(basename "$dxvk_folder" | sed 's/^dxvk-//')
    
    local vkd3d_folder
    vkd3d_folder=$(find "$VKD3D_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "vkd3d-proton-*" 2>/dev/null | sort -V | tail -1)
    [ -n "$vkd3d_folder" ] && current_vkd3d=$(basename "$vkd3d_folder" | sed 's/^vkd3d-proton-//')
    
    echo ""
    echo "Vérification des mises à jour de DXVK et VKD3D-Proton..."
    
    latest_dxvk=$(get_latest_dxvk_version)
    latest_vkd3d=$(get_latest_vkd3d_version)
    
    if [ -z "$latest_dxvk" ] || [ -z "$latest_vkd3d" ]; then
        echo "Impossible de récupérer les dernières versions"
        return 1
    fi
    
    echo "DXVK - Installé: ${current_dxvk:-Aucun}, Dernière: $latest_dxvk"
    echo "VKD3D - Installé: ${current_vkd3d:-Aucun}, Dernière: $latest_vkd3d"
    
    local current_nvapi=""
    local latest_nvapi=""
    local needs_nvapi_update=false
    
    if is_nvidia_gpu; then
        latest_nvapi=$(get_latest_dxvk_nvapi_version)
        local nvapi_folder
        nvapi_folder=$(find "$DXVK_NVAPI_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "dxvk-nvapi-*" 2>/dev/null | sort -V | tail -1)
        [ -n "$nvapi_folder" ] && current_nvapi=$(basename "$nvapi_folder" | sed 's/^dxvk-nvapi-v//; s/^dxvk-nvapi-//')
        echo ""
        echo "DXVK-NVAPI (NVIDIA) - Installé: ${current_nvapi:-Aucun}, Dernière: ${latest_nvapi:-Inconnue}"
    fi
    
    if [ -n "$current_dxvk" ] && ! compare_versions "$latest_dxvk" "$current_dxvk"; then
        latest_dxvk=""
    fi
    
    if [ -n "$current_vkd3d" ] && ! compare_versions "$latest_vkd3d" "$current_vkd3d"; then
        latest_vkd3d=""
    fi
    
    if is_nvidia_gpu && [ -n "$latest_nvapi" ]; then
        if [ -z "$current_nvapi" ] || compare_versions "$latest_nvapi" "$current_nvapi"; then
            needs_nvapi_update=true
        fi
    fi
    
    if [ -z "$latest_dxvk" ] && [ -z "$latest_vkd3d" ] && [ "$needs_nvapi_update" = false ]; then
        echo "Vous avez déjà les dernières versions des composants."
        return 0
    fi
    
    local temp_dir="$CACHE_DIR/.temp_download"
    ensure_dir -s "$temp_dir"
    
    local update_success=true
    
    if [ -n "$latest_dxvk" ]; then
        local dxvk_url
        if [ "${_DXVK_SOURCE:-official}" = "bottles" ]; then
            local dxvk_tag="dxvk-${latest_dxvk}"
            dxvk_url="https://github.com/bottlesdevs/components/releases/download/${dxvk_tag}/${dxvk_tag}.tar.gz"
        else
            local dxvk_tag="v${latest_dxvk}"
            dxvk_url="https://github.com/doitsujin/dxvk/releases/download/${dxvk_tag}/dxvk-${latest_dxvk}.tar.gz"
        fi
        if ! download_and_install_component "dxvk" "$latest_dxvk" "$DXVK_CACHE_DIR" "dxvk-*" "$dxvk_url" "$temp_dir"; then
            update_success=false
        fi
    fi
    
    if [ -n "$latest_vkd3d" ]; then
        local vkd3d_url
        if [ "${_VKD3D_SOURCE:-official}" = "bottles" ]; then
            local vkd3d_tag="vkd3d-proton-${latest_vkd3d}"
            vkd3d_url="https://github.com/bottlesdevs/components/releases/download/${vkd3d_tag}/${vkd3d_tag}.tar.gz"
        else
            local vkd3d_tag="v${latest_vkd3d}"
            vkd3d_url="https://github.com/HansKristian-Work/vkd3d-proton/releases/download/${vkd3d_tag}/vkd3d-proton-${latest_vkd3d}.tar.gz"
        fi
        if ! download_and_install_component "vkd3d-proton" "$latest_vkd3d" "$VKD3D_CACHE_DIR" "vkd3d-proton-*" "$vkd3d_url" "$temp_dir"; then
            update_success=false
        fi
    fi
    
    rm -rf "$temp_dir"
    
    if [ "$needs_nvapi_update" = true ]; then
        echo ""
        echo "Téléchargement de DXVK-NVAPI $latest_nvapi..."
        if ! download_dxvk_nvapi --no-confirm; then
            echo "✗ Échec du téléchargement de DXVK-NVAPI"
            update_success=false
        fi
    fi
    
    if [ "$update_success" = true ]; then
        echo ""
        echo "Mises à jour installées avec succès !"
        if [ -d "$WINEPREFIX" ] && [ "${init_mode:-false}" != "true" ]; then
            echo "Réinstallation des DLLs dans le préfixe Wine..."
            install_dxvk_vkd3d
        fi
    fi
    
    return 0
}
