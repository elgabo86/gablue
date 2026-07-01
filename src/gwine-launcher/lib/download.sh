#!/bin/bash

################################################################################
# download.sh - Téléchargement, extraction et gestion des versions GitHub
################################################################################

# Télécharge un fichier depuis une URL avec gestion d'erreur
# Usage: download_file <url> <output_path> [description]
# Retourne 0 si succès, 1 sinon
download_file() {
    local url="$1"
    local output_path="$2"
    local description="${3:-fichier}"
    
    if ! wget -q --show-progress "$url" -O "$output_path" 2>&1; then
        echo "Erreur: Échec du téléchargement de $description"
        return 1
    fi
    return 0
}

# Extrait une archive (tar.gz, tar.xz, ou 7z)
# Usage: extract_archive <archive_path> <dest_dir> [archive_type]
# Retourne 0 si succès, 1 sinon
extract_archive() {
    local archive_path="$1"
    local dest_dir="$2"
    local archive_type="${3:-auto}"
    
    # Détecter le type si auto
    if [ "$archive_type" = "auto" ]; then
        if [[ "$archive_path" == *.tar.gz ]] || [[ "$archive_path" == *.tgz ]]; then
            archive_type="tar.gz"
        elif [[ "$archive_path" == *.tar.xz ]]; then
            archive_type="tar.xz"
        elif [[ "$archive_path" == *.7z ]]; then
            archive_type="7z"
        fi
    fi
    
    case "$archive_type" in
        tar.gz|tgz)
            ensure_dir -s "$dest_dir"
            tar -xzf "$archive_path" -C "$dest_dir" 2>/dev/null || return 1
            ;;
        tar.xz)
            ensure_dir -s "$dest_dir"
            tar -xf "$archive_path" -C "$dest_dir" 2>/dev/null || return 1
            ;;
        7z)
            ensure_dir -s "$dest_dir"
            if command -v 7z &>/dev/null; then
                7z x "$archive_path" -o"$dest_dir/" >/dev/null 2>&1 || return 1
            elif command -v 7za &>/dev/null; then
                7za x "$archive_path" -o"$dest_dir/" >/dev/null 2>&1 || return 1
            else
                echo "Erreur: 7z n'est pas installé"
                return 1
            fi
            ;;
        *)
            echo "Erreur: Type d'archive inconnu: $archive_type"
            return 1
            ;;
    esac
    
    return 0
}

# Fonction générique pour télécharger et installer un composant GitHub
# Usage: download_github_component <cache_dir> <component_name> <version> <url> <archive_type> [no_confirm]
download_github_component() {
    local cache_dir="$1"
    local component_name="$2"
    local version="$3"
    local download_url="$4"
    local archive_type="${5:-tar.gz}"
    local no_confirm="${6:-false}"
    local dest_dir="$cache_dir/${component_name}-${version}"
    
    local old_versions
    old_versions=$(find "$cache_dir" -mindepth 1 -maxdepth 1 -type d -name "${component_name}-*" 2>/dev/null | grep -v "^${dest_dir}$")
    if [ -n "$old_versions" ]; then
        echo "Suppression des anciennes versions de $component_name..."
        rm -rf $old_versions
    fi
    
    if [ -d "$dest_dir" ]; then
        echo "$component_name $version déjà présent dans le cache"
        return 0
    fi
    
    echo "Téléchargement de $component_name $version..."
    
    local temp_dir
    temp_dir=$(mktemp -d)
    local archive_path="$temp_dir/${component_name}.${archive_type}"
    
    if ! download_file "$download_url" "$archive_path" "$component_name"; then
        rm -rf "$temp_dir"
        return 1
    fi
    
    echo "Extraction de $component_name..."
    
    if ! extract_archive "$archive_path" "$dest_dir" "$archive_type"; then
        rm -rf "$temp_dir" "$dest_dir" 2>/dev/null
        echo "Erreur: Échec de l'extraction de $component_name"
        return 1
    fi
    
    rm -rf "$temp_dir"
    echo "$component_name $version téléchargé avec succès"
    return 0
}

# Télécharge et installe un composant depuis GitHub avec backup/restauration automatique
# Usage: download_and_install_component <name> <version> <cache_dir> <pattern> <url> <temp_dir>
download_and_install_component() {
    local name="$1"
    local version="$2"
    local cache_dir="$3"
    local pattern="$4"
    local url="$5"
    local temp_dir="$6"
    
    _COMPONENT_OLD_VERSION=""
    _COMPONENT_SUCCESS=false
    
    echo ""
    echo "Téléchargement de $name $version..."
    
    local archive_temp="$temp_dir/${name}.tar.gz"
    local extract_temp="$temp_dir/.extract_${name}"
    
    # Sauvegarder l'ancienne version
    local old_version
    old_version=$(find "$cache_dir" -mindepth 1 -maxdepth 1 -type d -name "$pattern" | head -1)
    if [ -n "$old_version" ]; then
        mv "$old_version" "$old_version.backup"
        _COMPONENT_OLD_VERSION="$old_version"
    fi
    
    # Télécharger
    if ! wget -q --show-progress "$url" -O "$archive_temp" 2>&1; then
        if [ -n "$_COMPONENT_OLD_VERSION" ]; then
            mv "$_COMPONENT_OLD_VERSION.backup" "$_COMPONENT_OLD_VERSION"
            echo "✗ Échec du téléchargement de $name - Version précédente conservée"
        else
            echo "✗ Échec du téléchargement de $name"
        fi
        return 1
    fi
    
    # Extraire
    rm -rf "$extract_temp"
    ensure_dir -s "$extract_temp"
    
    if tar -xzf "$archive_temp" -C "$extract_temp" 2>/dev/null; then
        ensure_dir -s "$cache_dir/${name}-${version}"
        local extracted_dir
        extracted_dir=$(find "$extract_temp" -mindepth 1 -maxdepth 1 -type d | head -1)
        if [ -n "$extracted_dir" ]; then
            cp -r "$extracted_dir"/* "$cache_dir/${name}-${version}/" 2>/dev/null || true
        fi
        rm -rf "$extract_temp"
        
        if [ -n "$_COMPONENT_OLD_VERSION" ]; then
            rm -rf "$_COMPONENT_OLD_VERSION.backup"
        fi
        echo "✓ $name $version installé"
        _COMPONENT_SUCCESS=true
        return 0
    else
        rm -rf "$extract_temp"
        if [ -n "$_COMPONENT_OLD_VERSION" ]; then
            mv "$_COMPONENT_OLD_VERSION.backup" "$_COMPONENT_OLD_VERSION"
            echo "✗ Échec de l'extraction de $name - Version précédente conservée"
        else
            echo "✗ Échec de l'installation de $name"
        fi
        return 1
    fi
    
    rm -f "$archive_temp"
}

# Récupère la dernière version d'un composant depuis GitHub via flux Atom (sans API)
# Usage: get_github_latest_version <repo> <pattern> <prefix_to_strip>
# Paramètres:
#   repo            : Repository GitHub (format: owner/repo)
#   pattern         : Pattern regex pour extraire la version (ex: 'v[0-9]+\.[0-9]+\.[0-9]+')
#   prefix_to_strip : Préfixe à supprimer de la version (optionnel, ex: "v")
# Retourne la version ou chaîne vide si erreur
get_github_latest_version() {
    local repo="$1"
    local pattern="$2"
    local prefix_to_strip="${3:-}"
    
    local version
    version=$(curl -s "https://github.com/$repo/releases.atom" 2>/dev/null | grep -oE "$pattern" | head -1)
    
    if [ -n "$prefix_to_strip" ] && [[ "$version" == "$prefix_to_strip"* ]]; then
        version="${version#$prefix_to_strip}"
    fi
    
    echo "$version"
}

# Récupère la dernière version depuis une liste de releases GitHub via flux Atom (sans API)
# Usage: get_github_release_version <repo> <pattern> <prefix_to_strip>
get_github_release_version() {
    local repo="$1"
    local pattern="$2"
    local prefix_to_strip="${3:-}"
    
    local version
    version=$(curl -s "https://github.com/$repo/releases.atom" 2>/dev/null | grep -oE "$pattern" | head -1)
    
    if [ -n "$prefix_to_strip" ]; then
        version="${version#$prefix_to_strip}"
    fi
    
    echo "$version"
}

# Récupère la dernière version de gwine depuis GitHub
get_latest_gwine_version() {
    curl -s "https://github.com/elgabo86/gwine/releases.atom" 2>/dev/null | grep -oE 'gwine-[0-9]+\.[0-9]+\.r[0-9]+\.g[0-9a-f]+' | head -1
}

# Récupère la dernière version d'un composant depuis GitHub
# Usage: get_component_version <type>
#   type: dxvk (officiel + bottles), vkd3d (officiel + bottles), dxvk-nvapi (bottles)
# Pour dxvk et vkd3d, compare l'officiel et bottlesdevs/components et prend la plus
# haute version (en cas d'égalité, préfère l'officiel).
# Stocke la source choisie dans les globales _DXVK_SOURCE / _VKD3D_SOURCE ("official" ou "bottles")
get_component_version() {
    local component="$1"
    local version
    local official_version bottles_version
    
    case "$component" in
        dxvk)
            # Source officielle
            official_version=$(curl -s "https://github.com/doitsujin/dxvk/releases.atom" 2>/dev/null | grep -oE "v[0-9]+\.[0-9]+(\.[0-9]+)?" | grep -v "nvapi\|gplasync" | head -1)
            [ -n "$official_version" ] && official_version="${official_version#v}"
            
            # Source bottlesdevs
            bottles_version=$(curl -s "https://github.com/bottlesdevs/components/releases.atom" 2>/dev/null | grep -oE "dxvk-[0-9]+\.[0-9]+(\.[0-9]+)?(-[0-9]+-[0-9a-f]+)?" | grep -v "nvapi\|gplasync" | head -1)
            [ -n "$bottles_version" ] && bottles_version="${bottles_version#dxvk-}"
            
            # Comparer et choisir la meilleure
            if [ -z "$official_version" ] && [ -z "$bottles_version" ]; then
                _DXVK_SOURCE=""
                return 1
            elif [ -z "$official_version" ]; then
                version="$bottles_version"
                _DXVK_SOURCE="bottles"
            elif [ -z "$bottles_version" ]; then
                version="$official_version"
                _DXVK_SOURCE="official"
            elif compare_versions "$bottles_version" "$official_version"; then
                version="$bottles_version"
                _DXVK_SOURCE="bottles"
            else
                version="$official_version"
                _DXVK_SOURCE="official"
            fi
            echo "$version"
            ;;
        vkd3d)
            # Source officielle
            official_version=$(curl -s "https://github.com/HansKristian-Work/vkd3d-proton/releases.atom" 2>/dev/null | grep -oE "v[0-9]+\.[0-9]+(\.[0-9]+)?" | head -1)
            [ -n "$official_version" ] && official_version="${official_version#v}"
            
            # Source bottlesdevs
            bottles_version=$(curl -s "https://github.com/bottlesdevs/components/releases.atom" 2>/dev/null | grep -oE "vkd3d-proton-[0-9]+\.[0-9]+(\.[0-9]+)?" | head -1)
            [ -n "$bottles_version" ] && bottles_version="${bottles_version#vkd3d-proton-}"
            
            # Comparer et choisir la meilleure
            if [ -z "$official_version" ] && [ -z "$bottles_version" ]; then
                _VKD3D_SOURCE=""
                return 1
            elif [ -z "$official_version" ]; then
                version="$bottles_version"
                _VKD3D_SOURCE="bottles"
            elif [ -z "$bottles_version" ]; then
                version="$official_version"
                _VKD3D_SOURCE="official"
            elif compare_versions "$bottles_version" "$official_version"; then
                version="$bottles_version"
                _VKD3D_SOURCE="bottles"
            else
                version="$official_version"
                _VKD3D_SOURCE="official"
            fi
            echo "$version"
            ;;
        dxvk-nvapi)
            version=$(curl -s "https://github.com/bottlesdevs/components/releases.atom" 2>/dev/null | grep -oE "dxvk-nvapi-v[0-9]+\.[0-9]+(\.[0-9]+)?" | head -1)
            [ -n "$version" ] && version="${version#dxvk-nvapi-v}"
            echo "$version"
            ;;
        *)
            return 1
            ;;
    esac
}

# Wrappers pour compatibilité
get_latest_dxvk_version() { get_component_version dxvk; }
get_latest_vkd3d_version() { get_component_version vkd3d; }
get_latest_dxvk_nvapi_version() { get_component_version dxvk-nvapi; }
