#!/bin/bash

################################################################################
# component-winetricks.sh - Gestion de winetricks et composants Windows
################################################################################

# Variable globale pour stocker le PID du processus winetricks en cours
_WINETRICKS_CURRENT_PID=""

# Variable globale pour stocker le chemin du backup du préfixe
PREFIX_BACKUP_PATH=""

# Fonction de nettoyage appelée lors d'une interruption
cleanup_winetricks_interrupt() {
    local pid="$_WINETRICKS_CURRENT_PID"
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null
        wait "$pid" 2>/dev/null
    fi
    trap - INT
    echo ""
    echo "Installation interrompue par l'utilisateur"
    
    if [ -n "$_PROGRESS_DBUS_REF" ]; then
        progress_close "$_PROGRESS_DBUS_REF" 2>/dev/null
    fi
    
    restore_wineprefix || exit 1
    exit 130
}

# Installe les composants Windows
install_winetricks_components() {
    export WINE="$WINE_BIN"
    export PATH="$WINE_DIR/bin:$PATH"
    
    trap cleanup_winetricks_interrupt INT
    
    if ! install_all_wincomponents; then
        trap - INT
        echo ""
        echo "Erreur: L'installation des composants Windows a échoué"
        return 1
    fi
    
    trap - INT
    
    echo "Installation des composants Windows terminée avec succès"
    return 0
}

ensure_winetricks() {
    local WINETRICKS_LOCAL="$GWINE_DIR/bin/winetricks"
    
    if [ -x "$WINETRICKS_LOCAL" ]; then
        echo "winetricks trouvé: $WINETRICKS_LOCAL"
        return 0
    fi
    
    echo "winetricks non trouvé, téléchargement..."
    ensure_dir -s "$GWINE_DIR/bin"
    
    local WINETRICKS_URL="https://raw.githubusercontent.com/Winetricks/winetricks/master/src/winetricks"
    
    if wget -q --show-progress "$WINETRICKS_URL" -O "$WINETRICKS_LOCAL" 2>&1; then
        chmod +x "$WINETRICKS_LOCAL"
        echo "winetricks téléchargé avec succès: $WINETRICKS_LOCAL"
        return 0
    else
        error_exit "Impossible de télécharger winetricks. Vérifiez votre connexion internet."
    fi
}

get_winetricks_bin() {
    local WINETRICKS_LOCAL="$GWINE_DIR/bin/winetricks"
    
    if [ -x "$WINETRICKS_LOCAL" ]; then
        echo "$WINETRICKS_LOCAL"
        return 0
    fi
    
    error_exit "winetricks non trouvé"
}
