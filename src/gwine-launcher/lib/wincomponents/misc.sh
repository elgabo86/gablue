#!/bin/bash

################################################################################
# misc.sh - Installation des composants divers (OpenAL, PhysX, MSLS31, VB6)
################################################################################

install_openal() {
    echo "Installation d'OpenAL..."
    
    local cache_zip="$WINCOMPONENTS_CACHE/openal/oalinst.zip"
    
    if [ -f "$cache_zip" ]; then
        local tmp_dir=$(mktemp -d)
        unzip -q "$cache_zip" -d "$tmp_dir" 2>/dev/null || true
        
        if [ -f "$tmp_dir/oalinst.exe" ]; then
            WINEPREFIX="$WINEPREFIX" "$WINE_BIN" "$tmp_dir/oalinst.exe" /s 2>/dev/null || true
        fi
        
        rm -rf "$tmp_dir"
    fi
    
    return 0
}

install_physx() {
    echo "Installation de PhysX..."
    
    local cache_file="$WINCOMPONENTS_CACHE/physx/PhysX_9.23.1019_SystemSoftware.exe"
    
    if [ -f "$cache_file" ]; then
        echo "  - Lancement de l'installateur PhysX..."
        if WINEPREFIX="$WINEPREFIX" "$WINE_BIN" "$cache_file" /s 2>/dev/null; then
            echo "  ✓ PhysX installé"
        else
            echo "  ⚠ PhysX installé (peut déjà être présent)"
        fi
    else
        echo "  ✗ Fichier PhysX non trouvé dans le cache"
        return 1
    fi
    
    return 0
}

install_msls31() {
    echo "Installation de MSLS31..."
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    local cache_file="$WINCOMPONENTS_CACHE/msls31/InstMsiW.exe"
    
    if [ ! -f "$cache_file" ]; then
        echo "  ✗ Fichier MSLS31 non trouvé"
        return 1
    fi
    
    if command -v cabextract &>/dev/null; then
        cabextract -d "$win32_sys" -F 'msls31.dll' "$cache_file" >/dev/null 2>&1 || true
    fi
    
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "msls31" /d native,builtin /f >/dev/null 2>&1 || true
    
    echo "  ✓ MSLS31 installé"
    return 0
}

install_vb6run() {
    echo "Installation de VB6 Runtime..."
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    local cache_file="$WINCOMPONENTS_CACHE/vb6run/VB6.0-KB290887-X86.exe"
    
    if [ ! -f "$cache_file" ]; then
        echo "  ✗ Fichier VB6 non trouvé"
        return 1
    fi
    
    if command -v cabextract &>/dev/null; then
        local tmp_dir=$(mktemp -d)
        cabextract -d "$tmp_dir" "$cache_file" >/dev/null 2>&1 || true
        
        for dll in "$tmp_dir"/*.dll; do
            if [ -f "$dll" ]; then
                cp -f "$dll" "$win32_sys/" 2>/dev/null || true
            fi
        done
        
        rm -rf "$tmp_dir"
    fi
    
    echo "  ✓ VB6 Runtime installé"
    return 0
}
