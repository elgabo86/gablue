#!/bin/bash
# Version: 1.2

# Vérifie qu'un fichier .exe est passé en argument
if [ -z "$1" ] || [ ! -f "$1" ]; then
    echo "Erreur : spécifie un fichier .exe en argument"
    exit 1
fi

EXE_PATH="$1"
EXE_NAME=$(basename "$EXE_PATH" .exe)
DESKTOP_NAME=$(echo "$EXE_NAME" | tr '[:upper:]' '[:lower:]' | tr ' ' '-')
DESKTOP_FILE="$HOME/.local/share/applications/$DESKTOP_NAME.desktop"
DESKTOP_DIR=$(xdg-user-dir DESKTOP)
DESKTOP_SHORTCUT="$DESKTOP_DIR/$DESKTOP_NAME.desktop"
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

# Demande à l'utilisateur le mode de lancement par défaut
LAUNCH_MODE=$(kdialog --title "Mode de lancement" --radiolist "Choisissez le lancement par défaut :" \
    "normal" "Lancement normal" on \
    "fix" "Lancement avec fix gamepad" off)
if [ $? -ne 0 ]; then
    echo "Annulé par l'utilisateur"
    exit 0
fi

# Demande si l'utilisateur veut un raccourci sur le bureau
CREATE_DESKTOP=$(kdialog --title "Raccourci sur le bureau" --yesno "Voulez-vous également ajouter un raccourci sur le bureau ?")
CREATE_DESKTOP_STATUS=$?

# Définit la commande d'exécution principale et l'action alternative
if [ "$LAUNCH_MODE" = "normal" ]; then
    EXEC_COMMAND="/usr/share/ublue-os/gablue/scripts/launchwin.sh \"$EXE_PATH\""
    ALT_ACTION="LaunchFix"
    ALT_NAME="Lancer avec fix gamepad"
    ALT_EXEC="qdbus org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.activateLauncherMenu && /usr/share/ublue-os/gablue/scripts/launchwinfix.sh \"$EXE_PATH\""
else
    EXEC_COMMAND="/usr/share/ublue-os/gablue/scripts/launchwinfix.sh \"$EXE_PATH\""
    ALT_ACTION="LaunchNormal"
    ALT_NAME="Lancer normal"
    ALT_EXEC="/usr/share/ublue-os/gablue/scripts/launchwin.sh \"$EXE_PATH\""
fi

# Débogage : affiche la catégorie et le mode choisis
echo "Catégorie sélectionnée : $CATEGORY"
echo "Mode de lancement choisi : $LAUNCH_MODE"

# Extrait l'icône du .exe
/usr/bin/wrestool -x -t 14 "$EXE_PATH" > "$ICON_TEMP" 2>/dev/null
if [ ! -s "$ICON_TEMP" ]; then
    ICON_PATH="applications-games"  # Fallback si pas d'icône
else
    ICON_PATH="$HOME/.local/share/icons/$DESKTOP_NAME.png"
    mkdir -p "$HOME/.local/share/icons"
    mv "$ICON_TEMP" "$ICON_PATH"
fi

# Crée le fichier .desktop avec l'action alternative et la suppression
cat > "$DESKTOP_FILE" << EOF
[Desktop Entry]
Name=$CUSTOM_NAME
Exec=$EXEC_COMMAND
Type=Application
Icon=$ICON_PATH
Terminal=false
Categories=$CATEGORY;
X-KDE-StartupNotify=false
Actions=$ALT_ACTION;DeleteShortcut

[Desktop Action $ALT_ACTION]
Name=$ALT_NAME
Exec=$ALT_EXEC
Icon=$ICON_PATH

[Desktop Action DeleteShortcut]
Name=Supprimer ce raccourci
Exec=qdbus org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.activateLauncherMenu && rm -f "$DESKTOP_FILE" "$DESKTOP_SHORTCUT" && kbuildsycoca6 && update-desktop-database ~/.local/share/applications
Icon=edit-delete
EOF

# Applique les permissions
chmod +x "$DESKTOP_FILE"

# Crée un lien symbolique sur le bureau si demandé
if [ $CREATE_DESKTOP_STATUS -eq 0 ]; then
    ln -sf "$DESKTOP_FILE" "$DESKTOP_SHORTCUT"
    echo "Raccourci créé sur le bureau : $DESKTOP_SHORTCUT"
fi

# Met à jour la base de données des applications
kbuildsycoca6
update-desktop-database ~/.local/share/applications

echo "Raccourci créé : $DESKTOP_FILE"
echo "Il devrait apparaître dans la catégorie $CATEGORY du menu Plasma avec le nom : $CUSTOM_NAME"
echo "Lancement par défaut : $(if [ "$LAUNCH_MODE" = "normal" ]; then echo "normal"; else echo "avec fix gamepad"; fi)"
