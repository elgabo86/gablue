#!/bin/bash

################################################################################
# dxvk-mode.sh - Gestion du mode DXVK (standard vs async)
#
# Ce module gère la sélection entre DXVK standard et DXVK-GPLAsync.
# Les modes disponibles sont:
#   - dxvk: DXVK standard (depuis doitsujin/dxvk)
#   - dxvk-async: DXVK-GPLAsync (depuis gitlab.com/Ph42oN/dxvk-gplasync)
#
# Le mode sélectionné est stocké dans ~/.local/share/gwine/options
# sous la forme: dxvk_mode=dxvk ou dxvk_mode=dxvk-async
#
# Lorsque dxvk-async est utilisé, la variable d'environnement DXVK_ASYNC=1
# est automatiquement définie.
################################################################################

# Fichier de configuration du mode DXVK (même fichier que pour le runner)
DXVK_CONFIG_FILE="$GWINE_DIR/options"

# =============================================================================
# Chemins des caches DXVK
# =============================================================================

# Cache pour DXVK standard
DXVK_STANDARD_CACHE_DIR="$COMPONENTS_DIR/dxvk"

# Cache pour DXVK-GPLAsync
DXVK_ASYNC_CACHE_DIR="$COMPONENTS_DIR/dxvk-gplasync"

# =============================================================================
# Variables de mode
# =============================================================================

dxvk_mode=false
dxvk_async_mode=false

# =============================================================================
# Fonctions de gestion du mode DXVK
# =============================================================================

# Initialise le mode DXVK au démarrage
# Cette fonction charge la configuration et initialise les variables
init_dxvk_mode() {
    # Le mode DXVK est déjà initialisé via get_current_dxvk_mode quand nécessaire
    # Cette fonction est un point d'entrée pour une initialisation future si besoin
    :
}

# Obtient le mode DXVK actuellement configuré
# Retourne: "dxvk" ou "dxvk-async" (défaut: dxvk)
get_current_dxvk_mode() {
    if [ -f "$DXVK_CONFIG_FILE" ]; then
        local mode
        mode=$(grep "^dxvk_mode=" "$DXVK_CONFIG_FILE" 2>/dev/null | cut -d'=' -f2)
        if [ "$mode" = "dxvk" ] || [ "$mode" = "dxvk-async" ]; then
            echo "$mode"
            return 0
        fi
    fi
    # Défaut: dxvk
    echo "dxvk"
}

# Définit le mode DXVK actif
# Usage: set_dxvk_mode <dxvk|dxvk-async>
set_dxvk_mode() {
    local mode="$1"
    
    if [ "$mode" != "dxvk" ] && [ "$mode" != "dxvk-async" ]; then
        echo "Erreur: Mode DXVK invalide '$mode'. Utilisez 'dxvk' ou 'dxvk-async'." >&2
        return 1
    fi
    
    # Créer le répertoire gwine si nécessaire
    ensure_dir "$GWINE_DIR"
    
    # Lire le fichier existant
    local config_content=""
    if [ -f "$DXVK_CONFIG_FILE" ]; then
        # Supprimer l'ancienne ligne dxvk_mode si elle existe
        config_content=$(grep -v "^dxvk_mode=" "$DXVK_CONFIG_FILE" 2>/dev/null || true)
    fi
    
    # Ajouter la nouvelle configuration
    if [ -n "$config_content" ]; then
        echo "$config_content" > "$DXVK_CONFIG_FILE"
        echo "dxvk_mode=$mode" >> "$DXVK_CONFIG_FILE"
    else
        echo "dxvk_mode=$mode" > "$DXVK_CONFIG_FILE"
    fi
    
    return 0
}

# Obtient le répertoire de cache DXVK actif selon le mode
# Usage: get_dxvk_cache_dir [mode]
get_dxvk_cache_dir() {
    local mode="${1:-$(get_current_dxvk_mode)}"
    
    if [ "$mode" = "dxvk-async" ]; then
        echo "$DXVK_ASYNC_CACHE_DIR"
    else
        echo "$DXVK_STANDARD_CACHE_DIR"
    fi
}

# Vérifie si dxvk-async est utilisé (pour définir DXVK_ASYNC=1)
# Usage: is_dxvk_async_mode
is_dxvk_async_mode() {
    local mode
    mode=$(get_current_dxvk_mode)
    [ "$mode" = "dxvk-async" ]
}

# Obtient la variable d'environnement DXVK_ASYNC si nécessaire
# Usage: get_dxvk_async_env
get_dxvk_async_env() {
    if is_dxvk_async_mode; then
        echo "1"
    else
        echo ""
    fi
}

# Affiche le mode DXVK actuellement utilisé
show_current_dxvk_mode() {
    local mode
    mode=$(get_current_dxvk_mode)
    
    echo "Mode DXVK: $mode"
    if [ "$mode" = "dxvk-async" ]; then
        echo "DXVK_ASYNC=1 sera défini automatiquement"
    fi
}

# Vérifie si le DXVK du mode actuel est installé
# Usage: is_dxvk_installed [mode]
is_dxvk_installed() {
    local mode="${1:-$(get_current_dxvk_mode)}"
    local cache_dir
    cache_dir=$(get_dxvk_cache_dir "$mode")
    
    # Vérifier si un dossier dxvk-* existe dans le cache
    local dxvk_folder
    dxvk_folder=$(find "$cache_dir" -mindepth 1 -maxdepth 1 -type d -name "dxvk*" 2>/dev/null | head -1)
    
    [ -n "$dxvk_folder" ] && [ -d "$dxvk_folder" ]
}

# =============================================================================
# Fonctions de téléchargement pour dxvk-gplasync
# =============================================================================

# Récupère la dernière version de dxvk-gplasync depuis GitLab
get_latest_dxvk_async_version() {
    local version
    # Utiliser l'API GitLab pour récupérer les releases
    version=$(curl -s --max-time 10 "https://gitlab.com/api/v4/projects/Ph42oN%2Fdxvk-gplasync/releases" 2>/dev/null | grep -oP '"tag_name":\s*"\K[^"]+' | head -1)
    
    if [ -z "$version" ]; then
        return 1
    fi
    
    echo "$version"
}

# Télécharge et installe dxvk-gplasync
download_dxvk_async() {
    local force_update="${1:-false}"
    local auto_mode="${2:-false}"
    
    local cache_dir="$DXVK_ASYNC_CACHE_DIR"
    local current_version=""
    local latest_version=""
    
    ensure_dir "$cache_dir"
    
    # Vérifier la version actuelle
    local current_folder
    current_folder=$(find "$cache_dir" -mindepth 1 -maxdepth 1 -type d -name "dxvk*gplasync*" 2>/dev/null | sort -V | tail -1)
    if [ -n "$current_folder" ]; then
        # Extraire la version du nom de dossier (dxvk-gplasync-v2.7.1-1 -> v2.7.1-1)
        current_version=$(basename "$current_folder" | sed 's/^dxvk-gplasync-//')
    fi
    
    echo "Vérification des mises à jour de DXVK-GPLAsync..."
    latest_version=$(get_latest_dxvk_async_version)
    
    if [ -z "$latest_version" ]; then
        echo "Impossible de récupérer la dernière version (pas de connexion internet)"
        return 1
    fi
    
    echo "Version installée: ${current_version:-Aucune}"
    echo "Dernière version disponible: $latest_version"
    
    if [ "$force_update" = "false" ] && [ "$current_version" = "$latest_version" ]; then
        echo "Vous avez déjà la dernière version de DXVK-GPLAsync."
        return 0
    fi
    
    if [ "$force_update" = "false" ] && [ -n "$current_version" ]; then
        if ! compare_versions "$latest_version" "$current_version"; then
            echo "Vous avez déjà la dernière version de DXVK-GPLAsync."
            return 0
        fi
    fi
    
    if [ "$auto_mode" != "true" ] && [ "${init_mode:-false}" != "true" ]; then
        echo ""
        echo "Une nouvelle version de DXVK-GPLAsync est disponible : $latest_version"
        read -p "Voulez-vous télécharger et installer cette version ? [O/n]: " -r
        if [[ ! "$REPLY" =~ ^[OoYy]$ ]] && [ -n "$REPLY" ]; then
            echo "Mise à jour annulée."
            return 0
        fi
    fi
    
    echo ""
    echo "Téléchargement de DXVK-GPLAsync $latest_version..."
    
    # Construire l'URL de téléchargement depuis le dossier releases du repo
    local download_url="https://gitlab.com/Ph42oN/dxvk-gplasync/-/raw/main/releases/dxvk-gplasync-${latest_version}.tar.gz"
    local temp_dir="$CACHE_DIR/.temp_dxvk_async"
    local archive_path="$cache_dir/dxvk-gplasync-${latest_version}.tar.gz"
    
    rm -rf "$temp_dir"
    ensure_dir -s "$temp_dir"
    
    # Supprimer les anciennes archives
    find "$cache_dir" -maxdepth 1 -name "dxvk-gplasync-*.tar.gz" -delete 2>/dev/null || true
    
    echo "Téléchargement depuis: $download_url"
    if ! curl -L --max-time 120 -o "$archive_path" "$download_url" 2>/dev/null; then
        echo "✗ Échec du téléchargement"
        rm -rf "$temp_dir"
        rm -f "$archive_path"
        return 1
    fi
    
    # Vérifier que l'archive est valide
    if [ ! -s "$archive_path" ]; then
        echo "✗ Archive téléchargée vide ou invalide"
        rm -rf "$temp_dir"
        rm -f "$archive_path"
        return 1
    fi
    
    echo "Extraction..."
    local extract_dir="$temp_dir/extracted"
    ensure_dir -s "$extract_dir"
    
    if ! tar -xzf "$archive_path" -C "$extract_dir" 2>/dev/null; then
        echo "✗ Échec de l'extraction"
        rm -rf "$temp_dir"
        return 1
    fi
    
    # Trouver le dossier extrait
    local extracted_dxvk_dir
    extracted_dxvk_dir=$(find "$extract_dir" -maxdepth 2 -type d -name "dxvk*" | head -1)
    if [ -z "$extracted_dxvk_dir" ]; then
        echo "✗ Dossier DXVK non trouvé dans l'archive"
        rm -rf "$temp_dir"
        return 1
    fi
    
    # Backup de l'ancienne version si elle existe
    local old_backup=""
    if [ -n "$current_folder" ] && [ -d "$current_folder" ]; then
        old_backup=$(backup_component "$current_folder")
    fi
    
    # Supprimer l'ancienne version
    if [ -n "$current_folder" ] && [ -d "$current_folder" ]; then
        rm -rf "$current_folder"
    fi
    
    # Installer la nouvelle version
    local install_dir="$cache_dir/dxvk-gplasync-${latest_version}"
    mv "$extracted_dxvk_dir" "$install_dir"
    
    # Vérifier l'installation
    if [ ! -d "$install_dir" ]; then
        echo "✗ Échec de l'installation"
        if [ -n "$old_backup" ]; then
            restore_backup_component "$old_backup" "$current_folder"
            echo "Ancienne version restaurée"
        fi
        rm -rf "$temp_dir"
        return 1
    fi
    
    cleanup_backup_component "$old_backup"
    rm -rf "$temp_dir"
    
    echo "✓ DXVK-GPLAsync $latest_version installé avec succès"
    return 0
}

# Vérifie et télécharge DXVK si nécessaire selon le mode actuel
# Usage: ensure_dxvk_installed
ensure_dxvk_installed() {
    local mode
    mode=$(get_current_dxvk_mode)
    
    if [ "$mode" = "dxvk-async" ]; then
        # Vérifier si dxvk-async est installé
        if ! is_dxvk_installed "dxvk-async"; then
            echo "DXVK-GPLAsync non installé. Téléchargement en cours..."
            
            # Vérifier la connexion réseau
            if ! curl -s --max-time 5 https://gitlab.com > /dev/null 2>&1; then
                error_exit "DXVK-GPLAsync non installé et pas de connexion internet pour le télécharger"
            fi
            
            if ! download_dxvk_async "force" "true"; then
                error_exit "Échec du téléchargement de DXVK-GPLAsync"
            fi
            
            echo "✓ DXVK-GPLAsync installé avec succès"
        fi
    fi
    # Pour le mode standard, on ne télécharge pas automatiquement (géré par --update)
}
