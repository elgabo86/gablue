#!/bin/bash

################################################################################
# display-mode.sh - Gestion du mode d'affichage (Wayland vs X11)
#
# Ce module gère la sélection entre le mode Wayland natif et le mode X11/XWayland.
# Les modes disponibles sont:
#   - wayland: Wayland natif (désactive XWayland, unset DISPLAY)
#   - x11: X11/XWayland (mode par défaut, utilise DISPLAY)
#
# Le mode sélectionné est stocké dans ~/.local/share/gwine/options
# sous la forme: display_mode=wayland ou display_mode=x11
################################################################################

# Fichier de configuration du mode d'affichage (même fichier que pour runner/dxvk)
DISPLAY_CONFIG_FILE="$GWINE_DIR/options"

# =============================================================================
# Variables de mode
# =============================================================================

x11_mode=false

# =============================================================================
# Fonctions de gestion du mode d'affichage
# =============================================================================

# Initialise le mode d'affichage au démarrage
# Cette fonction charge la configuration et initialise les variables
init_display_mode() {
    # Le mode d'affichage est déjà initialisé via get_current_display_mode quand nécessaire
    # Cette fonction est un point d'entrée pour une initialisation future si besoin
    :
}

# Obtient le mode d'affichage actuellement configuré
# Retourne: "wayland" ou "x11" (défaut: x11)
get_current_display_mode() {
    if [ -f "$DISPLAY_CONFIG_FILE" ]; then
        local mode
        mode=$(grep "^display_mode=" "$DISPLAY_CONFIG_FILE" 2>/dev/null | cut -d'=' -f2)
        if [ "$mode" = "wayland" ] || [ "$mode" = "x11" ]; then
            echo "$mode"
            return 0
        fi
    fi
    # Défaut: x11
    echo "x11"
}

# Définit le mode d'affichage actif
# Usage: set_display_mode <wayland|x11>
set_display_mode() {
    local mode="$1"
    
    if [ "$mode" != "wayland" ] && [ "$mode" != "x11" ]; then
        echo "Erreur: Mode d'affichage invalide '$mode'. Utilisez 'wayland' ou 'x11'." >&2
        return 1
    fi
    
    # Créer le répertoire gwine si nécessaire
    ensure_dir "$GWINE_DIR"
    
    # Lire le fichier existant
    local config_content=""
    if [ -f "$DISPLAY_CONFIG_FILE" ]; then
        # Supprimer l'ancienne ligne display_mode si elle existe
        config_content=$(grep -v "^display_mode=" "$DISPLAY_CONFIG_FILE" 2>/dev/null || true)
    fi
    
    # Ajouter la nouvelle configuration
    if [ -n "$config_content" ]; then
        echo "$config_content" > "$DISPLAY_CONFIG_FILE"
        echo "display_mode=$mode" >> "$DISPLAY_CONFIG_FILE"
    else
        echo "display_mode=$mode" > "$DISPLAY_CONFIG_FILE"
    fi
    
    return 0
}

# Vérifie si le mode Wayland est utilisé
# Usage: is_wayland_mode
is_wayland_mode() {
    local mode
    mode=$(get_current_display_mode)
    [ "$mode" = "wayland" ]
}

# Affiche le mode d'affichage actuellement utilisé
show_current_display_mode() {
    local mode
    mode=$(get_current_display_mode)
    
    echo "Mode d'affichage: $mode"
    if [ "$mode" = "wayland" ]; then
        echo "Wayland natif (XWayland désactivé)"
    else
        echo "X11/XWayland (mode par défaut)"
    fi
}
