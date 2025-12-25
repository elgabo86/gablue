#!/bin/bash

# Vérifier si un chemin est fourni
if [ $# -eq 0 ]; then
    echo "Usage: $0 /chemin/vers/fichier.exe ou .wgp"
    exit 1
fi

# Normaliser le chemin fourni
fullpath="$1"
onlypath=$(dirname "$fullpath")

# Déterminer le type de fichier et l'extension
if [[ "$fullpath" == *.wgp ]]; then
    onlyapp=$(basename "$fullpath" .wgp)
    filetype="wgp"
else
    onlyapp=$(basename "$fullpath" .exe)
    filetype="exe"
fi

# Vérifier si le .fix existe dans le pack .wgp
choice="normal"
if [ "$filetype" = "wgp" ]; then
    MOUNT_BASE="/tmp/wgpack_winshortcut_check_$(date +%s)"
    MOUNT_DIR="$MOUNT_BASE/mount"
    mkdir -p "$MOUNT_DIR"

    if command -v squashfuse &> /dev/null; then
        squashfuse -r "$fullpath" "$MOUNT_DIR" 2>/dev/null
        if [ $? -eq 0 ]; then
            if [ -f "$MOUNT_DIR/.fix" ]; then
                choice="fix"
            fi
            fusermount -u "$MOUNT_DIR" 2>/dev/null
        fi
    fi
    rm -rf "$MOUNT_BASE"

    # Si .fix n'existe pas, demander le mode
    if [ "$choice" != "fix" ]; then
        choice=$(kdialog --menu "Choisissez le mode de lancement :" \
            "normal" "Lancement normal" \
            "fix" "Lancement avec fix gamepad")

        # Vérifier si l'utilisateur a annulé
        if [ $? -ne 0 ] || [ -z "$choice" ]; then
            echo "Aucun choix effectué, utilisation du lancement normal par défaut"
            choice="normal"
        fi
    fi
else
    # Pour les .exe, demander toujours le mode
    choice=$(kdialog --menu "Choisissez le mode de lancement :" \
        "normal" "Lancement normal" \
        "fix" "Lancement avec fix gamepad")

    # Vérifier si l'utilisateur a annulé
    if [ $? -ne 0 ] || [ -z "$choice" ]; then
        echo "Aucun choix effectué, utilisation du lancement normal par défaut"
        choice="normal"
    fi
fi

# Déterminer le script de lancement à utiliser
LAUNCH_SCRIPT="/usr/share/ublue-os/gablue/scripts/launchwin.sh"

# Générer le script selon le choix
output_sh="$onlypath/$onlyapp.sh"
echo "#!/bin/bash" > "$output_sh"

if [ "$choice" = "fix" ]; then
    echo "exec \"$LAUNCH_SCRIPT\" --fix \"$fullpath\"" >> "$output_sh"
else
    echo "exec \"$LAUNCH_SCRIPT\" \"$fullpath\"" >> "$output_sh"
fi

chmod +x "$output_sh"
echo "Fichier créé : $output_sh"
