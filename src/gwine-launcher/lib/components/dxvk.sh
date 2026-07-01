#!/bin/bash

################################################################################
# component-dxvk.sh - Installation de DXVK et VKD3D-Proton
################################################################################

# Installe DXVK et VKD3D-Proton dans le préfixe Wine depuis le cache local
install_dxvk_vkd3d() {
    prepare_local_cache
    
    # Déterminer quel DXVK utiliser selon le mode configuré
    local dxvk_cache_dir
    dxvk_cache_dir=$(get_dxvk_cache_dir)
    local dxvk_mode
    dxvk_mode=$(get_current_dxvk_mode)
    
    if [ "$dxvk_mode" = "dxvk-async" ]; then
        echo "Installation de DXVK-GPLAsync et VKD3D-Proton depuis le cache..."
    else
        echo "Installation de DXVK et VKD3D-Proton depuis le cache..."
    fi
    
    if ! get_wine_system_paths; then
        return 1
    fi
    
    local installation_failed=false
    local dxvk_installed=false
    local vkd3d_installed=false
    
    # Installer DXVK (standard ou async selon le mode)
    if [ "$dxvk_mode" = "dxvk-async" ]; then
        if install_dll_component "DXVK-GPLAsync" "$dxvk_cache_dir" "dxvk-gplasync-*" "d3d8.dll d3d9.dll d3d10core.dll d3d11.dll dxgi.dll" "d3d8 d3d9 d3d10core d3d11 dxgi"; then
            dxvk_installed=true
        fi
    else
        if install_dll_component "DXVK" "$dxvk_cache_dir" "dxvk-[0-9]*" "d3d8.dll d3d9.dll d3d10core.dll d3d11.dll dxgi.dll" "d3d8 d3d9 d3d10core d3d11 dxgi"; then
            dxvk_installed=true
        fi
    fi
    
    # Installer VKD3D-Proton
    if install_dll_component "VKD3D-Proton" "$VKD3D_CACHE_DIR" "vkd3d-proton-*" "d3d12.dll d3d12core.dll" "d3d12 d3d12core"; then
        vkd3d_installed=true
    fi
    
    if [ "$dxvk_installed" = false ] && [ "$vkd3d_installed" = false ]; then
        echo "Erreur: Ni DXVK ni VKD3D-Proton n'ont pu être installés"
        return 1
    fi
    
    if [ "$dxvk_mode" = "dxvk-async" ]; then
        echo "DXVK-GPLAsync et VKD3D-Proton installés avec succès"
    else
        echo "DXVK et VKD3D-Proton installés avec succès"
    fi
    
    # Installer DXVK-NVAPI si carte NVIDIA présente
    if is_nvidia_gpu; then
        install_dxvk_nvapi
    fi
    
    return 0
}
