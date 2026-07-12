#!/bin/bash

################################################################################
# runner.sh - Gestion des runners Wine (gwine et gwine-proton)
#
# Ce module gère la sélection et le chargement des runners Wine.
# Les runners disponibles sont:
#   - wine: gwine standard (TKG modifié)
#   - proton: gwine-proton (version Proton)
#
# Le runner sélectionné est stocké dans ~/.local/share/gwine/options
# sous la forme: runner=wine ou runner=proton
################################################################################

# Fichier de configuration du runner
RUNNER_CONFIG_FILE="$GWINE_DIR/options"

# =============================================================================
# Chemins des runners
# =============================================================================

# Runner wine (gwine standard)
WINE_RUNNER_DIR="$GWINE_DIR/wine"
WINE_RUNNER_BIN="$WINE_RUNNER_DIR/bin/wine"
WINE_RUNNER_WINE32_BIN="$WINE_RUNNER_DIR/bin/wine"
WINE_RUNNER_SERVER_BIN="$WINE_RUNNER_DIR/bin/wineserver"
WINE_RUNNER_VERSION_FILE="$GWINE_DIR/.version"

# Runner proton (gwine-proton)
PROTON_RUNNER_DIR="$GWINE_DIR/wine-proton"
PROTON_RUNNER_BIN="$PROTON_RUNNER_DIR/bin/wine"
PROTON_RUNNER_WINE32_BIN="$PROTON_RUNNER_DIR/bin/wine"
PROTON_RUNNER_SERVER_BIN="$PROTON_RUNNER_DIR/bin/wineserver"
PROTON_RUNNER_VERSION_FILE="$GWINE_DIR/.version-proton"

# =============================================================================
# Variables de mode
# =============================================================================

wine_mode=false
proton_mode=false

# =============================================================================
# Fonctions de gestion du runner
# =============================================================================

# Obtient le runner actuellement configuré
# Retourne: "wine" ou "proton" (défaut: proton)
get_current_runner() {
    if [ -f "$RUNNER_CONFIG_FILE" ]; then
        local runner
        runner=$(grep "^runner=" "$RUNNER_CONFIG_FILE" 2>/dev/null | cut -d'=' -f2)
        if [ "$runner" = "wine" ] || [ "$runner" = "proton" ]; then
            echo "$runner"
            return 0
        fi
    fi
    # Défaut: proton
    echo "proton"
}

# Définit le runner actif
# Usage: set_runner <wine|proton>
set_runner() {
    local runner="$1"
    
    if [ "$runner" != "wine" ] && [ "$runner" != "proton" ]; then
        echo "Erreur: Runner invalide '$runner'. Utilisez 'wine' ou 'proton'." >&2
        return 1
    fi
    
    # Créer le répertoire gwine si nécessaire
    ensure_dir "$GWINE_DIR"
    
    # Sauvegarder le choix
    echo "runner=$runner" > "$RUNNER_CONFIG_FILE"
    
    # Mettre à jour les variables globales
    update_runner_paths "$runner"
    
    return 0
}

# Met à jour les chemins du runner actif
# Usage: update_runner_paths <wine|proton>
update_runner_paths() {
    local runner="${1:-$(get_current_runner)}"
    
    if [ "$runner" = "proton" ]; then
        WINE_DIR="$PROTON_RUNNER_DIR"
        WINE_BIN="$PROTON_RUNNER_BIN"
        WINE32_BIN="$PROTON_RUNNER_WINE32_BIN"
        WINESERVER_BIN="$PROTON_RUNNER_SERVER_BIN"
    else
        # Défaut: wine
        WINE_DIR="$WINE_RUNNER_DIR"
        WINE_BIN="$WINE_RUNNER_BIN"
        WINE32_BIN="$WINE_RUNNER_WINE32_BIN"
        WINESERVER_BIN="$WINE_RUNNER_SERVER_BIN"
    fi
    
    # Exporter pour les sous-processus
    export WINE_DIR WINE_BIN WINE32_BIN WINESERVER_BIN
}

# Initialise le runner au démarrage
# Cette fonction doit être appelée après le parsing des arguments
# Usage: init_runner [--save]
#   --save: sauvegarde le runner dans le fichier options (pour --init ou config seule)
init_runner() {
    local save_mode="${1:-}"
    local current_runner
    current_runner=$(get_current_runner)
    
    # Si --wine ou --proton sont spécifiés explicitement, les utiliser
    if [ "$wine_mode" = true ]; then
        if [ "$save_mode" = "--save" ]; then
            set_runner "wine"  # Sauvegarde dans le fichier options
        else
            update_runner_paths "wine"  # Changement temporaire uniquement
        fi
    elif [ "$proton_mode" = true ]; then
        if [ "$save_mode" = "--save" ]; then
            set_runner "proton"  # Sauvegarde dans le fichier options
        else
            update_runner_paths "proton"  # Changement temporaire uniquement
        fi
    else
        # Sinon utiliser le runner configuré
        update_runner_paths "$current_runner"
    fi
}

# Vérifie et télécharge le runner si nécessaire
# Usage: ensure_runner_installed
ensure_runner_installed() {
    local runner
    runner=$(get_current_runner)
    
    # Vérifier si le runner est installé
    if [ -d "$WINE_DIR" ] && [ -f "$WINE_DIR/bin/wine" ]; then
        return 0
    fi
    
    # Runner manquant, tentative de téléchargement
    echo "Runner $runner non installé. Téléchargement en cours..."
    
    # Vérifier la connexion réseau
    if ! curl -s --max-time 5 https://github.com > /dev/null 2>&1; then
        # Pas de réseau : tentative d'installation du runner depuis le cache offline
        # (permet de déployer uniquement ~/.cache/gwine et d'installer le runner
        # extrait à la volée en cas de rupture d'internet)
        echo "Pas de connexion internet, tentative d'installation du runner depuis le cache..."
        if [ "$runner" = "proton" ]; then
            if install_gwine_proton_from_cache; then
                update_runner_paths "$runner"
                return 0
            fi
        else
            if install_gwine_from_cache; then
                update_runner_paths "$runner"
                return 0
            fi
        fi
        error_exit "Runner $runner non installé, pas de connexion internet et aucune archive disponible dans le cache"
    fi
    
    if [ "$runner" = "proton" ]; then
        if ! download_gwine_proton "force" "true"; then
            error_exit "Échec du téléchargement de gwine-proton"
        fi
    else
        if ! download_gwine "force" "true"; then
            error_exit "Échec du téléchargement de gwine"
        fi
    fi
    
    # Mettre à jour les chemins après installation
    update_runner_paths "$runner"
    
    echo "✓ Runner $runner installé avec succès"
}

# Vérifie si le runner actuel est installé
is_runner_installed() {
    local runner="${1:-$(get_current_runner)}"
    
    if [ "$runner" = "proton" ]; then
        [ -d "$PROTON_RUNNER_DIR" ] && [ -f "$PROTON_RUNNER_BIN" ]
    else
        [ -d "$WINE_RUNNER_DIR" ] && [ -f "$WINE_RUNNER_BIN" ]
    fi
}

# Obtient le répertoire d'installation du runner
get_runner_dir() {
    local runner="${1:-$(get_current_runner)}"
    
    if [ "$runner" = "proton" ]; then
        echo "$PROTON_RUNNER_DIR"
    else
        echo "$WINE_RUNNER_DIR"
    fi
}

# Obtient le binaire wine du runner
get_runner_bin() {
    local runner="${1:-$(get_current_runner)}"
    
    if [ "$runner" = "proton" ]; then
        echo "$PROTON_RUNNER_BIN"
    else
        echo "$WINE_RUNNER_BIN"
    fi
}

# Obtient le binaire wineserver du runner
get_runner_server_bin() {
    local runner="${1:-$(get_current_runner)}"
    
    if [ "$runner" = "proton" ]; then
        echo "$PROTON_RUNNER_SERVER_BIN"
    else
        echo "$WINE_RUNNER_SERVER_BIN"
    fi
}

# Obtient le fichier de version du runner
get_runner_version_file() {
    local runner="${1:-$(get_current_runner)}"
    
    if [ "$runner" = "proton" ]; then
        echo "$PROTON_RUNNER_VERSION_FILE"
    else
        echo "$WINE_RUNNER_VERSION_FILE"
    fi
}

# Obtient la version installée du runner
get_runner_installed_version() {
    local runner="${1:-$(get_current_runner)}"
    local version_file
    version_file=$(get_runner_version_file "$runner")
    
    if [ -f "$version_file" ]; then
        cat "$version_file" 2>/dev/null
    else
        echo ""
    fi
}

# Affiche le runner actuellement utilisé
show_current_runner() {
    local runner
    runner=$(get_current_runner)
    local version
    version=$(get_runner_installed_version "$runner")
    
    echo "Runner actuel: $runner"
    if [ -n "$version" ]; then
        echo "Version: $version"
    fi
}
