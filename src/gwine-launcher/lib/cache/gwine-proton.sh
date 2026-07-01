#!/bin/bash

################################################################################
# gwine-proton.sh - Téléchargement et installation de gwine
################################################################################

install_gwine_from_cache() {
    local WINE_INSTALL_DIR="$GWINE_DIR/wine"
    local archives_dir="$COMPONENTS_DIR/gwine"
    
    if [ ! -d "$archives_dir" ] || [ -z "$(ls -A "$archives_dir" 2>/dev/null)" ]; then
        return 1
    fi
    
    local latest_archive
    latest_archive=$(ls -1 "$archives_dir"/gwine-*.tar.xz 2>/dev/null | sort -V | tail -1)
    
    if [ -z "$latest_archive" ] || [ ! -f "$latest_archive" ]; then
        return 1
    fi
    
    local version_name
    version_name=$(basename "$latest_archive" .tar.xz)
    
    echo "Installation de $version_name depuis le cache..."
    
    local temp_dir="$CACHE_DIR/.temp_install"
    rm -rf "$temp_dir"
    ensure_dir "$temp_dir" "Impossible de créer le répertoire temporaire"
    
    local extract_dir="$temp_dir/extracted"
    ensure_dir -s "$extract_dir"
    
    if ! tar -xf "$latest_archive" -C "$extract_dir" 2>/dev/null; then
        echo "Échec de l'extraction de l'archive"
        rm -rf "$temp_dir"
        return 1
    fi
    
    local extracted_wine_dir
    extracted_wine_dir=$(find "$extract_dir" -maxdepth 1 -type d -name "*gwine*" | head -1)
    if [ -z "$extracted_wine_dir" ]; then
        extracted_wine_dir=$(find "$extract_dir" -maxdepth 1 -type d | head -1)
    fi
    if [ -z "$extracted_wine_dir" ]; then
        extracted_wine_dir="$extract_dir"
    fi
    
    rm -rf "$WINE_INSTALL_DIR"
    ensure_dir "$WINE_INSTALL_DIR" "Impossible de créer le répertoire d'installation Wine"
    cp -r "$extracted_wine_dir"/* "$WINE_INSTALL_DIR/"
    
    if [ ! -f "$WINE_INSTALL_DIR/bin/wine" ]; then
        echo "Échec de l'installation"
        rm -rf "$WINE_INSTALL_DIR"
        rm -rf "$temp_dir"
        return 1
    fi
    
    echo "$version_name" > "$GWINE_DIR/.version"
    rm -rf "$temp_dir"
    
    WINE_DIR="$WINE_INSTALL_DIR"
    WINE_BIN="$WINE_INSTALL_DIR/bin/wine"
    WINESERVER_BIN="$WINE_INSTALL_DIR/bin/wineserver"
    
    echo "✓ $version_name installé depuis le cache"
    return 0
}

install_gwine_from_extracted_internal() {
    local extracted_dir="$1"
    local install_dir="$2"
    local backup_dir="$GWINE_DIR/.wine-backup"
    
    local wine_subdir
    wine_subdir=$(find "$extracted_dir" -maxdepth 1 -type d -name "*gwine*" | head -1)
    if [ -z "$wine_subdir" ]; then
        wine_subdir=$(find "$extracted_dir" -maxdepth 1 -type d | head -1)
    fi
    [ -z "$wine_subdir" ] && wine_subdir="$extracted_dir"
    
    local old_backup=""
    if [ -d "$install_dir" ]; then
        old_backup=$(backup_component "$install_dir")
    fi
    
    echo "Installation de la nouvelle version..."
    ensure_dir "$install_dir" "Impossible de créer le répertoire d'installation"
    cp -r "$wine_subdir"/* "$install_dir/"
    
    if [ ! -f "$install_dir/bin/wine" ]; then
        echo "Échec de l'installation"
        rm -rf "$install_dir"
        if [ -n "$old_backup" ]; then
            restore_backup_component "$old_backup" "$install_dir"
            echo "Ancienne version restaurée avec succès"
        fi
        return 1
    fi
    
    cleanup_backup_component "$old_backup"
    return 0
}

# Récupère la dernière version de gwine depuis GitHub
get_latest_gwine_version() {
    local version
    version=$(curl -s --max-time 10 https://api.github.com/repos/elgabo86/gwine/releases/latest 2>/dev/null | grep -oP '"tag_name":\s*"\K[^"]+' | head -1)
    
    if [ -z "$version" ]; then
        return 1
    fi
    
    # Filtrer pour ne garder que les versions gwine (pas gwine-proton)
    # Les versions gwine standard ne contiennent pas "proton" dans le tag
    if [[ "$version" == *proton* ]]; then
        # Si la dernière release est proton, chercher la release précédente
        version=$(curl -s --max-time 10 https://api.github.com/repos/elgabo86/gwine/releases 2>/dev/null | grep -oP '"tag_name":\s*"\K[^"]+' | grep -v proton | head -1)
    fi
    
    echo "$version"
}

download_gwine() {
    local WINE_INSTALL_DIR="$GWINE_DIR/wine"
    local current_version=""
    local latest_version=""
    local force_update="${1:-false}"
    local auto_mode="${2:-false}"
    
    if [ -d "$WINE_INSTALL_DIR" ] && [ -f "$WINE_INSTALL_DIR/bin/wine" ]; then
        current_version=$(cat "$GWINE_DIR/.version" 2>/dev/null || echo "unknown")
    fi
    
    echo "Vérification des mises à jour de gwine..."
    latest_version=$(get_latest_gwine_version)
    
    if [ -z "$latest_version" ]; then
        echo "Impossible de récupérer la dernière version (pas de connexion internet)"
        echo "Tentative d'installation depuis le cache..."
        if install_gwine_from_cache; then
            return 0
        else
            echo "Échec: aucune archive disponible dans le cache"
            return 1
        fi
    fi
    
    echo "Version installée: ${current_version:-Aucune}"
    echo "Dernière version disponible: $latest_version"
    
    if [ "$force_update" = "false" ] && [ "$current_version" = "$latest_version" ]; then
        echo "Vous avez déjà la dernière version de gwine."
        return 0
    fi
    
    if [ "$force_update" = "false" ] && [ -n "$current_version" ]; then
        if ! compare_versions "$latest_version" "$current_version"; then
            echo "Vous avez déjà la dernière version de gwine."
            return 0
        fi
    fi
    
    if [ "$auto_mode" != "true" ] && [ "$init_mode" != "true" ]; then
        echo ""
        echo "Une nouvelle version de gwine est disponible : $latest_version"
        read -p "Voulez-vous télécharger et installer cette version ? [O/n]: " -r
        if [[ ! "$REPLY" =~ ^[OoYy]$ ]] && [ -n "$REPLY" ]; then
            echo "Mise à jour annulée."
            return 0
        fi
    fi
    
    echo ""
    echo "Téléchargement de $latest_version..."
    
    local archives_dir="$COMPONENTS_DIR/gwine"
    local archive_name="${latest_version}.tar.xz"
    local download_url="https://github.com/elgabo86/gwine/releases/download/${latest_version}/${archive_name}"
    local temp_dir="$CACHE_DIR/.temp_gwine"
    local archive_path="$archives_dir/$archive_name"
    
    ensure_dir -s "$archives_dir"
    rm -rf "$temp_dir"
    ensure_dir -s "$temp_dir"
    
    local old_archive
    old_archive=$(find "$archives_dir" -maxdepth 1 -name "gwine-*.tar.xz" | head -1)
    if [ -n "$old_archive" ] && [ "$old_archive" != "$archive_path" ]; then
        echo "Suppression de l'ancienne archive: $(basename "$old_archive")"
        rm -f "$old_archive"
    fi
    
    if [ ! -f "$archive_path" ]; then
        echo "Téléchargement depuis: $download_url"
        if ! download_file "$download_url" "$archive_path" "gwine"; then
            rm -rf "$temp_dir"
            if [ -n "$current_version" ] && [ "$current_version" != "unknown" ]; then
                echo "Utilisation de la version existante: $current_version"
                return 0
            fi
            return 1
        fi
        echo "Archive sauvegardée dans: $archive_path"
    else
        echo "Archive trouvée dans le cache: $archive_path"
    fi
    
    echo "Extraction..."
    local extract_dir="$temp_dir/extracted"
    if ! extract_archive "$archive_path" "$extract_dir" "tar.xz"; then
        rm -rf "$temp_dir"
        if [ -n "$current_version" ] && [ "$current_version" != "unknown" ]; then
            echo "Utilisation de la version existante: $current_version"
            return 0
        fi
        return 1
    fi
    
    if install_gwine_from_extracted_internal "$extract_dir" "$WINE_INSTALL_DIR"; then
        echo "$latest_version" > "$GWINE_DIR/.version"
        
        WINE_DIR="$WINE_INSTALL_DIR"
        WINE_BIN="$WINE_INSTALL_DIR/bin/wine"
        WINESERVER_BIN="$WINE_INSTALL_DIR/bin/wineserver"
        
        rm -rf "$temp_dir"
        echo "✓ gwine $latest_version installé avec succès"
        return 0
    else
        rm -rf "$temp_dir"
        return 1
    fi
}

# Fonctions de compatibilité (alias)
install_gwine_proton_from_cache() {
    install_gwine_from_cache "$@"
}

download_gwine_proton() {
    download_gwine "$@"
}
