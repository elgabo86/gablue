#!/bin/bash
# Version: 1.2

# Vérifie qu'un fichier .exe ou .wgp est passé en argument
if [ -z "$1" ] || [ ! -f "$1" ]; then
    echo "Erreur : spécifie un fichier .exe ou .wgp en argument"
    exit 1
fi

EXE_PATH="$1"

# Déterminer le type de fichier et extraire le nom sans extension
if [[ "$EXE_PATH" == *.wgp ]]; then
    EXE_NAME=$(basename "$EXE_PATH" .wgp)
    FILETYPE="wgp"
else
    EXE_NAME=$(basename "$EXE_PATH" .exe)
    FILETYPE="exe"
fi
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
    ALT_EXEC="qdbus org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.activateLauncherMenu && /usr/share/ublue-os/gablue/scripts/launchwin.sh --fix \"$EXE_PATH\""
else
    EXEC_COMMAND="/usr/share/ublue-os/gablue/scripts/launchwin.sh --fix \"$EXE_PATH\""
    ALT_ACTION="LaunchNormal"
    ALT_NAME="Lancer normal"
    ALT_EXEC="/usr/share/ublue-os/gablue/scripts/launchwin.sh \"$EXE_PATH\""
fi

# Débogage : affiche la catégorie et le mode choisis
echo "Catégorie sélectionnée : $CATEGORY"
echo "Mode de lancement choisi : $LAUNCH_MODE"

# Extrait l'icône (support .exe et .wgp)
if [ "$FILETYPE" = "wgp" ]; then
    # Pour les .wgp : monter temporairement et extraire l'icône depuis l'exécutable
    WGP_FILE="$(realpath "$EXE_PATH")"
    MOUNT_BASE="/tmp/icon_extract_$(date +%s)"
    MOUNT_DIR="$MOUNT_BASE/mount"

    # Créer les dossiers de montage
    mkdir -p "$MOUNT_BASE"

    # Vérifier que squashfuse est disponible
    if command -v squashfuse &> /dev/null; then
        # Monter le squashfs
        mkdir -p "$MOUNT_DIR"
        squashfuse -r "$WGP_FILE" "$MOUNT_DIR" 2>/dev/null

        if [ $? -eq 0 ]; then
            # Lire le fichier .launch pour connaître l'exécutable
            LAUNCH_FILE="$MOUNT_DIR/.launch"
            if [ -f "$LAUNCH_FILE" ]; then
                EXE_IN_WGP=$(cat "$LAUNCH_FILE")
                ICON_EXE_PATH="$MOUNT_DIR/$EXE_IN_WGP"

                if [ -f "$ICON_EXE_PATH" ]; then
                    # Extraire l'icône de meilleure qualité depuis l'exécutable dans le pack
                    wrestool -x -t 14 "$ICON_EXE_PATH" > "$ICON_TEMP" 2>/dev/null
                    if [ -s "$ICON_TEMP" ]; then
                        icotool -x -o "$HOME/.local/share/icons/" --largest "$ICON_TEMP" 2>/dev/null
                        # Move extracted icon to final location
                        mv -f "$HOME/.local/share/icons/"*.png "$ICON_PATH" 2>/dev/null
                    fi
                fi
            fi

            # Démonter le squashfs
            fusermount -u "$MOUNT_DIR" 2>/dev/null
        fi
    fi

    # Nettoyer le dossier de montage
    rm -rf "$MOUNT_BASE"
else
    # Pour les .exe : extraction directe avec meilleure qualité
    wrestool -x -t 14 "$EXE_PATH" > "$ICON_TEMP" 2>/dev/null
    if [ -s "$ICON_TEMP" ]; then
        icotool -x -o "$HOME/.local/share/icons/" --largest "$ICON_TEMP" 2>/dev/null
        # Move extracted icon to final location
        mv -f "$HOME/.local/share/icons/"*.png "$ICON_PATH" 2>/dev/null
    fi
fi

# Définir le chemin final de l'icône
ICON_PATH="$HOME/.local/share/icons/$DESKTOP_NAME.png"
mkdir -p "$HOME/.local/share/icons"

if [ ! -f "$ICON_PATH" ]; then
    ICON_PATH="applications-games"  # Fallback si pas d'icône
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
