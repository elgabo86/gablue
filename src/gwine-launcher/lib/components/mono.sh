#!/bin/bash

################################################################################
# component-mono.sh - Installation de Wine Mono et Wine Gecko
################################################################################

# Installe Wine Mono et Wine Gecko depuis le cache local
install_wine_mono_gecko() {
    local WINE_CACHE="$COMPONENTS_DIR/wine-cache"
    local _MONO_GECKO_PID=""
    
    trap '_mono_gecko_interrupt' INT
    
    _mono_gecko_interrupt() {
        if [ -n "$_MONO_GECKO_PID" ] && kill -0 "$_MONO_GECKO_PID" 2>/dev/null; then
            kill "$_MONO_GECKO_PID" 2>/dev/null
            wait "$_MONO_GECKO_PID" 2>/dev/null
        fi
        trap - INT
        echo ""
        echo "Installation interrompue par l'utilisateur"
        if [ -n "$_PROGRESS_DBUS_REF" ]; then
            progress_close "$_PROGRESS_DBUS_REF" 2>/dev/null
        fi
        restore_wineprefix
        exit 130
    }
    
    prepare_local_cache
    
    if [ -d "$WINE_CACHE" ]; then
        echo "Installation de Wine Mono et Gecko..."
        
        local mono_installed=false
        for mono_msi in "$WINE_CACHE"/wine-mono-*.msi; do
            if [ -f "$mono_msi" ]; then
                local mono_path
                mono_path=$(WINEPREFIX="$WINEPREFIX" "$WINE_BIN" winepath -w "$(realpath "$mono_msi")" 2>/dev/null | tr -d '\r')
                if [ -z "$mono_path" ]; then
                    mono_path="Z:$(realpath "$mono_msi" | sed 's|/|\\|g')"
                fi
                echo "  - Installation de $(basename "$mono_msi")..."
                
                WINEPREFIX="$WINEPREFIX" DISPLAY= "$WINE_BIN" msiexec /i "$mono_path" /quiet >/dev/null 2>&1 &
                _MONO_GECKO_PID=$!
                
                if ! wait "$_MONO_GECKO_PID"; then
                    trap - INT
                    _MONO_GECKO_PID=""
                    echo "    ⚠️  Échec installation $(basename "$mono_msi")"
                    return 1
                fi
                _MONO_GECKO_PID=""
                mono_installed=true
            fi
        done
        
        local gecko_installed=false
        for gecko_msi in "$WINE_CACHE"/wine-gecko-*.msi; do
            if [ -f "$gecko_msi" ]; then
                local gecko_path
                gecko_path=$(WINEPREFIX="$WINEPREFIX" "$WINE_BIN" winepath -w "$(realpath "$gecko_msi")" 2>/dev/null | tr -d '\r')
                if [ -z "$gecko_path" ]; then
                    gecko_path="Z:$(realpath "$gecko_msi" | sed 's|/|\\|g')"
                fi
                echo "  - Installation de $(basename "$gecko_msi")..."
                
                WINEPREFIX="$WINEPREFIX" DISPLAY= "$WINE_BIN" msiexec /i "$gecko_path" /quiet >/dev/null 2>&1 &
                _MONO_GECKO_PID=$!
                
                if ! wait "$_MONO_GECKO_PID"; then
                    trap - INT
                    _MONO_GECKO_PID=""
                    echo "    ⚠️  Échec installation $(basename "$gecko_msi")"
                    return 1
                fi
                _MONO_GECKO_PID=""
                gecko_installed=true
            fi
        done
        
        trap - INT
        
        if [ "$mono_installed" = false ] || [ "$gecko_installed" = false ]; then
            echo "Erreur: Mono ou Gecko n'ont pas pu être installés depuis le cache"
            return 1
        fi
        
        echo "Mono et Gecko installés"
    else
        echo "Erreur: Cache Mono/Gecko non trouvé"
        return 1
    fi
    
    return 0
}
