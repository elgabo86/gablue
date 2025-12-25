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

# Définir le chemin final de l'icône AVANT l'extraction
ICON_PATH="$HOME/.local/share/icons/$DESKTOP_NAME.png"
mkdir -p "$HOME/.local/share/icons"

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

# Vérifier si le .fix existe dans le pack .wgp
HAS_FIX=false
if [ "$FILETYPE" = "wgp" ]; then
    MOUNT_BASE="/tmp/wgpack_shortcut_check_$(date +%s)"
    MOUNT_DIR="$MOUNT_BASE/mount"
    mkdir -p "$MOUNT_DIR"

    if command -v squashfuse &> /dev/null; then
        squashfuse -r "$EXE_PATH" "$MOUNT_DIR" 2>/dev/null
        if [ $? -eq 0 ]; then
            if [ -f "$MOUNT_DIR/.fix" ]; then
                HAS_FIX=true
            fi
            fusermount -u "$MOUNT_DIR" 2>/dev/null
        fi
    fi
    rm -rf "$MOUNT_BASE"

    # Si .fix existe, utiliser le mode fix directement sans demander
    if [ "$HAS_FIX" = true ]; then
        LAUNCH_MODE="fix"
    fi
fi

if [ "$LAUNCH_MODE" != "fix" ]; then
    # Demande à l'utilisateur le mode de lancement par défaut
    DEFAULT_NORMAL="on"
    DEFAULT_FIX="off"
    if [ "$HAS_FIX" = true ]; then
        DEFAULT_NORMAL="off"
        DEFAULT_FIX="on"
    fi

    LAUNCH_MODE=$(kdialog --title "Mode de lancement" --radiolist "Choisissez le lancement par défaut :" \
        "normal" "Lancement normal" $DEFAULT_NORMAL \
        "fix" "Lancement avec fix gamepad" $DEFAULT_FIX)
    if [ $? -ne 0 ]; then
        echo "Annulé par l'utilisateur"
        exit 0
    fi
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
                    # Extraire l'icône avec icotool et prendre le PNG le plus gros
                    TMP_ICO=$(mktemp -d)
                    wrestool -x -t 14 "$ICON_EXE_PATH" -o "$TMP_ICO" 2>/dev/null
                    icotool --extract --output="$TMP_ICO" "$TMP_ICO"/*.ico 2>/dev/null
                    BIGGEST_PNG=$(ls -S "$TMP_ICO"/*.png 2>/dev/null | head -1)
                    if [ -n "$BIGGEST_PNG" ]; then
                        cp "$BIGGEST_PNG" "$ICON_PATH"
                    fi
                    rm -rf "$TMP_ICO"
                fi
            fi

            # Démonter le squashfs
            fusermount -u "$MOUNT_DIR" 2>/dev/null
        fi
    fi

    # Nettoyer le dossier de montage
    rm -rf "$MOUNT_BASE"
else
    # Pour les .exe : extraction avec icotool et prendre le PNG le plus gros
    TMP_ICO=$(mktemp -d)
    wrestool -x -t 14 "$EXE_PATH" -o "$TMP_ICO" 2>/dev/null
    icotool --extract --output="$TMP_ICO" "$TMP_ICO"/*.ico 2>/dev/null
    BIGGEST_PNG=$(ls -S "$TMP_ICO"/*.png 2>/dev/null | head -1)
    if [ -n "$BIGGEST_PNG" ]; then
        cp "$BIGGEST_PNG" "$ICON_PATH"
    fi
    rm -rf "$TMP_ICO"
fi

# Fallback si pas d'icône extraite
if [ ! -f "$ICON_PATH" ]; then
    ICON_PATH="applications-games"
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
