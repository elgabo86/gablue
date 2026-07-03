#!/bin/bash

################################################################################
# wmp9.sh - Installation de Windows Media Player 9 (WMP9)
# Inspiré de winetricks wmp9
# Dépendances : wsh57 (Windows Script Host 5.7)
################################################################################

_wmp9_save_winver() {
    _WMP9_SAVED_WINVER=""
    local current
    current=$(WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg query 'HKEY_CURRENT_USER\Software\Wine' /v Version 2>/dev/null | grep -oE 'win[a-z0-9]+' | head -1) || true
    _WMP9_SAVED_WINVER="$current"
}

_wmp9_set_winver() {
    local ver="$1"
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg delete 'HKEY_CURRENT_USER\Software\Wine' /v Version /f >/dev/null 2>&1 || true
    if [ "$ver" != "default" ]; then
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine' /v Version /d "$ver" /f >/dev/null 2>&1 || true
    fi
}

_wmp9_restore_winver() {
    if [ -n "$_WMP9_SAVED_WINVER" ]; then
        _wmp9_set_winver "$_WMP9_SAVED_WINVER"
    else
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg delete 'HKEY_CURRENT_USER\Software\Wine' /v Version /f >/dev/null 2>&1 || true
    fi
}

install_wsh57() {
    echo "Installation de Windows Script Host 5.7 (wsh57)..."
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    ensure_dir -s "$win32_sys"
    
    local cache_file="$WINCOMPONENTS_CACHE/wsh57/scripten.exe"
    
    if [ ! -f "$cache_file" ]; then
        echo "  ✗ Fichier wsh57 non trouvé dans le cache"
        return 1
    fi
    
    if ! command -v cabextract &>/dev/null; then
        echo "  ✗ cabextract non trouvé, requis pour wsh57"
        return 1
    fi
    
    cabextract -d "$win32_sys" "$cache_file" >/dev/null 2>&1 || true
    
    local wsh_dlls="jscript scrrun vbscript cscript.exe wscript.exe"
    for dll in $wsh_dlls; do
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*$dll" /d native,builtin /f >/dev/null 2>&1 || true
    done
    
    if [ -x "$WINE32_BIN" ]; then
        local regsvr32_dlls="dispex.dll jscript.dll scrobj.dll scrrun.dll vbscript.dll wshcon.dll wshext.dll"
        for dll in $regsvr32_dlls; do
            if [ -f "$win32_sys/$dll" ]; then
                WINEPREFIX="$WINEPREFIX" "$WINE32_BIN" "C:\\windows\\syswow64\\regsvr32.exe" /S "$dll" >/dev/null 2>&1 || true
            fi
        done
    else
        local regsvr32_dlls="dispex.dll jscript.dll scrobj.dll scrrun.dll vbscript.dll wshcon.dll wshext.dll"
        for dll in $regsvr32_dlls; do
            if [ -f "$win32_sys/$dll" ]; then
                WINEPREFIX="$WINEPREFIX" "$WINE_BIN" "C:\\windows\\syswow64\\regsvr32.exe" /S "$dll" >/dev/null 2>&1 || true
            fi
        done
    fi
    
    echo "  ✓ wsh57 installé"
    return 0
}

install_wmp9() {
    echo "Installation de Windows Media Player 9 (wmp9)..."
    
    if ! install_wsh57; then
        echo "  ✗ Échec de l'installation de la dépendance wsh57"
        return 1
    fi
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    local win64_sys="$WINEPREFIX/drive_c/windows/system32"
    ensure_dir -s "$win32_sys"
    [ "$WINEARCH" = "win64" ] && ensure_dir -s "$win64_sys"
    
    local cache_file="$WINCOMPONENTS_CACHE/wmp9/MPSetup.exe"
    
    if [ ! -f "$cache_file" ]; then
        echo "  ✗ Fichier WMP9 non trouvé dans le cache"
        return 1
    fi
    
    if ! command -v cabextract &>/dev/null; then
        echo "  ✗ cabextract non trouvé, requis pour wmp9"
        return 1
    fi
    
    _wmp9_save_winver
    _wmp9_set_winver "winxp"
    
    rm -f "$win32_sys/wmvcore.dll" "$win32_sys/wmp.dll" 2>/dev/null || true
    rm -f "$win64_sys/wmvcore.dll" "$win64_sys/wmp.dll" 2>/dev/null || true
    rm -f "$WINEPREFIX/drive_c/Program Files (x86)/Windows Media Player/wmplayer.exe" 2>/dev/null || true
    rm -f "$WINEPREFIX/drive_c/Program Files/Windows Media Player/wmplayer.exe" 2>/dev/null || true
    
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*l3codeca.acm" /d native /f >/dev/null 2>&1 || true
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*wmp" /d native /f >/dev/null 2>&1 || true
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*wmplayer.exe" /d native /f >/dev/null 2>&1 || true
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*wmvcore" /d native /f >/dev/null 2>&1 || true
    
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\AppDefaults\MPSetup.exe\DllOverrides' /v "pidgen" /d native /f >/dev/null 2>&1 || true
    
    local tmp_dir=$(mktemp -d)
    cabextract -d "$tmp_dir" "$cache_file" >/dev/null 2>&1 || true
    
    if [ ! -f "$tmp_dir/setup_wm.exe" ]; then
        echo "  ✗ setup_wm.exe non trouvé après extraction"
        rm -rf "$tmp_dir"
        _wmp9_restore_winver
        return 1
    fi
    
    local saved_cwd="$PWD"
    cd "$tmp_dir" || { rm -rf "$tmp_dir"; _wmp9_restore_winver; return 1; }
    
    if [ "$WINEARCH" = "win64" ]; then
        sed -i 's/IsWow64Process/IsNow64Process/' setup_wm.exe 2>/dev/null || true
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" setup_wm.exe /Quiet 2>/dev/null || true
    else
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" setup_wm.exe /Quiet 2>/dev/null || true
        _wmp9_install_codecs
    fi
    
    cd "$saved_cwd"
    
    rm -rf "$tmp_dir"
    
    _wmp9_restore_winver
    
    echo "  ✓ WMP9 installé"
    return 0
}

_wmp9_install_codecs() {
    echo "  - Installation des codecs WMP9 (wm9codecs)..."
    
    local cache_codecs="$WINCOMPONENTS_CACHE/wm9codecs/WM9Codecs9x.exe"
    
    if [ ! -f "$cache_codecs" ]; then
        echo "    ✗ Fichier wm9codecs non trouvé dans le cache"
        return 1
    fi
    
    local saved
    saved="$_WMP9_SAVED_WINVER"
    _wmp9_set_winver "win2k"
    
    local saved_cwd="$PWD"
    cd "$(dirname "$cache_codecs")" || return 1
    
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" WM9Codecs9x.exe /q 2>/dev/null || true
    
    cd "$saved_cwd"
    
    if [ -n "$saved" ]; then
        _wmp9_set_winver "$saved"
    fi
    
    echo "    ✓ Codecs WMP9 installés"
    return 0
}
