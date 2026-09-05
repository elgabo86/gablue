#!/usr/bin/bash
# Lance Jellyfin Desktop en mode TV plein écran (navigation manette)
# GABLUE_ES_LAUNCH permet à killthemall de ne tuer l'app que si lancée depuis ES-DE
export GABLUE_ES_LAUNCH=1
exec flatpak run org.jellyfin.JellyfinDesktop --tv --fullscreen
