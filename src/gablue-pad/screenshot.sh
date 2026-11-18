#!/usr/bin/bash
# screenshot.sh - capture d'écran du banc (remplace takescreenshot).
#
#   ./screenshot.sh
#
# Même convention que l'ancien script : ~/Images/Screenshots/screenshot_<date>.png
# et un son de déclencheur (sounds/shot.wav) — sans lui on ne sait pas si la
# capture est partie.
set -uo pipefail

HERE="$(dirname "$(readlink -f "$0")")"
DIR=${SCREENSHOT_DIR:-$HOME/Images/Screenshots}
f="$DIR/screenshot_$(date +%Y%m%d_%H%M%S).png"

mkdir -p "$DIR"

if ! command -v spectacle > /dev/null 2>&1; then
    echo "ECHEC: spectacle absent"
    exit 1
fi

# spectacle -b : pas de fenêtre ; le & de l'ancien script n'est pas nécessaire
# (l'option -b rend déjà la main une fois la capture faite)
spectacle -f -b -n -o "$f"

if [[ -f "$f" ]]; then
    echo "capture : $f"
else
    echo "ECHEC: fichier non créé ($f)"
fi

# son de déclencheur, comme l'ancien takescreenshot
w="$HERE/sounds/shot.wav"
[[ -f "$w" ]] && command -v ffplay > /dev/null && \
    ffplay -nodisp -autoexit -loglevel quiet "$w" > /dev/null 2>&1

exit 0
