#!/usr/bin/bash

################################################################################
# launchwin.sh - Script de migration Bottles vers gwine
#
# Ce script remplace l'ancien lanceur basé sur Bottles.
# Il nettoie l'ancienne bouteille et initialise gwine.
# Les données utilisateur (sauvegardes de jeux) sont conservées.
#
# Fenêtres : Gablue Manager (confirmation + progression), jamais kdialog.
# Sans session graphique, tout se fait dans le terminal.
################################################################################

set -eou pipefail

_ui_available() {
    [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && command -v gablue-manager >/dev/null 2>&1
}

# =============================================================================
# Notification de début de migration
# =============================================================================

if _ui_available; then
    gablue-manager --confirm --title "Migration vers gwine" \
        --text "L'ancien système Bottles va être migré vers Gwine.\n\nVos données utilisateur (sauvegardes de jeux) sont conservées." \
        --yes "Migrer" --no "Annuler" || exit 0
fi

# =============================================================================
# Nettoyage de l'ancien système Bottles
# =============================================================================

echo "Nettoyage de l'ancien système Bottles..."
ujust windows-remove || true

# =============================================================================
# Initialisation de gwine
# =============================================================================

echo "Initialisation de gwine..."
if _ui_available; then
    GABLUE_PROGRESS=1 gablue-manager --progress \
        --title "Migration vers gwine" \
        --subtitle "Création du préfixe Wine et installation des composants." \
        --auto-close-success -- gwine --init || exit 1
else
    gwine --init || exit 1
fi

echo "Migration terminée."

# =============================================================================
# Lancer le programme demandé par l'utilisateur (si un fichier a été passé)
# =============================================================================

if [ -n "${1:-}" ]; then
    echo "Lancement du programme: $1"
    exec gwine "$@"
fi
