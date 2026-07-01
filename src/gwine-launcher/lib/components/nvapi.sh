#!/bin/bash

################################################################################
# component-nvapi.sh - Gestion de DXVK-NVAPI (NVIDIA uniquement)
################################################################################

# Installe DXVK-NVAPI dans le préfixe Wine depuis le cache local (uniquement pour NVIDIA)
install_dxvk_nvapi() {
    if ! is_nvidia_gpu; then
        return 0
    fi
    
    echo "Installation de DXVK-NVAPI depuis le cache..."
    
    if ! get_wine_system_paths; then
        return 1
    fi
    
    install_dll_component "DXVK-NVAPI" "$DXVK_NVAPI_CACHE_DIR" "dxvk-nvapi-*" "nvapi64.dll nvapi.dll" "nvapi nvapi64" || true
    
    return 0
}

# Télécharge DXVK-NVAPI (uniquement pour NVIDIA)
download_dxvk_nvapi() {
    local target_version="${1:-}"
    local no_confirm=false
    
    if [ "$1" = "--no-confirm" ]; then
        no_confirm=true
        target_version=""
    fi
    
    ensure_dir "$DXVK_NVAPI_CACHE_DIR"
    
    if [ -z "$target_version" ]; then
        target_version=$(get_latest_dxvk_nvapi_version)
    fi
    
    if [ -z "$target_version" ]; then
        echo "Impossible de récupérer la version de DXVK-NVAPI"
        return 1
    fi
    
    local current_nvapi=""
    local nvapi_folder
    nvapi_folder=$(find_component_dir "$DXVK_NVAPI_CACHE_DIR" "dxvk-nvapi-*")
    [ -n "$nvapi_folder" ] && current_nvapi=$(basename "$nvapi_folder" | sed 's/^dxvk-nvapi-v//; s/^dxvk-nvapi-//')
    
    if [ -n "$current_nvapi" ] && [ "$current_nvapi" = "$target_version" ]; then
        echo "DXVK-NVAPI $target_version est déjà installé"
        return 0
    fi
    
    echo "DXVK-NVAPI - Installé: ${current_nvapi:-Aucun}, Cible: $target_version"
    
    local nvapi_tag="dxvk-nvapi-v${target_version}"
    local nvapi_url="https://github.com/bottlesdevs/components/releases/download/${nvapi_tag}/${nvapi_tag}.tar.gz"
    
    _do_download() {
        download_github_component "$DXVK_NVAPI_CACHE_DIR" "dxvk-nvapi" "$target_version" "$nvapi_url" "tar.gz" "$no_confirm"
    }
    
    if update_component_with_backup "$DXVK_NVAPI_CACHE_DIR" "dxvk-nvapi-*" _do_download; then
        echo "✓ DXVK-NVAPI $target_version installé"
        return 0
    else
        echo "✗ Échec de l'installation de DXVK-NVAPI"
        return 1
    fi
}
