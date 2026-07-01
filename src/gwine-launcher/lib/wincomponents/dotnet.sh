#!/bin/bash

################################################################################
# dotnet.sh - Installation de .NET Desktop Runtime
################################################################################

install_dotnet() {
    local version="$1"
    local arch="${2:-both}"
    
    echo "Installation de .NET Desktop Runtime $version..."
    
    if [ "$arch" = "x86" ] || [ "$arch" = "both" ]; then
        local component_name="dotnetdesktop${version}_x86"
        local cache_file="$WINCOMPONENTS_CACHE/$component_name/$(basename "${COMPONENT_URLS[$component_name]}")"
        
        if [ -f "$cache_file" ]; then
            echo "  - Installation .NET $version x86..."
            WINEPREFIX="$WINEPREFIX" "$WINE_BIN" "$cache_file" /quiet /norestart 2>/dev/null || true
        fi
    fi
    
    if [ "$WINEARCH" = "win64" ] && { [ "$arch" = "x64" ] || [ "$arch" = "both" ]; }; then
        local component_name="dotnetdesktop${version}_x64"
        local cache_file="$WINCOMPONENTS_CACHE/$component_name/$(basename "${COMPONENT_URLS[$component_name]}")"
        
        if [ -f "$cache_file" ]; then
            echo "  - Installation .NET $version x64..."
            WINEPREFIX="$WINEPREFIX" "$WINE_BIN" "$cache_file" /quiet /norestart 2>/dev/null || true
        fi
    fi
    
    return 0
}
