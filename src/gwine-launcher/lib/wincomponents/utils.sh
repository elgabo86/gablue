#!/bin/bash

################################################################################
# utils.sh - Fonctions utilitaires pour les composants Windows
################################################################################

calculate_sha256() {
    local file="$1"
    if command -v sha256sum &>/dev/null; then
        sha256sum "$file" | cut -d' ' -f1
    elif command -v shasum &>/dev/null; then
        shasum -a 256 "$file" | cut -d' ' -f1
    else
        echo ""
    fi
}

verify_component_file() {
    local file="$1"
    local expected_sha256="$2"
    
    if [ ! -f "$file" ]; then
        return 1
    fi
    
    if [ -z "$expected_sha256" ]; then
        return 0
    fi
    
    local actual_sha256
    actual_sha256=$(calculate_sha256 "$file")
    
    if [ "$actual_sha256" = "$expected_sha256" ]; then
        return 0
    else
        return 1
    fi
}

download_component() {
    local name="$1"
    local url="${COMPONENT_URLS[$name]}"
    local sha256="${COMPONENT_SHA256[$name]}"
    
    if [ -z "$url" ]; then
        echo "Erreur: URL non définie pour le composant $name"
        return 1
    fi
    
    local filename=$(basename "$url")
    local cache_file="$WINCOMPONENTS_CACHE/$name/$filename"
    
    ensure_dir "$WINCOMPONENTS_CACHE/$name"
    
    if verify_component_file "$cache_file" "$sha256"; then
        echo "Composant $name déjà en cache"
        return 0
    fi
    
    echo "  - Téléchargement de $name..."
    if ! wget -q "$url" -O "$cache_file" 2>/dev/null; then
        echo "    ✗ Échec du téléchargement"
        rm -f "$cache_file"
        return 1
    fi
    
    if [ -n "$sha256" ] && ! verify_component_file "$cache_file" "$sha256"; then
        echo "Erreur: Checksum incorrect pour $name"
        rm -f "$cache_file"
        return 1
    fi
    
    echo "✓ $name téléchargé avec succès"
    return 0
}

check_wincomponents_cache() {
    local missing=()
    
    for component in "${WINCOMPONENTS_REQUIRED[@]}"; do
        local url="${COMPONENT_URLS[$component]}"
        if [ -z "$url" ]; then
            continue
        fi
        
        local filename=$(basename "$url")
        local cache_file="$WINCOMPONENTS_CACHE/$component/$filename"
        local sha256="${COMPONENT_SHA256[$component]}"
        
        if ! verify_component_file "$cache_file" "$sha256"; then
            missing+=("$component")
        fi
    done
    
    if [ ${#missing[@]} -gt 0 ]; then
        echo "Composants manquants dans le cache:"
        for comp in "${missing[@]}"; do
            echo "  - $comp"
        done
        return 1
    fi
    
    return 0
}

download_all_wincomponents() {
    echo "Téléchargement des composants Windows..."
    
    local failed=()
    
    for component in "${WINCOMPONENTS_REQUIRED[@]}"; do
        if ! download_component "$component"; then
            failed+=("$component")
        fi
    done
    
    if [ ${#failed[@]} -gt 0 ]; then
        echo "Erreur: Échec du téléchargement de certains composants:"
        for comp in "${failed[@]}"; do
            echo "  - $comp"
        done
        return 1
    fi
    
    echo "✓ Tous les composants sont dans le cache"
    return 0
}

prepare_wincomponents_cache() {
    ensure_dir "$WINCOMPONENTS_CACHE"
    
    if [ "${OFFLINE_MODE:-false}" = "true" ]; then
        if ! check_wincomponents_cache; then
            return 1
        fi
        return 0
    fi
    
    download_all_wincomponents
}
