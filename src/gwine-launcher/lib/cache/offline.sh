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
            local VKD3D_TAG VKD3D_URL VKD3D_FALLBACK_URL
            if [ "${_VKD3D_SOURCE:-official}" = "bottles" ]; then
                VKD3D_TAG="vkd3d-proton-${VKD3D_VERSION}"
                VKD3D_URL="https://github.com/bottlesdevs/components/releases/download/${VKD3D_TAG}/${VKD3D_TAG}.tar.gz"
                VKD3D_FALLBACK_URL=""
            else
                VKD3D_TAG="v${VKD3D_VERSION}"
                # v3.0.1+ utilise .tar.zst, versions antérieures .tar.gz
                VKD3D_URL="https://github.com/HansKristian-Work/vkd3d-proton/releases/download/${VKD3D_TAG}/vkd3d-proton-${VKD3D_VERSION}.tar.zst"
                VKD3D_FALLBACK_URL="https://github.com/HansKristian-Work/vkd3d-proton/releases/download/${VKD3D_TAG}/vkd3d-proton-${VKD3D_VERSION}.tar.gz"
            fi
            local VKD3D_TEMP="$VKD3D_CACHE_DIR/vkd3d.tar"
            local VKD3D_DIR="$VKD3D_CACHE_DIR/vkd3d-proton-${VKD3D_VERSION}"
            
            if ! wget -q --show-progress "$VKD3D_URL" -O "$VKD3D_TEMP" 2>&1; then
                if [ -n "$VKD3D_FALLBACK_URL" ]; then
                    echo "Format .tar.zst indisponible, tentative .tar.gz..."
                    wget -q --show-progress "$VKD3D_FALLBACK_URL" -O "$VKD3D_TEMP" 2>&1 || echo "Warning: Échec du téléchargement de VKD3D-Proton"
                else
                    echo "Warning: Échec du téléchargement de VKD3D-Proton"
                fi
            fi
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
    
    # Obtenir le mode DXVK actuel (standard ou async)
    local dxvk_mode
    dxvk_mode=$(get_current_dxvk_mode)
    
    [ -d "$WINE_DIR" ] && [ -f "$WINE_DIR/bin/wine" ] && has_runner=true
    if [ "$dxvk_mode" = "dxvk-async" ]; then
        [ -n "$(find "$DXVK_ASYNC_CACHE_DIR" -maxdepth 1 -type d -name "dxvk*gplasync*" 2>/dev/null)" ] && has_dxvk=true
    else
        [ -n "$(find "$DXVK_CACHE_DIR" -maxdepth 1 -type d -name "dxvk-*" 2>/dev/null | grep -v "gplasync\|nvapi")" ] && has_dxvk=true
    fi
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
        
        latest_runner=$(get_latest_gwine_version)
        current_runner_version=$(cat "$GWINE_DIR/.version" 2>/dev/null || echo "")
        
        if [ "$dxvk_mode" = "dxvk-async" ]; then
            local latest_dxvk
            latest_dxvk=$(get_latest_dxvk_async_version)
            local current_dxvk=""
            local dxvk_folder
            dxvk_folder=$(find "$DXVK_ASYNC_CACHE_DIR" -maxdepth 1 -type d -name "dxvk*gplasync*" 2>/dev/null | sort -V | tail -1)
            [ -n "$dxvk_folder" ] && current_dxvk=$(basename "$dxvk_folder" | sed 's/^dxvk-gplasync-//')
        else
            local latest_dxvk
            latest_dxvk=$(get_latest_dxvk_version)
            local current_dxvk=""
            local dxvk_folder
            dxvk_folder=$(find "$DXVK_CACHE_DIR" -maxdepth 1 -type d -name "dxvk-*" 2>/dev/null | grep -v "gplasync\|nvapi" | sort -V | tail -1)
            [ -n "$dxvk_folder" ] && current_dxvk=$(basename "$dxvk_folder" | sed 's/^dxvk-//')
        fi
        
        local latest_vkd3d
        latest_vkd3d=$(get_latest_vkd3d_version)
        
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
        fi
        
        if [ -n "$latest_dxvk" ]; then
            if [ "$dxvk_mode" = "dxvk-async" ]; then
                if [ -z "$current_dxvk" ] || [ "$current_dxvk" != "$latest_dxvk" ]; then
                    need_download=true
                fi
            else
                if [ -z "$current_dxvk" ] || compare_versions "$latest_dxvk" "$current_dxvk"; then
                    need_download=true
                fi
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
                echo "  - Installation de gwine..."
                download_gwine "force" "true" || error_exit "Échec du téléchargement de gwine"
        elif [ "$has_network" = true ]; then
                download_gwine "false" "true"
        fi
        
        if [ "$dxvk_mode" = "dxvk-async" ]; then
            if [ "$has_dxvk" = false ] || [ "$has_vkd3d" = false ]; then
                echo "  - Installation de DXVK-GPLAsync/VKD3D..."
                download_dxvk_async "force" "true" || error_exit "Échec du téléchargement de DXVK-GPLAsync"
                download_vkd3d "force" || error_exit "Échec du téléchargement de VKD3D"
            elif [ "$has_network" = true ]; then
                download_dxvk_async "false" "true"
                download_vkd3d "false"
            fi
        else
            if [ "$has_dxvk" = false ] || [ "$has_vkd3d" = false ]; then
                echo "  - Installation de DXVK/VKD3D..."
                download_updated_dxvk_vkd3d --no-confirm || error_exit "Échec du téléchargement de DXVK/VKD3D"
            elif [ "$has_network" = true ]; then
                download_updated_dxvk_vkd3d --no-confirm
            fi
        fi
    else
        echo "Composants à jour, utilisation des versions locales"
    fi
}

prepare_full_offline_cache() {
    echo "Préparation du cache complet pour le mode offline..."
    echo ""
    
    local failed=false
    
    echo "1. Préparation du runner gwine..."
    echo ""
    
    # Seul le runner gwine est pré-caché.
    if ! download_gwine "false" "true"; then
        echo "      ⚠️ Échec du téléchargement/mise à jour de gwine"
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
    
    echo "4. Préparation de DXVK-NVAPI..."
    
    # Toujours télécharger DXVK-NVAPI pour un cache portable (pack offline / ISO)
    # déployable sur une machine NVIDIA, indépendamment du GPU de la machine
    # qui construit le cache. L'installation dans le préfixe reste conditionnée
    # au GPU (voir install_dxvk_nvapi).
    echo "   Téléchargement/mise à jour de DXVK-NVAPI..."
    if ! download_dxvk_nvapi --no-confirm 2>/dev/null; then
        echo "   ⚠️ Échec du téléchargement de DXVK-NVAPI"
        failed=true
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

# =============================================================================
# Détection automatique du mode offline (first-run sans connexion)
# =============================================================================

# Vérifie si le cache local contient le minimum requis pour un --init --offline
# complet : runner archive + mono/gecko + wincomponents.
# Retourne 0 si le cache est prêt pour l'offline, 1 sinon.
gablue_offline_cache_ready() {
    # Runner gwine (archive .tar.xz dans le cache)
    if [ ! -d "$COMPONENTS_DIR/gwine" ] || \
       [ -z "$(ls -A "$COMPONENTS_DIR/gwine"/gwine-*.tar.xz 2>/dev/null)" ]; then
        return 1
    fi

    # Wine Mono et Gecko (archives .msi)
    local WINE_CACHE="$COMPONENTS_DIR/wine-cache"
    local mono_count gecko64_count gecko32_count
    mono_count=$(find "$WINE_CACHE" -maxdepth 1 -name "wine-mono-*-x86.msi" 2>/dev/null | wc -l)
    gecko64_count=$(find "$WINE_CACHE" -maxdepth 1 -name "wine-gecko-*-x86_64.msi" 2>/dev/null | wc -l)
    gecko32_count=$(find "$WINE_CACHE" -maxdepth 1 -name "wine-gecko-*-x86.msi" 2>/dev/null | wc -l)
    if [ "$mono_count" -eq 0 ] || [ "$gecko64_count" -eq 0 ] || [ "$gecko32_count" -eq 0 ]; then
        return 1
    fi

    # Composants Windows (wincomponents)
    if [ ! -d "$CACHE_DIR/wincomponents" ]; then
        return 1
    fi
    if command -v check_wincomponents_cache &>/dev/null; then
        if ! check_wincomponents_cache 2>/dev/null; then
            return 1
        fi
    else
        # Fonction absente = pas de vérification fine, on accepte
        :
    fi

    return 0
}
