#!/bin/bash

################################################################################
# registry.sh - Installation des fichiers de registre
################################################################################

install_registry_files() {
    local reg_dir="$1"
    local reg_files

    local real_reg_dir
    real_reg_dir="$(realpath "$reg_dir" 2>/dev/null || echo "$reg_dir")"

    reg_files=()
    while IFS= read -r -d '' file; do
        reg_files+=("$file")
    done < <(find "$real_reg_dir" -maxdepth 1 -name '*.reg' -print0 2>/dev/null)

    [ ${#reg_files[@]} -eq 0 ] && return 0

    echo "Installation des fichiers de registre..."
    
    local bottle_c="$WINEPREFIX/drive_c"
    local temp_dir="$bottle_c/windows/temp"
    ensure_dir -s "$temp_dir"
    
    for reg_file in "${reg_files[@]}"; do
        local reg_name
        reg_name=$(basename "$reg_file")
        
        local reg_hash
        reg_hash=$(md5sum "$reg_file" | cut -d' ' -f1)
        
        local dest_file="$temp_dir/${reg_hash}_${reg_name}"
        
        if [ -f "$dest_file" ]; then
            local existing_hash
            existing_hash=$(md5sum "$dest_file" | cut -d' ' -f1)
            if [ "$reg_hash" = "$existing_hash" ]; then
                echo "  - $reg_name (déjà installé)"
                continue
            fi
        fi
        
        echo "  - $reg_name"
        
        cp "$reg_file" "$dest_file"
        
        local win_path="C:\\windows\\temp\\$(basename "$dest_file")"
        
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" regedit.exe /S "$win_path" 2>/dev/null || true
    done
}
