#!/bin/bash

# Vérifier si kdialog est installé
if ! command -v kdialog &> /dev/null; then
    echo "Erreur : kdialog n'est pas installé." >&2
    exit 1
fi

# Vérifier si update-desktop-database est installé
if ! command -v update-desktop-database &> /dev/null; then
    echo "Erreur : update-desktop-database n'est pas installé." >&2
    exit 1
fi

# Vérifier si jq est installé (nécessaire pour parser l'API GitLab)
if ! command -v jq &> /dev/null; then
    echo "Erreur : jq n'est pas installé." >&2
    exit 1
fi

# Vérifier si flatpak et Gearlever sont installés
if ! flatpak list | grep -q "it.mijorus.gearlever"; then
    kdialog --title "Installation ES-DE" --error "Erreur : Gearlever n'est pas installé via flatpak."
    exit 1
fi

# Déterminer le dossier de téléchargement
DOWNLOAD_DIR=$(xdg-user-dir DOWNLOAD)
if [ -z "$DOWNLOAD_DIR" ] || [ ! -d "$DOWNLOAD_DIR" ]; then
    kdialog --title "Installation ES-DE" --error "Erreur : impossible de déterminer ou d'accéder au dossier de téléchargement."
    exit 1
fi

# Définir le dossier où Gearlever stocke les AppImages
APPIMAGES_DIR="$HOME/AppImages"
# Si votre système utilise un chemin spécifique comme /var/home/gab/AppImages, décommentez la ligne suivante :
# APPIMAGES_DIR="/var/home/gab/AppImages"
APPIMAGE_FILE="$APPIMAGES_DIR/esde.appimage"

# Vérifier si ES-DE est déjà installé via Gearlever dans ~/AppImages
if [ -f "$APPIMAGE_FILE" ] && flatpak run it.mijorus.gearlever --list-installed | grep -q "ES-DE"; then
    kdialog --title "Installation ES-DE" --yesno "ES-DE semble déjà installé via Gearlever !\n\nFichier trouvé :\n- $APPIMAGE_FILE\n\nVoulez-vous réinstaller ES-DE ?"
    if [ $? -eq 0 ]; then
        echo "Suppression de l'installation existante..."
        echo y | flatpak run it.mijorus.gearlever --remove "$APPIMAGE_FILE"
        update-desktop-database "$HOME/.local/share/applications"
        if [ $? -ne 0 ]; then
            kdialog --title "Installation ES-DE" --error "Erreur lors de la suppression de l'ancienne installation."
            exit 1
        fi
        rm -f "$APPIMAGE_FILE"
    else
        kdialog --title "Installation ES-DE" --msgbox "Installation annulée."
        exit 0
    fi
fi

# Vérifier les permissions du dossier de téléchargement
if [ ! -w "$DOWNLOAD_DIR" ]; then
    kdialog --title "Installation ES-DE" --error "Erreur : pas de permission d'écriture dans $DOWNLOAD_DIR."
    exit 1
fi

# URL de l'API GitLab pour les releases
API_URL="https://gitlab.com/api/v4/projects/es-de%2Femulationstation-de/releases"

# Récupérer le lien de ES-DE_x64.AppImage
echo "Récupération du lien de téléchargement..."
DOWNLOAD_URL=$(curl -s --retry 3 "$API_URL" | jq -r '.[0].assets.links[] | select(.name == "ES-DE_x64.AppImage") | .url')

if [ -z "$DOWNLOAD_URL" ]; then
    kdialog --title "Installation ES-DE" --error "Erreur : impossible de trouver le lien de téléchargement pour ES-DE_x64.AppImage."
    exit 1
fi

# Nom du fichier de destination dans le dossier de téléchargement
FILENAME="$DOWNLOAD_DIR/esde.appimage"

# Télécharger le fichier avec barre de progression visible dans le terminal
echo "Téléchargement de $DOWNLOAD_URL vers $FILENAME..."
curl -L --progress-bar "$DOWNLOAD_URL" -o "$FILENAME"
DOWNLOAD_STATUS=$?

if [ $DOWNLOAD_STATUS -ne 0 ]; then
    rm -f "$FILENAME"
    kdialog --title "Installation ES-DE" --error "Erreur lors du téléchargement de l'AppImage."
    exit 1
fi

# Vérifier si le fichier existe et n'est pas vide
if [ ! -s "$FILENAME" ]; then
    rm -f "$FILENAME"
    kdialog --title "Installation ES-DE" --error "Erreur : le fichier téléchargé est vide ou n'existe pas."
    exit 1
fi

# Rendre le fichier exécutable
echo "Rendre $FILENAME exécutable..."
chmod +x "$FILENAME"

# Installer avec Gearlever
echo "Intégration de ES-DE avec Gearlever..."
echo y | flatpak run it.mijorus.gearlever --integrate "$FILENAME"

# Vérifier si l'installation a réussi
if [ $? -eq 0 ]; then
    DESKTOP_FILE="$HOME/.local/share/applications/esde.desktop"
    if [ -f "$DESKTOP_FILE" ]; then
        if grep -q "Name=ES-DE (.*)" "$DESKTOP_FILE"; then
            sed -i 's/Name=ES-DE (.*/Name=ES-DE/' "$DESKTOP_FILE"
            sed -i '/NoDisplay=true/d' "$DESKTOP_FILE"  # Supprimer NoDisplay si présent
            echo "Fichier .desktop modifié avec succès !"
        fi
        # Vérifier les permissions
        chmod u+rw "$DESKTOP_FILE"
        chmod u+rwx "$HOME/.local/share/applications"
        # Valider le fichier .desktop
        if command -v desktop-file-validate &> /dev/null; then
            desktop-file-validate "$DESKTOP_FILE" || {
                echo "Erreur : le fichier $DESKTOP_FILE est invalide."
                kdialog --title "Installation ES-DE" --error "Le fichier .desktop est invalide."
                exit 1
            }
        fi
        # Mettre à jour la base de données des applications
        update-desktop-database "$HOME/.local/share/applications"
        if [ $? -eq 0 ]; then
            echo "Base de données des applications mise à jour avec succès !"
        else
            echo "Erreur lors de la mise à jour de la base de données des applications."
            kdialog --title "Installation ES-DE" --error "Erreur lors de la mise à jour de la base de données des applications."
        fi
        # Rafraîchir selon l'environnement
        if [ "$XDG_CURRENT_DESKTOP" = "GNOME" ]; then
            gio mime --reset
        elif [ "$XDG_CURRENT_DESKTOP" = "KDE" ]; then
            kbuildsycoca5
        fi
        kdialog --title "Installation ES-DE" --msgbox "Installation de ES-DE terminée avec succès !"
    else
        kdialog --title "Installation ES-DE" --error "Le fichier $DESKTOP_FILE n'a pas été trouvé."
        exit 1
    fi
else
    kdialog --title "Installation ES-DE" --error "Erreur lors de l'installation avec Gearlever."
    exit 1
fi
