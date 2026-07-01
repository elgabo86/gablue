#!/bin/bash

################################################################################
# fonts.sh - Installation des polices Windows (Core Fonts)
################################################################################

extract_corefont() {
    local font_name="$1"
    local font_file="$2"
    local fonts_dir="$3"
    
    if [ ! -f "$font_file" ]; then
        return 1
    fi
    
    local tmp_dir=$(mktemp -d)
    local font_count=0
    
    if command -v cabextract &>/dev/null; then
        cabextract -d "$tmp_dir" "$font_file" >/dev/null 2>&1 || true
        
        for font in "$tmp_dir"/*.TTF "$tmp_dir"/*.ttf; do
            if [ -f "$font" ]; then
                local dest_name=$(basename "$font" | tr '[:upper:]' '[:lower:]')
                cp -f "$font" "$fonts_dir/$dest_name" 2>/dev/null || true
                font_count=$((font_count + 1))
            fi
        done
    fi
    
    rm -rf "$tmp_dir"
    return $font_count
}

register_fonts_in_wine() {
    local fonts_dir="$1"
    
    for font_file in "$fonts_dir"/*.ttf; do
        if [ -f "$font_file" ]; then
            local font_name=$(basename "$font_file" .ttf)
            local reg_name="$font_name (TrueType)"
            WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_LOCAL_MACHINE\Software\Microsoft\Windows NT\CurrentVersion\Fonts' /v "$reg_name" /d "$(basename "$font_file")" /f >/dev/null 2>&1 || true
        fi
    done
}

install_corefonts() {
    echo "Installation des polices Windows de base (Core Fonts)..."
    
    local fonts_dir="$WINEPREFIX/drive_c/windows/Fonts"
    ensure_dir -s "$fonts_dir"
    
    local fonts_installed=0
    local fonts_missing=0
    
    for font in andale arial arialb comic courier georgia impact times trebuchet verdana webdings; do
        local cache_file="$WINCOMPONENTS_CACHE/corefont_${font}/$(basename "${COMPONENT_URLS[corefont_${font}]}")"
        if [ -f "$cache_file" ]; then
            local count
            extract_corefont "$font" "$cache_file" "$fonts_dir"
            count=$?
            fonts_installed=$((fonts_installed + count))
        else
            fonts_missing=$((fonts_missing + 1))
        fi
    done
    
    local cache_cab="$WINCOMPONENTS_CACHE/tahoma_cab/IELPKTH.CAB"
    if [ -f "$cache_cab" ] && command -v cabextract &>/dev/null; then
        local tmp_dir=$(mktemp -d)
        cabextract -d "$tmp_dir" -F '*.TTF' "$cache_cab" >/dev/null 2>&1 || true
        
        for font in "$tmp_dir"/tahoma*.TTF "$tmp_dir"/tahoma*.ttf; do
            if [ -f "$font" ]; then
                local font_name=$(basename "$font" | tr '[:upper:]' '[:lower:]')
                cp -f "$font" "$fonts_dir/$font_name" 2>/dev/null || true
                fonts_installed=$((fonts_installed + 1))
            fi
        done
        
        rm -rf "$tmp_dir"
    else
        fonts_missing=$((fonts_missing + 1))
    fi
    
    touch "$fonts_dir/corefonts.installed" 2>/dev/null || true
    
    if [ $fonts_installed -gt 0 ]; then
        echo "  - Enregistrement des polices dans le registre..."
        register_fonts_in_wine "$fonts_dir"
    fi
    
    echo "  ✓ $fonts_installed polices installées (${fonts_missing} manquantes)"
    return 0
}
