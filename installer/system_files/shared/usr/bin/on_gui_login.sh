#!/usr/bin/bash
# Script exécuté à l'ouverture de la session live Gablue
# Lance l'installateur Anaconda automatiquement

set -euo pipefail

# Lancer l'installateur après un court délai (laisse Plasma s'initialiser)
sleep 2
liveinst &
