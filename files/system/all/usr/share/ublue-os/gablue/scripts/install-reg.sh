#!/bin/bash

# Vérifie qu'un fichier .reg est passé en argument
if [ -z "$1" ] || [ ! -f "$1" ]; then
    echo "Erreur : spécifie un fichier .reg en argument"
    exit 1
fi

REG_FILE="$1"
REAL_RUNNERS_DIR="$HOME/.var/app/com.usebottles.bottles/data/bottles/runners"
SANDBOX_RUNNERS_DIR="/var/data/bottles/runners"

# Cherche un dossier soda* ou gwine* dans le chemin réel
WINE_RUNNER=$(ls -d "$REAL_RUNNERS_DIR"/soda* "$REAL_RUNNERS_DIR"/gwine* 2>/dev/null | head -n 1)
if [ -z "$WINE_RUNNER" ]; then
    kdialog --error "Aucun runner Wine (soda* ou gwine*) trouvé dans $REAL_RUNNERS_DIR"
    exit 1
fi

# Extrait le nom du dossier runner (ex. soda-9.0-1)
RUNNER_NAME=$(basename "$WINE_RUNNER")

# Construit le chemin sandboxé vers wine
WINE_PATH="$SANDBOX_RUNNERS_DIR/$RUNNER_NAME/bin/wine"

# Exécute la commande flatpak avec le chemin sandboxé
flatpak run --env=WINEPREFIX="/var/data/bottles/bottles/def" --env=WINE="$WINE_PATH" --command=regedit com.usebottles.bottles "$REG_FILE"

# Vérifie le succès et affiche un message
if [ $? -eq 0 ]; then
    kdialog --msgbox "Le fichier .reg s'est bien installé"
else
    kdialog --error "Erreur lors de l'installation du fichier .reg"
fi
