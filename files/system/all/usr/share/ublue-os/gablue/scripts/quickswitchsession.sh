#!/bin/bash

# ============================================================
# ideestud-chooser.sh — Switch vers la prochaine session graphique
# ============================================================

# Session en cours (via variable d'environnement logind)
CURRENT_SESSION="${XDG_SESSION_ID}"

# Chercher une autre session graphique (type wayland/x11, pas tty)
while read -r sid uid user seat tty rest; do
    [ "$seat" != "seat0" ] && continue
    [ "$sid" = "$CURRENT_SESSION" ] && continue
    type=$(loginctl show-session "$sid" -p Type --value 2>/dev/null)
    [ "$type" != "wayland" ] && [ "$type" != "x11" ] && continue
    loginctl activate "$sid"
    exit 0
done < <(loginctl list-sessions --no-legend 2>/dev/null)

# Aucune autre session graphique → écran de login
busctl call org.freedesktop.DisplayManager \
    /org/freedesktop/DisplayManager/Seat0 \
    org.freedesktop.DisplayManager.Seat \
    SwitchToGreeter 2>/dev/null
