#!/bin/bash

# Vérifie qu'un fichier .exe est passé en argument
if [ -z "$1" ] || [ ! -f "$1" ]; then
    echo "Erreur : spécifie un fichier .exe en argument"
    exit 1
fi

EXE_PATH="$1"
EXE_NAME=$(basename "$EXE_PATH" .exe)
DESKTOP_NAME=$(echo "$EXE_NAME" | tr '[:upper:]' '[:lower:]' | tr ' ' '-')
DESKTOP_FILE="$HOME/.local/share/applications/$DESKTOP_NAME.desktop"
ICON_TEMP=$(mktemp).png

# Demande le nom personnalisé via kdialog
CUSTOM_NAME=$(kdialog --title "Nom du raccourci" --inputbox "Entrez le nom à afficher dans le menu (par défaut : $EXE_NAME)" "$EXE_NAME")
if [ $? -ne 0 ]; then
    echo "Annulé par l'utilisateur"
    exit 0
fi

# Si l'utilisateur n'entre rien, utilise le nom par défaut
[ -z "$CUSTOM_NAME" ] && CUSTOM_NAME="$EXE_NAME"

# Demande la catégorie via un menu déroulant kdialog avec "Jeux" par défaut
CATEGORY=$(kdialog --title "Choisir une catégorie" --menu "Sélectionnez la catégorie du menu :" --default "Game" \
    "Game" "Jeux" \
    "Multimedia" "Multimédia" \
    "Internet" "Internet" \
    "Utility" "Utilitaire" \
    "Office" "Bureautique" \
    "Development" "Développement" \
    "System" "Système" \
    "Education" "Éducation")
if [ $? -ne 0 ]; then
    echo "Annulé par l'utilisateur"
    exit 0
fi

# Débogage : affiche la catégorie choisie
echo "Catégorie sélectionnée : $CATEGORY"

# Extrait l'icône du .exe
/usr/bin/wrestool -x -t 14 "$EXE_PATH" > "$ICON_TEMP" 2>/dev/null
if [ ! -s "$ICON_TEMP" ]; then
    ICON_PATH="applications-games"  # Fallback si pas d'icône
else
    ICON_PATH="$HOME/.local/share/icons/$DESKTOP_NAME.png"
    mkdir -p "$HOME/.local/share/icons"
    mv "$ICON_TEMP" "$ICON_PATH"
fi

# Crée le fichier .desktop avec la catégorie choisie
cat > "$DESKTOP_FILE" << EOF
[Desktop Entry]
Name=$CUSTOM_NAME
Exec=/usr/share/ublue-os/gablue/scripts/launchwin.sh "$EXE_PATH"
Type=Application
Icon=$ICON_PATH
Terminal=false
Categories=$CATEGORY;
X-KDE-StartupNotify=false
Actions=LaunchFix;DeleteShortcut

[Desktop Action LaunchFix]
Name=Lancer avec fix gamepad
Exec=/usr/share/ublue-os/gablue/scripts/launchwinfix.sh "$EXE_PATH"
Icon=$ICON_PATH

[Desktop Action DeleteShortcut]
Name=Supprimer ce raccourci
Exec=rm -f "$DESKTOP_FILE" && update-desktop-database ~/.local/share/applications
Icon=edit-delete
EOF

# Applique les permissions
chmod +x "$DESKTOP_FILE"

# Met à jour la base de données des applications
update-desktop-database ~/.local/share/applications

echo "Raccourci créé : $DESKTOP_FILE"
echo "Il devrait apparaître dans la catégorie $CATEGORY du menu Plasma avec le nom : $CUSTOM_NAME"
