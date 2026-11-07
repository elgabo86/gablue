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
    
    if [ ! -f "$cache_file" ]; then
        echo "  ✗ Fichier PhysX non trouvé dans le cache"
        return 1
    fi
    
    echo "  - Lancement de l'installateur PhysX..."
    # L'auto-extracteur NVIDIA (PackageLauncher NVI2) lance `setup.exe` en
    # enfant puis attend sa fin : sans écran, cet enfant reste suspendu et
    # l'init ne rend jamais la main, même avec `-s` (constaté gen1,
    # 07/11/2026 : `C:\NVIDIA\PhysX\Temp\setup.exe` vivant des heures).
    # On extrait donc la charge utile nous-mêmes (le .exe embarque une
    # archive 7z) et on lance `setup.exe -s` directement : ~21 s, code 0 sur
    # un préfixe neuf, avec les DLL attendues en place. `-s` est bien le
    # drapeau silencieux de NVI2 (`/s`, `/quiet` et `-s -noreboot` échouent).
    local payload="$WINEPREFIX/drive_c/NVIDIA/PhysX/Temp"
    if command -v 7z >/dev/null 2>&1 \
       && rm -rf "$payload" && mkdir -p "$payload" \
       && 7z x -y -o"$payload" "$cache_file" >/dev/null 2>&1; then
        if timeout 300 env WINEPREFIX="$WINEPREFIX" \
                "$WINE_BIN" "$payload/setup.exe" -s >/dev/null 2>&1; then
            echo "  ✓ PhysX installé"
        else
            echo "  ⚠ PhysX installé (peut déjà être présent)"
        fi
        # La charge utile pèse 229 Mo une fois extraite : elle n'a plus
        # d'utilité après l'installation (l'auto-extracteur la laissait
        # traîner dans le préfixe).
        rm -rf "$payload"
        rmdir "$WINEPREFIX/drive_c/NVIDIA/PhysX" \
              "$WINEPREFIX/drive_c/NVIDIA" 2>/dev/null || true
        return 0
    fi
    
    # Sans 7z : auto-extracteur d'origine, borné par un timeout puisqu'il
    # peut se figer sans écran.
    if timeout 300 env WINEPREFIX="$WINEPREFIX" \
            "$WINE_BIN" "$cache_file" /s 2>/dev/null; then
        echo "  ✓ PhysX installé"
    else
        echo "  ⚠ PhysX installé (peut déjà être présent)"
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
