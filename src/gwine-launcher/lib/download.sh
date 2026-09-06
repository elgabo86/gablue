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

# Valide l'intégrité d'une archive téléchargée
# Détecte le format par magic bytes puis teste le flux complet (détecte les
# téléchargements tronqués/corrompus qui restent présents sur disque)
# Usage: validate_archive <archive_path>
# Retourne 0 si l'archive est intacte, 1 sinon
validate_archive() {
    local archive_path="$1"

    # Fichier inexistant ou vide (échec wget crée un fichier vide)
    [ -s "$archive_path" ] || return 1

    local magic
    magic=$(od -An -tx1 -N4 "$archive_path" 2>/dev/null | tr -d ' \n')

    case "$magic" in
        fd377a58*)
            # xz
            xz -t "$archive_path" 2>/dev/null
            ;;
        1f8b*)
            # gzip
            gzip -t "$archive_path" 2>/dev/null
            ;;
        28b52ffd*)
            # zstd
            if command -v zstd &>/dev/null; then
                zstd -t "$archive_path" >/dev/null 2>&1
            else
                tar --zstd -tf "$archive_path" >/dev/null 2>&1
            fi
            ;;
        377abc*)
            # 7z (magic complet: 37 7a bc af 27 1c)
            if command -v 7z &>/dev/null; then
                7z t "$archive_path" >/dev/null 2>&1
            else
                return 1
            fi
            ;;
        504b*)
            # zip
            if command -v unzip &>/dev/null; then
                unzip -tqq "$archive_path" >/dev/null 2>&1
            else
                return 1
            fi
            ;;
        d0cf11e0*)
            # MSI (OLE Compound File)
            if command -v 7z &>/dev/null; then
                7z t "$archive_path" >/dev/null 2>&1
            else
                # Magic valide mais pas d'outil pour tester l'intégrité complète
                return 0
            fi
            ;;
        *)
            # Format inconnu (page d'erreur HTML, JSON, fichier tronqué...)
            return 1
            ;;
    esac
}

# Télécharge une archive avec validation d'intégrité (1 retry si corrompue)
# L'archive est supprimée en cas d'échec final (pas de fichier corrompu en cache)
# Usage: download_archive <url> <output_path> [description]
# Retourne 0 si succès, 1 sinon
download_archive() {
    local url="$1"
    local output_path="$2"
    local description="${3:-archive}"
    local attempt

    for attempt in 1 2; do
        rm -f "$output_path"
        if download_file "$url" "$output_path" "$description" && validate_archive "$output_path"; then
            return 0
        fi
        if [ "$attempt" -lt 2 ] && [ -s "$output_path" ]; then
            echo "Archive corrompue (téléchargement incomplet ?), nouvelle tentative..."
        fi
    done

    rm -f "$output_path"
    echo "Erreur: $description corrompu ou illisible après 2 tentatives de téléchargement"
    return 1
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
    
    if ! download_archive "$download_url" "$archive_path" "$component_name"; then
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
    
    # Télécharger (avec validation d'intégrité et retry)
    if ! download_archive "$url" "$archive_temp" "$name"; then
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
    
    if tar -xf "$archive_temp" -C "$extract_temp" 2>/dev/null; then
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
#   type: dxvk (officiel + bottles), vkd3d (officiel + bottles), dxvk-nvapi (officiel)
# Pour dxvk et vkd3d, compare l'officiel et bottlesdevs/components et prend la plus
# haute version (en cas d'égalité, préfère l'officiel).
# Affiche "version source" (ex: "3.0.2-1-abc1234 bottles") car la fonction est toujours
# appelée en sous-shell (substitution de commande) : une globale serait perdue.
# L'appelant doit parser avec : read -r version _DXVK_SOURCE < <(get_latest_dxvk_version)
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
            echo "$version $_DXVK_SOURCE"
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
            echo "$version $_VKD3D_SOURCE"
            ;;
        dxvk-nvapi)
            # Source officielle unique (jp7677/dxvk-nvapi) : le flux atom de
            # bottlesdevs/components (ancien miroir) ne contient que les ~10
            # dernières releases, dxvk-nvapi y disparaît périodiquement ; l'API
            # GitHub (paginable) est rate-limitée et inutilisable en CI parallèle.
            # Le flux atom jp7677 ne contient que des releases nvapi → fiable.
            version=$(curl -s "https://github.com/jp7677/dxvk-nvapi/releases.atom" 2>/dev/null | grep -oE "v[0-9]+\.[0-9]+(\.[0-9]+)?" | head -1)
            [ -n "$version" ] && version="${version#v}"
            [ -n "$version" ] && echo "$version"
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
