#!/bin/bash

################################################################################
# offline.sh - Préparation du cache et mode offline
################################################################################

download_missing_components() {
    echo "Téléchargement des composants manquants..."
    
    local MONO_VER="11.2.0"
    local GECKO_VER="2.47.4"
    local WINE_CACHE_DIR="$COMPONENTS_DIR/wine-cache"
    local MONO_FILE="$WINE_CACHE_DIR/wine-mono-${MONO_VER}-x86.msi"
    local GECKO64_FILE="$WINE_CACHE_DIR/wine-gecko-${GECKO_VER}-x86_64.msi"
    local GECKO32_FILE="$WINE_CACHE_DIR/wine-gecko-${GECKO_VER}-x86.msi"
    
    if [ ! -f "$MONO_FILE" ] || [ ! -f "$GECKO64_FILE" ] || [ ! -f "$GECKO32_FILE" ]; then
        echo "Téléchargement de Wine Mono et Wine Gecko..."
        local MONO_URL="https://dl.winehq.org/wine/wine-mono/${MONO_VER}/wine-mono-${MONO_VER}-x86.msi"
        local GECKO_URL="https://dl.winehq.org/wine/wine-gecko/${GECKO_VER}/wine-gecko-${GECKO_VER}-x86_64.msi"
        local GECKO32_URL="https://dl.winehq.org/wine/wine-gecko/${GECKO_VER}/wine-gecko-${GECKO_VER}-x86.msi"
        
        mkdir -p "$WINE_CACHE_DIR"
        
        rm -f "$WINE_CACHE_DIR"/wine-mono-*.msi "$WINE_CACHE_DIR"/wine-gecko-*.msi
        
        wget -q --show-progress "$MONO_URL" -O "$MONO_FILE" 2>&1 || echo "Warning: Échec du téléchargement de Wine Mono"
        wget -q --show-progress "$GECKO_URL" -O "$GECKO64_FILE" 2>&1 || echo "Warning: Échec du téléchargement de Wine Gecko (x64)"
        wget -q --show-progress "$GECKO32_URL" -O "$GECKO32_FILE" 2>&1 || echo "Warning: Échec du téléchargement de Wine Gecko (x86)"
    fi
    
    ensure_dirs "$DXVK_CACHE_DIR" "$VKD3D_CACHE_DIR"
    
    local has_dxvk=false
    find "$DXVK_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "dxvk-*" 2>/dev/null | grep -qv "gplasync\|nvapi" && has_dxvk=true
    
    local has_vkd3d=false
    find "$VKD3D_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "vkd3d-proton-*" 2>/dev/null | grep -q . && has_vkd3d=true
    
    if [ "$has_dxvk" = false ]; then
        echo "Téléchargement de DXVK..."
        
        local DXVK_VERSION
        DXVK_VERSION=$(get_latest_dxvk_version)
        if [ -z "$DXVK_VERSION" ]; then
            echo "Warning: Impossible de récupérer la dernière version de DXVK"
        else
            local DXVK_TAG DXVK_URL
            if [ "${_DXVK_SOURCE:-official}" = "bottles" ]; then
                DXVK_TAG="dxvk-${DXVK_VERSION}"
                DXVK_URL="https://github.com/bottlesdevs/components/releases/download/${DXVK_TAG}/${DXVK_TAG}.tar.gz"
            else
                DXVK_TAG="v${DXVK_VERSION}"
                DXVK_URL="https://github.com/doitsujin/dxvk/releases/download/${DXVK_TAG}/dxvk-${DXVK_VERSION}.tar.gz"
            fi
            local DXVK_TEMP="$DXVK_CACHE_DIR/dxvk.tar.gz"
            local DXVK_DIR="$DXVK_CACHE_DIR/dxvk-${DXVK_VERSION}"
            
            wget -q --show-progress "$DXVK_URL" -O "$DXVK_TEMP" 2>&1 || echo "Warning: Échec du téléchargement de DXVK"
            if [ -f "$DXVK_TEMP" ]; then
                ensure_dir -s "$DXVK_DIR"
                local temp_extract="$DXVK_CACHE_DIR/.temp_dxvk"
                rm -rf "$temp_extract"
                ensure_dir -s "$temp_extract"
                tar -xzf "$DXVK_TEMP" -C "$temp_extract" 2>/dev/null || true
                local extracted_dir
                extracted_dir=$(find "$temp_extract" -mindepth 1 -maxdepth 1 -type d | head -1)
                if [ -n "$extracted_dir" ]; then
                    cp -r "$extracted_dir"/* "$DXVK_DIR/" 2>/dev/null || true
                fi
                rm -rf "$temp_extract" "$DXVK_TEMP"
            fi
        fi
    fi
    
    if [ "$has_vkd3d" = false ]; then
        echo "Téléchargement de VKD3D-Proton..."
        
        local old_vkd3d_dir
        old_vkd3d_dir=$(find "$VKD3D_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "vkd3d-proton-*" | head -1)
        if [ -n "$old_vkd3d_dir" ]; then
            echo "Suppression de l'ancienne version VKD3D-Proton: $(basename "$old_vkd3d_dir")"
            rm -rf "$old_vkd3d_dir"
        fi
        
        local VKD3D_VERSION
        VKD3D_VERSION=$(get_latest_vkd3d_version)
        if [ -z "$VKD3D_VERSION" ]; then
            echo "Warning: Impossible de récupérer la dernière version de VKD3D-Proton"
        else
            local VKD3D_TAG VKD3D_URL
            if [ "${_VKD3D_SOURCE:-official}" = "bottles" ]; then
                VKD3D_TAG="vkd3d-proton-${VKD3D_VERSION}"
                VKD3D_URL="https://github.com/bottlesdevs/components/releases/download/${VKD3D_TAG}/${VKD3D_TAG}.tar.gz"
            else
                VKD3D_TAG="v${VKD3D_VERSION}"
                VKD3D_URL="https://github.com/HansKristian-Work/vkd3d-proton/releases/download/${VKD3D_TAG}/vkd3d-proton-${VKD3D_VERSION}.tar.gz"
            fi
            local VKD3D_TEMP="$VKD3D_CACHE_DIR/vkd3d.tar.gz"
            local VKD3D_DIR="$VKD3D_CACHE_DIR/vkd3d-proton-${VKD3D_VERSION}"
            
            wget -q --show-progress "$VKD3D_URL" -O "$VKD3D_TEMP" 2>&1 || echo "Warning: Échec du téléchargement de VKD3D-Proton"
            if [ -f "$VKD3D_TEMP" ]; then
                ensure_dir -s "$VKD3D_DIR"
                local temp_extract="$VKD3D_CACHE_DIR/.temp_vkd3d"
                rm -rf "$temp_extract"
                ensure_dir -s "$temp_extract"
                tar -xf "$VKD3D_TEMP" -C "$temp_extract" 2>/dev/null || true
                local extracted_dir
                extracted_dir=$(find "$temp_extract" -mindepth 1 -maxdepth 1 -type d | head -1)
                if [ -n "$extracted_dir" ]; then
                    cp -r "$extracted_dir"/* "$VKD3D_DIR/" 2>/dev/null || true
                fi
                rm -rf "$temp_extract" "$VKD3D_TEMP"
            fi
        fi
    fi
}

prepare_local_cache() {
    ensure_dirs "$CACHE_DIR" "$COMPONENTS_DIR" "$SHADER_CACHE_DIR"
    
    local missing_components=false
    
    local MONO_VER="11.2.0"
    local GECKO_VER="2.47.4"
    if [ ! -f "$COMPONENTS_DIR/wine-cache/wine-mono-${MONO_VER}-x86.msi" ] || \
       [ ! -f "$COMPONENTS_DIR/wine-cache/wine-gecko-${GECKO_VER}-x86_64.msi" ] || \
       [ ! -f "$COMPONENTS_DIR/wine-cache/wine-gecko-${GECKO_VER}-x86.msi" ]; then
        missing_components=true
    fi
    
    if [ ! -d "$DXVK_CACHE_DIR" ] || [ -z "$(find "$DXVK_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "dxvk-*" 2>/dev/null | grep -v "gplasync\|nvapi")" ]; then
        missing_components=true
    fi
    
    if [ ! -d "$VKD3D_CACHE_DIR" ] || [ -z "$(find "$VKD3D_CACHE_DIR" -mindepth 1 -maxdepth 1 -type d -name "vkd3d-proton-*" 2>/dev/null)" ]; then
        missing_components=true
    fi
    
    if [ "$missing_components" = true ]; then
        if [ "$OFFLINE_MODE" = "true" ]; then
            error_exit "Mode offline: composants manquants dans le cache. Lancez 'gwine --download-components' avec une connexion internet pour préparer le cache."
        fi
        
        echo ""
        echo "Certains composants sont manquants dans ~/.cache/gwine/components/"
        echo "Ils seront téléchargés automatiquement..."
        echo ""
        download_missing_components
    fi
}

auto_update_components() {
    local need_download=false
    local has_network=false
    
    local has_runner=false
    local has_dxvk=false
    local has_vkd3d=false
    
    # Obtenir le runner actuel (wine ou proton)
    local current_runner
    current_runner=$(get_current_runner)
    
    [ -d "$WINE_DIR" ] && [ -f "$WINE_DIR/bin/wine" ] && has_runner=true
    [ -n "$(find "$DXVK_CACHE_DIR" -maxdepth 1 -type d -name "dxvk-*" 2>/dev/null | grep -v "gplasync\|nvapi")" ] && has_dxvk=true
    [ -n "$(find "$VKD3D_CACHE_DIR" -maxdepth 1 -type d -name "vkd3d-proton-*" 2>/dev/null)" ] && has_vkd3d=true
    
    if curl -s --max-time 5 https://github.com > /dev/null 2>&1; then
        has_network=true
    fi
    
    if [ "$has_runner" = false ] || [ "$has_dxvk" = false ] || [ "$has_vkd3d" = false ]; then
        need_download=true
        
        if [ "$has_network" = false ]; then
            error_exit "Composants manquants et pas de connexion réseau disponible"
        fi
    fi
    
    if [ "$need_download" = false ] && [ "$has_network" = true ]; then
        local latest_runner
        local current_runner_version
        
        if [ "$current_runner" = "proton" ]; then
            latest_runner=$(get_latest_gwine_proton_version)
            current_runner_version=$(cat "$PROTON_RUNNER_VERSION_FILE" 2>/dev/null || echo "")
        else
            latest_runner=$(get_latest_gwine_version)
            current_runner_version=$(cat "$WINE_RUNNER_VERSION_FILE" 2>/dev/null || echo "")
        fi
        
        local latest_dxvk
        latest_dxvk=$(get_latest_dxvk_version)
        local latest_vkd3d
        latest_vkd3d=$(get_latest_vkd3d_version)
        
        local current_dxvk=""
        local dxvk_folder
        dxvk_folder=$(find "$DXVK_CACHE_DIR" -maxdepth 1 -type d -name "dxvk-*" 2>/dev/null | grep -v "gplasync\|nvapi" | sort -V | tail -1)
        [ -n "$dxvk_folder" ] && current_dxvk=$(basename "$dxvk_folder" | sed 's/^dxvk-//')
        
        local current_vkd3d=""
        local vkd3d_folder
        vkd3d_folder=$(find "$VKD3D_CACHE_DIR" -maxdepth 1 -type d -name "vkd3d-proton-*" 2>/dev/null | sort -V | tail -1)
        [ -n "$vkd3d_folder" ] && current_vkd3d=$(basename "$vkd3d_folder" | sed 's/^vkd3d-proton-//')
        
        if [ -n "$latest_runner" ] && [ -n "$current_runner_version" ]; then
            if compare_versions "$latest_runner" "$current_runner_version"; then
                need_download=true
            fi
        elif [ -z "$current_runner_version" ]; then
            need_download=true
        fi
        
        if [ -n "$latest_dxvk" ]; then
            if [ -z "$current_dxvk" ] || compare_versions "$latest_dxvk" "$current_dxvk"; then
                need_download=true
            fi
        fi
        
        if [ -n "$latest_vkd3d" ]; then
            if [ -z "$current_vkd3d" ] || compare_versions "$latest_vkd3d" "$current_vkd3d"; then
                need_download=true
            fi
        fi
        
        if is_nvidia_gpu; then
            local latest_nvapi
            latest_nvapi=$(get_latest_dxvk_nvapi_version)
            local current_nvapi=""
            local nvapi_folder
            nvapi_folder=$(find "$DXVK_NVAPI_CACHE_DIR" -maxdepth 1 -type d -name "dxvk-nvapi-*" 2>/dev/null | sort -V | tail -1)
            [ -n "$nvapi_folder" ] && current_nvapi=$(basename "$nvapi_folder" | sed 's/^dxvk-nvapi-v//; s/^dxvk-nvapi-//')
            
            if [ -n "$latest_nvapi" ]; then
                if [ -z "$current_nvapi" ] || compare_versions "$latest_nvapi" "$current_nvapi"; then
                    need_download=true
                fi
            fi
        fi
    fi
    
    if [ "$need_download" = true ]; then
        echo "Téléchargement des composants..."
        
        if [ "$has_runner" = false ]; then
            if [ "$current_runner" = "proton" ]; then
                echo "  - Installation de gwine-proton..."
                download_gwine_proton "force" "true" || error_exit "Échec du téléchargement de gwine-proton"
            else
                echo "  - Installation de gwine..."
                download_gwine "force" "true" || error_exit "Échec du téléchargement de gwine"
            fi
        elif [ "$has_network" = true ]; then
            if [ "$current_runner" = "proton" ]; then
                download_gwine_proton "false" "true"
            else
                download_gwine "false" "true"
            fi
        fi
        
        if [ "$has_dxvk" = false ] || [ "$has_vkd3d" = false ]; then
            echo "  - Installation de DXVK/VKD3D..."
            download_updated_dxvk_vkd3d --no-confirm || error_exit "Échec du téléchargement de DXVK/VKD3D"
        elif [ "$has_network" = true ]; then
            download_updated_dxvk_vkd3d --no-confirm
        fi
    else
        echo "Composants à jour, utilisation des versions locales"
    fi
}

prepare_full_offline_cache() {
    echo "Préparation du cache complet pour le mode offline..."
    echo ""
    
    local failed=false
    
    echo "1. Préparation des runners..."
    echo ""
    
    # Télécharger/mettre à jour gwine (runner standard)
    echo "   1a. Préparation de gwine..."
    if ! download_gwine "false" "true"; then
        echo "      ⚠️ Échec du téléchargement/mise à jour de gwine"
        failed=true
    fi
    
    # Télécharger/mettre à jour gwine-proton (runner proton)
    echo ""
    echo "   1b. Préparation de gwine-proton..."
    if ! download_gwine_proton "false" "true"; then
        echo "      ⚠️ Échec du téléchargement/mise à jour de gwine-proton"
        failed=true
    fi
    echo ""
    
    echo "2. Préparation de Wine Mono et Gecko..."
    local WINE_CACHE="$COMPONENTS_DIR/wine-cache"
    ensure_dir "$WINE_CACHE"
    
    local MONO_VER="11.2.0"
    local GECKO_VER="2.47.4"
    local MONO_URL="https://dl.winehq.org/wine/wine-mono/${MONO_VER}/wine-mono-${MONO_VER}-x86.msi"
    local GECKO_URL="https://dl.winehq.org/wine/wine-gecko/${GECKO_VER}/wine-gecko-${GECKO_VER}-x86_64.msi"
    local GECKO32_URL="https://dl.winehq.org/wine/wine-gecko/${GECKO_VER}/wine-gecko-${GECKO_VER}-x86.msi"
    
    local mono_file="$WINE_CACHE/wine-mono-${MONO_VER}-x86.msi"
    local gecko64_file="$WINE_CACHE/wine-gecko-${GECKO_VER}-x86_64.msi"
    local gecko32_file="$WINE_CACHE/wine-gecko-${GECKO_VER}-x86.msi"
    
    # Supprimer les anciennes versions pour éviter l'accumulation
    shopt -s nullglob
    for old in "$WINE_CACHE"/wine-mono-*.msi; do
        [ "$(basename "$old")" != "wine-mono-${MONO_VER}-x86.msi" ] && rm -f "$old"
    done
    for old in "$WINE_CACHE"/wine-gecko-*.msi; do
        [ "$(basename "$old")" != "wine-gecko-${GECKO_VER}-x86_64.msi" ] && \
        [ "$(basename "$old")" != "wine-gecko-${GECKO_VER}-x86.msi" ] && \
        rm -f "$old"
    done
    shopt -u nullglob
    
    if [ ! -f "$mono_file" ]; then
        echo "   Téléchargement de Wine Mono..."
        if ! wget -q --show-progress "$MONO_URL" -O "$mono_file" 2>&1; then
            echo "   ⚠️ Échec du téléchargement de Wine Mono"
            failed=true
        else
            echo "   ✓ Wine Mono téléchargé"
        fi
    else
        echo "   ✓ Wine Mono déjà en cache"
    fi
    
    if [ ! -f "$gecko64_file" ]; then
        echo "   Téléchargement de Wine Gecko (x64)..."
        if ! wget -q --show-progress "$GECKO_URL" -O "$gecko64_file" 2>&1; then
            echo "   ⚠️ Échec du téléchargement de Wine Gecko (x64)"
            failed=true
        else
            echo "   ✓ Wine Gecko (x64) téléchargé"
        fi
    else
        echo "   ✓ Wine Gecko (x64) déjà en cache"
    fi
    
    if [ ! -f "$gecko32_file" ]; then
        echo "   Téléchargement de Wine Gecko (x86)..."
        if ! wget -q --show-progress "$GECKO32_URL" -O "$gecko32_file" 2>&1; then
            echo "   ⚠️ Échec du téléchargement de Wine Gecko (x86)"
            failed=true
        else
            echo "   ✓ Wine Gecko (x86) téléchargé"
        fi
    else
        echo "   ✓ Wine Gecko (x86) déjà en cache"
    fi
    echo ""
    
    echo "3. Préparation de DXVK et VKD3D-Proton..."
    echo "   3a. DXVK standard..."
    if ! download_updated_dxvk_vkd3d --no-confirm; then
        echo "   ⚠️ Échec du téléchargement de DXVK/VKD3D-Proton"
        failed=true
    fi
    
    echo "   3b. DXVK-GPLAsync..."
    if ! download_dxvk_async "false" "true"; then
        echo "   ⚠️ Échec du téléchargement de DXVK-GPLAsync"
        failed=true
    fi
    echo ""
    
    echo "4. Préparation de DXVK-NVAPI (NVIDIA uniquement)..."
    
    if is_nvidia_gpu; then
        echo "   Téléchargement/mise à jour de DXVK-NVAPI..."
        if ! download_dxvk_nvapi --no-confirm 2>/dev/null; then
            echo "   ⚠️ Échec du téléchargement de DXVK-NVAPI"
            failed=true
        fi
    else
        echo "   GPU non-NVIDIA, DXVK-NVAPI ignoré"
    fi
    echo ""
    
    echo "6. Préparation des composants Windows..."
    if ! prepare_wincomponents_cache; then
        echo "   ⚠️ Échec du téléchargement des composants Windows"
        failed=true
    fi
    echo ""
    
    echo "=========================================="
    if [ "$failed" = true ]; then
        echo "⚠️  Certains composants n'ont pas pu être téléchargés"
        echo "Le mode offline pourrait ne pas fonctionner correctement"
        return 1
    else
        echo "✓ Tous les composants sont prêts pour le mode offline"
        echo "Vous pouvez maintenant utiliser 'gwine --init --offline'"
        return 0
    fi
}
