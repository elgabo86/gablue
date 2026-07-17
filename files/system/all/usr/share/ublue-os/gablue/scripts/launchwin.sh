#!/usr/bin/bash

################################################################################
# launchwin.sh - Script de migration Bottles vers gwine
#
# Ce script remplace l'ancien lanceur basé sur Bottles.
# Il nettoie l'ancienne bouteille et initialise gwine.
# Les données utilisateur (sauvegardes de jeux) sont conservées.
################################################################################

set -eou pipefail

# =============================================================================
# Notification de début de migration
# =============================================================================

if command -v kdialog >/dev/null 2>&1 && [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
    kdialog --msgbox "L'ancien système Bottles va être migré vers Gwine.\n\nVos données utilisateur (sauvegardes de jeux) sont conservées." 2>/dev/null || true
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
if command -v kdialog >/dev/null 2>&1 && [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
    kdialog --passivepopup "Initialisation de gwine en cours..." 5 2>/dev/null || true
    if gwine --init --kdialog; then
        kdialog --passivepopup "Migration vers gwine terminée !" 6 2>/dev/null || true
    else
        kdialog --error "L'initialisation de gwine a échoué." 2>/dev/null || true
        exit 1
    fi
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
