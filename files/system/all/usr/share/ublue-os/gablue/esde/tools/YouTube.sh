#!/usr/bin/bash
# Lance VacuumTube (YouTube TV) en plein écran
# GABLUE_ES_LAUNCH permet à killthemall de ne tuer l'app que si lancée depuis ES-DE
export GABLUE_ES_LAUNCH=1
exec flatpak run rocks.shy.VacuumTube --fullscreen
