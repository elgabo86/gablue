#!/bin/bash

################################################################################
# runner.sh - Gestion du runner gwine
#
# Le runner est le build Wine utilisé pour exécuter les jeux.
# Il est installé dans ~/.local/share/gwine/runner
################################################################################

# =============================================================================
# Chemins du runner
# =============================================================================

WINE_DIR="$GWINE_DIR/runner"
WINE_BIN="$WINE_DIR/bin/wine"
WINE32_BIN="$WINE_DIR/bin/wine"
WINESERVER_BIN="$WINE_DIR/bin/wineserver"
VERSION_FILE="$GWINE_DIR/.version"

# =============================================================================
# Initialisation des chemins
# =============================================================================

init_runner_paths() {
    # Chemins déjà définis ci-dessus (WINE_DIR, WINE_BIN, etc.)
    export WINE_DIR WINE_BIN WINE32_BIN WINESERVER_BIN
}

# =============================================================================
# Vérification et téléchargement du runner
# =============================================================================

ensure_runner_installed() {
    # Vérifier si le runner est installé
    if [ -d "$WINE_DIR" ] && [ -f "$WINE_BIN" ]; then
        return 0
    fi

    # Runner manquant, tentative de téléchargement
    echo "Runner gwine non installé. Téléchargement en cours..."

    # Vérifier la connexion réseau
    if ! curl -s --max-time 5 https://github.com > /dev/null 2>&1; then
        # Pas de réseau : tentative d'installation depuis le cache offline
        echo "Pas de connexion internet, tentative d'installation du runner depuis le cache..."
        if install_gwine_from_cache; then
            return 0
        fi
        error_exit "Runner gwine non installé, pas de connexion internet et aucune archive disponible dans le cache"
    fi

    if ! download_gwine "force" "true"; then
        error_exit "Échec du téléchargement de gwine"
    fi

    echo "✓ Runner gwine installé avec succès"
}

# =============================================================================
# Utilitaires
# =============================================================================

is_runner_installed() {
    [ -d "$WINE_DIR" ] && [ -f "$WINE_BIN" ]
}

get_runner_installed_version() {
    if [ -f "$VERSION_FILE" ]; then
        cat "$VERSION_FILE" 2>/dev/null
    else
        echo ""
    fi
}

show_runner_info() {
    local version
    version=$(get_runner_installed_version)

    echo "Runner: gwine"
    if [ -n "$version" ]; then
        echo "Version: $version"
    fi
}
