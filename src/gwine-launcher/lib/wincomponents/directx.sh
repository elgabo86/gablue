#!/bin/bash

################################################################################
# directx.sh - Installation des composants DirectX
################################################################################

install_d3dx9() {
    echo "Installation des DLLs DirectX9..."
    
    local cache_file="$WINCOMPONENTS_CACHE/directx_Jun2010/directx_Jun2010_redist.exe"
    
    if [ ! -f "$cache_file" ]; then
        echo "Erreur: DirectX Jun2010 non trouvé dans le cache"
        return 1
    fi
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    local win64_sys="$WINEPREFIX/drive_c/windows/system32"
    
    local tmp_dir=$(mktemp -d)
    local extracted_count=0
    
    if command -v cabextract &>/dev/null; then
        cabextract -d "$tmp_dir" -L "$cache_file" >/dev/null 2>&1 || true
        
        for cab in "$tmp_dir"/*d3dx9*x86*.cab; do
            if [ -f "$cab" ]; then
                if cabextract -d "$win32_sys" -L -F 'd3dx9*.dll' "$cab" >/dev/null 2>&1; then
                    extracted_count=$((extracted_count + 1))
                fi
            fi
        done
        
        if [ "$WINEARCH" = "win64" ]; then
            for cab in "$tmp_dir"/*d3dx9*x64*.cab; do
                if [ -f "$cab" ]; then
                    if cabextract -d "$win64_sys" -L -F 'd3dx9*.dll' "$cab" >/dev/null 2>&1; then
                        extracted_count=$((extracted_count + 1))
                    fi
                fi
            done
        fi
        
        echo "  ✓ $extracted_count DLLs DirectX9 extraites"
    fi
    
    for i in $(seq 24 43); do
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*d3dx9_$i" /d native,builtin /f >/dev/null 2>&1 || true
    done
    
    rm -rf "$tmp_dir"
    return 0
}

install_d3dcompiler() {
    local version="$1"
    echo "Installation de D3DCompiler_$version..."
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    local win64_sys="$WINEPREFIX/drive_c/windows/system32"
    
    local cache_file="$WINCOMPONENTS_CACHE/directx_Jun2010/directx_Jun2010_redist.exe"
    
    if [ ! -f "$cache_file" ]; then
        echo "  ✗ Fichier DirectX non trouvé"
        return 1
    fi
    
    local tmp_dir=$(mktemp -d)
    local dll_name="d3dcompiler_$version"
    
    if command -v cabextract &>/dev/null; then
        cabextract -d "$tmp_dir" -L -F "*${dll_name}*x86*" "$cache_file" >/dev/null 2>&1 || true
        
        for cab in "$tmp_dir"/*.cab; do
            if [ -f "$cab" ]; then
                cabextract -d "$win32_sys" -L -F "${dll_name}.dll" "$cab" >/dev/null 2>&1
            fi
        done
        
        if [ "$WINEARCH" = "win64" ]; then
            cabextract -d "$tmp_dir" -L -F "*${dll_name}*x64*" "$cache_file" >/dev/null 2>&1 || true
            for cab in "$tmp_dir"/*x64.cab; do
                if [ -f "$cab" ]; then
                    cabextract -d "$win64_sys" -L -F "${dll_name}.dll" "$cab" >/dev/null 2>&1
                fi
            done
        fi
    fi
    
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*$dll_name" /d native,builtin /f >/dev/null 2>&1 || true
    
    rm -rf "$tmp_dir"
    echo "  ✓ D3DCompiler_$version installé"
    return 0
}

install_d3dcompiler_47() {
    echo "Installation de D3DCompiler_47..."
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    local win64_sys="$WINEPREFIX/drive_c/windows/system32"
    
    local cache_x86="$WINCOMPONENTS_CACHE/d3dcompiler_47_x86/d3dcompiler_47_32.dll"
    local cache_x64="$WINCOMPONENTS_CACHE/d3dcompiler_47_x64/d3dcompiler_47.dll"
    
    if [ -f "$cache_x86" ]; then
        cp -f "$cache_x86" "$win32_sys/d3dcompiler_47.dll" 2>/dev/null || true
    fi
    
    if [ "$WINEARCH" = "win64" ] && [ -f "$cache_x64" ]; then
        cp -f "$cache_x64" "$win64_sys/d3dcompiler_47.dll" 2>/dev/null || true
    fi
    
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*d3dcompiler_47" /d native,builtin /f >/dev/null 2>&1 || true
    
    echo "  ✓ D3DCompiler_47 installé"
    return 0
}

install_xact() {
    echo "Installation de XACT Engine..."
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    
    # XACT utilise le même fichier que DirectX_Jun2010
    local cache_file="$WINCOMPONENTS_CACHE/directx_Jun2010/directx_Jun2010_redist.exe"
    
    if [ ! -f "$cache_file" ]; then
        echo "  ✗ Fichier DirectX non trouvé"
        return 1
    fi
    
    local tmp_dir=$(mktemp -d)
    local xact_count=0
    
    if command -v cabextract &>/dev/null; then
        # Extraire les CABs contenant les DLLs XACT (comme winetricks)
        cabextract -d "$tmp_dir" -L -F '*_xact_*x86*' "$cache_file" >/dev/null 2>&1 || true
        cabextract -d "$tmp_dir" -L -F '*_x3daudio_*x86*' "$cache_file" >/dev/null 2>&1 || true
        cabextract -d "$tmp_dir" -L -F '*_xaudio_*x86*' "$cache_file" >/dev/null 2>&1 || true
        
        # Extraire les DLLs des CABs
        for cab in "$tmp_dir"/*.cab; do
            if [ -f "$cab" ]; then
                cabextract -d "$win32_sys" -L -F 'xactengine*.dll' "$cab" >/dev/null 2>&1 && xact_count=$((xact_count + 1))
                cabextract -d "$win32_sys" -L -F 'xaudio*.dll' "$cab" >/dev/null 2>&1 && xact_count=$((xact_count + 1))
                cabextract -d "$win32_sys" -L -F 'x3daudio*.dll' "$cab" >/dev/null 2>&1 && xact_count=$((xact_count + 1))
                cabextract -d "$win32_sys" -L -F 'xapofx*.dll' "$cab" >/dev/null 2>&1 && xact_count=$((xact_count + 1))
            fi
        done
    fi
    
    # Créer les overrides pour tous les DLLs XACT (comme winetricks)
    local xact_overrides="xaudio2_0 xaudio2_1 xaudio2_2 xaudio2_3 xaudio2_4 xaudio2_5 xaudio2_6 xaudio2_7"
    local x3daudio_overrides="x3daudio1_0 x3daudio1_1 x3daudio1_2 x3daudio1_3 x3daudio1_4 x3daudio1_5 x3daudio1_6 x3daudio1_7"
    local xapofx_overrides="xapofx1_1 xapofx1_2 xapofx1_3 xapofx1_4 xapofx1_5"
    local xactengine_overrides="xactengine2_0 xactengine2_1 xactengine2_2 xactengine2_3 xactengine2_4 xactengine2_5 xactengine2_6 xactengine2_7 xactengine2_8 xactengine2_9 xactengine2_10"
    xactengine_overrides="$xactengine_overrides xactengine3_0 xactengine3_1 xactengine3_2 xactengine3_3 xactengine3_4 xactengine3_5 xactengine3_6 xactengine3_7"
    
    for dll in $xact_overrides $x3daudio_overrides $xapofx_overrides $xactengine_overrides; do
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*$dll" /d native,builtin /f >/dev/null 2>&1 || true
    done
    
    # Enregistrer les DLLs x86 avec regsvr32.exe /S (comme winetricks)
    # Note: Pour x86, on utilise wine (32-bit), pas wine64
    if [ -x "$WINE32_BIN" ]; then
        for dll in "$win32_sys"/xactengine*.dll; do
            if [ -f "$dll" ]; then
                WINEPREFIX="$WINEPREFIX" "$WINE32_BIN" "C:\\windows\\syswow64\\regsvr32.exe" /S "$(basename "$dll")" >/dev/null 2>&1 || true
            fi
        done
        
        # xaudio2_0 à xaudio2_7 x86
        for i in 0 1 2 3 4 5 6 7; do
            if [ -f "$win32_sys/xaudio2_${i}.dll" ]; then
                WINEPREFIX="$WINEPREFIX" "$WINE32_BIN" "C:\\windows\\syswow64\\regsvr32.exe" /S "xaudio2_${i}.dll" >/dev/null 2>&1 || true
            fi
        done
    fi
    
    rm -rf "$tmp_dir"
    echo "  ✓ $xact_count DLLs XACT x86 installées"
    
    return 0
}

install_xact_x64() {
    echo "Installation de XACT Engine x64..."
    
    local win64_sys="$WINEPREFIX/drive_c/windows/system32"
    ensure_dir -s "$win64_sys"
    
    local cache_file="$WINCOMPONENTS_CACHE/directx_Jun2010/directx_Jun2010_redist.exe"
    
    if [ ! -f "$cache_file" ]; then
        echo "  ✗ Fichier DirectX non trouvé"
        return 1
    fi
    
    local tmp_dir=$(mktemp -d)
    local xact_count=0
    
    if command -v cabextract &>/dev/null; then
        cabextract -d "$tmp_dir" -L -F '*_xact_*x64*' "$cache_file" >/dev/null 2>&1 || true
        cabextract -d "$tmp_dir" -L -F '*_x3daudio_*x64*' "$cache_file" >/dev/null 2>&1 || true
        cabextract -d "$tmp_dir" -L -F '*_xaudio_*x64*' "$cache_file" >/dev/null 2>&1 || true
        
        for cab in "$tmp_dir"/*.cab; do
            if [ -f "$cab" ]; then
                cabextract -d "$win64_sys" -L -F 'xactengine*.dll' "$cab" >/dev/null 2>&1 && xact_count=$((xact_count + 1))
                cabextract -d "$win64_sys" -L -F 'xaudio*.dll' "$cab" >/dev/null 2>&1 && xact_count=$((xact_count + 1))
                cabextract -d "$win64_sys" -L -F 'x3daudio*.dll' "$cab" >/dev/null 2>&1 && xact_count=$((xact_count + 1))
                cabextract -d "$win64_sys" -L -F 'xapofx*.dll' "$cab" >/dev/null 2>&1 && xact_count=$((xact_count + 1))
            fi
        done
    fi
    
    rm -rf "$tmp_dir"
    echo "  ✓ $xact_count DLLs XACT x64 installées"
    
    # NOTE: Les overrides sont déjà globaux depuis install_xact (x86)
    # Winetricks ne met pas les overrides dans xact_x64
    
    # NOTE: Ne PAS enregistrer les DLLs x64 avec regsvr32
    # Winetricks ne le fait pas car les DLLs x64 XACT sont cassées dans Wine (bug #41618)
    # Les overrides native,builtin suffisent
    
    return 0
}
