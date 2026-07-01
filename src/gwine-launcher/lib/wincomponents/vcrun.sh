#!/bin/bash

################################################################################
# vcrun.sh - Installation de Visual C++ Redistributables
################################################################################

install_vcrun() {
    local version="$1"
    local arch="${2:-both}"
    
    echo "Installation de Visual C++ $version..."
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    local win64_sys="$WINEPREFIX/drive_c/windows/system32"
    
    ensure_dir -s "$win32_sys"
    [ "$WINEARCH" = "win64" ] && ensure_dir -s "$win64_sys"
    
    if [ "$arch" = "x86" ] || [ "$arch" = "both" ]; then
        local component_name="vcrun${version}_x86"
        local cache_file="$WINCOMPONENTS_CACHE/$component_name/$(basename "${COMPONENT_URLS[$component_name]}")"
        
        if [ -f "$cache_file" ]; then
            echo "  - Installation VC++ $version x86..."
            WINEPREFIX="$WINEPREFIX" "$WINE_BIN" "$cache_file" /q /norestart 2>/dev/null || true
        fi
    fi
    
    if [ "$WINEARCH" = "win64" ] && { [ "$arch" = "x64" ] || [ "$arch" = "both" ]; }; then
        local component_name="vcrun${version}_x64"
        local cache_file="$WINCOMPONENTS_CACHE/$component_name/$(basename "${COMPONENT_URLS[$component_name]}")"
        
        if [ -f "$cache_file" ]; then
            echo "  - Installation VC++ $version x64..."
            WINEPREFIX="$WINEPREFIX" "$WINE_BIN" "$cache_file" /q /norestart 2>/dev/null || true
        fi
    fi
    
    local vcrun_dlls=""
    local vcrun_dlls_x64=""
    case "$version" in
        2010) vcrun_dlls="msvcp100 msvcr100 vcomp100 atl100" ;;
        2012) vcrun_dlls="msvcp110 msvcr110 vcomp110 atl110" ;;
        2013) vcrun_dlls="msvcp120 msvcr120 vcomp120 atl120" ;;
        2022) vcrun_dlls="concrt140 msvcp140 msvcp140_1 msvcp140_2 msvcp140_atomic_wait msvcp140_codecvt_ids vcamp140 vccorlib140 vcomp140 vcruntime140"
              vcrun_dlls_x64="vcruntime140_1" ;;
    esac
    
    for dll in $vcrun_dlls; do
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*$dll" /d native,builtin /f >/dev/null 2>&1 || true
    done
    
    if [ "$WINEARCH" = "win64" ] && [ -n "$vcrun_dlls_x64" ]; then
        for dll in $vcrun_dlls_x64; do
            WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*$dll" /d native,builtin /f >/dev/null 2>&1 || true
        done
    fi
    
    return 0
}

install_vcrun6() {
    echo "Installation de VCRUN6 (MFC42)..."
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    local cache_file="$WINCOMPONENTS_CACHE/vcrun6/VC6RedistSetup_deu.exe"
    
    if [ ! -f "$cache_file" ]; then
        echo "  ✗ Fichier VCRUN6 non trouvé"
        return 1
    fi
    
    local tmp_dir=$(mktemp -d)
    local vcredist="$WINCOMPONENTS_CACHE/vcrun6/vcredist.exe"
    
    if [ ! -f "$vcredist" ]; then
        if command -v cabextract &>/dev/null; then
            cabextract -d "$tmp_dir" "$cache_file" >/dev/null 2>&1 || true
            if [ -f "$tmp_dir/vcredist.exe" ]; then
                cp "$tmp_dir/vcredist.exe" "$vcredist" 2>/dev/null || true
            fi
        fi
    fi
    
    if [ -f "$vcredist" ] && command -v cabextract &>/dev/null; then
        cabextract -d "$tmp_dir" "$vcredist" >/dev/null 2>&1 || true
        
        for dll in mfc42.dll mfc42u.dll msvcirt.dll; do
            if [ -f "$tmp_dir/$dll" ]; then
                cp -f "$tmp_dir/$dll" "$win32_sys/" 2>/dev/null || true
            fi
        done
    fi
    
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*mfc42" /d native,builtin /f >/dev/null 2>&1 || true
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*mfc42u" /d native,builtin /f >/dev/null 2>&1 || true
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*msvcirt" /d native,builtin /f >/dev/null 2>&1 || true
    
    rm -rf "$tmp_dir"
    echo "  ✓ VCRUN6 (MFC42) installé"
    return 0
}
