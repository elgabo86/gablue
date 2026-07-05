#!/usr/bin/bash
# Script exécuté à l'ouverture de la session live Gablue
# L'installateur Anaconda n'est plus lancé automatiquement
# L'utilisateur peut le lancer manuellement via la commande 'liveinst'

set -euo pipefail

# =============================================================================
# CORRECTION DU DOSSIER BUREAU
# =============================================================================
# livesys-scripts crée le dossier "Desktop" (en anglais) avec liveinst.desktop
# avant que xdg-user-dirs-update ne tourne. Le skel pré-configuré user-dirs.dirs
# indique XDG_DESKTOP_DIR="$HOME/Bureau", mais livesys crée quand même Desktop.
# On déplace liveinst.desktop vers Bureau et on supprime Desktop.

mkdir -p "$HOME/Bureau"
if [ -d "$HOME/Desktop" ]; then
    mv "$HOME/Desktop"/* "$HOME/Bureau/" 2>/dev/null || true
    rmdir "$HOME/Desktop" 2>/dev/null || true
fi
