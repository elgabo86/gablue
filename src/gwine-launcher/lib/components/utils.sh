#!/bin/bash

################################################################################
# component-utils.sh - Fonctions utilitaires pour l'installation de composants
################################################################################

# Copie des DLLs depuis un dossier source vers les dossiers système Wine
copy_dll_files() {
    local source_dir="$1"
    local dest_win64="$2"
    local dest_win32="$3"
    local dll_list="${4:-}"
    local failed=false
    
    if [ -z "$dll_list" ]; then
        dll_list="*.dll"
    fi
    
    # Copier les DLLs x64
    if [ -d "$source_dir/x64" ]; then
        for dll in $dll_list; do
            local src_file="$source_dir/x64/$dll"
            if [ -f "$src_file" ]; then
                if ! cp -f "$src_file" "$dest_win64/" 2>/dev/null; then
                    echo "    ⚠️  Échec copie $dll (x64)"
                    failed=true
                fi
            fi
        done
    fi
    
    # Copier les DLLs x32/x86
    local x86_dir=""
    if [ -d "$source_dir/x32" ]; then
        x86_dir="$source_dir/x32"
    elif [ -d "$source_dir/x86" ]; then
        x86_dir="$source_dir/x86"
    fi
    
    if [ -n "$x86_dir" ]; then
        for dll in $dll_list; do
            local src_file="$x86_dir/$dll"
            if [ -f "$src_file" ]; then
                if ! cp -f "$src_file" "$dest_win32/" 2>/dev/null; then
                    echo "    ⚠️  Échec copie $dll (x32)"
                    failed=true
                fi
            fi
        done
    fi
    
    [ "$failed" = true ] && return 1
    return 0
}

# Crée des overrides de DLL dans le registre Wine (native,builtin comme winetricks et bottles)
create_dll_overrides() {
    local failed=false
    
    echo "  - Création des overrides de DLL..."
    for dll in "$@"; do
        if ! WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "$dll" /d native,builtin /f >/dev/null 2>&1; then
            echo "    ⚠️  Échec override $dll"
            failed=true
        fi
    done
    echo "  - Overrides créés"
    
    [ "$failed" = true ] && return 1
    return 0
}

# Récupère les chemins system32 et syswow64 du préfixe Wine
# IMPORTANT: Utiliser wine64 pour winepath sur un préfixe 64-bit
get_wine_system_paths() {
    local wine_path_cmd="$WINE_BIN"
    # Si wine64 existe, l'utiliser pour winepath (plus fiable sur 64-bit)
    if [ -x "${WINE_BIN%/*}/wine64" ]; then
        wine_path_cmd="${WINE_BIN%/*}/wine64"
    fi
    
    win64_sys_path=$(WINEPREFIX="$WINEPREFIX" "$wine_path_cmd" winepath -u 'C:\windows\system32' 2>/dev/null | tr -d '\r')
    win32_sys_path=$(WINEPREFIX="$WINEPREFIX" "$wine_path_cmd" winepath -u 'C:\windows\syswow64' 2>/dev/null | tr -d '\r')
    
    if [ -z "$win64_sys_path" ] || [ -z "$win32_sys_path" ] || \
       [[ "$win64_sys_path" == *"err:"* ]] || [[ "$win32_sys_path" == *"err:"* ]]; then
        echo "Erreur: Impossible d'obtenir les chemins system32/syswow64"
        return 1
    fi
    return 0
}

# Installe un composant DLL générique dans le préfixe Wine
install_dll_component() {
    local component_name="$1"
    local cache_dir="$2"
    local pattern="$3"
    local dlls="$4"
    local overrides="${5:-$dlls}"
    local installed=false
    local failed=false
    
    if [ -z "${win64_sys_path:-}" ] || [ -z "${win32_sys_path:-}" ]; then
        if ! get_wine_system_paths; then
            return 1
        fi
    fi
    
    local component_dir
    component_dir=$(find_component_dir "$cache_dir" "$pattern")
    if [ -n "$component_dir" ]; then
        echo "  - Installation de $(basename "$component_dir")..."
        if copy_dll_files "$component_dir" "$win64_sys_path" "$win32_sys_path" "$dlls"; then
            installed=true
        else
            failed=true
        fi
    fi
    
    if [ "$installed" = true ]; then
        if ! create_dll_overrides $overrides; then
            failed=true
        fi
    fi
    
    if [ "$failed" = true ]; then
        echo "Erreur: L'installation de $component_name a rencontré des erreurs"
        return 1
    fi
    
    if [ "$installed" = true ]; then
        echo "$component_name installé avec succès"
        return 0
    else
        echo "$component_name non trouvé dans le cache"
        return 1
    fi
}
