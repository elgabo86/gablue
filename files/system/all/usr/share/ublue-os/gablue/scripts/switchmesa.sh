#!/bin/bash

MESA_MARKER="$HOME/.config/.mesa-git"

# Vérifier si mesa-git est déjà installé
if [ -f "$MESA_MARKER" ]; then
    # Proposer la désinstallation
    kdialog --yesno "Les drivers Mesa beta sont déjà installés via Flathub Beta.\n\nVoulez-vous les désinstaller ?" --title "Désinstaller drivers Mesa beta"

    if [ $? -eq 0 ]; then
        # Supprimer le remote flathub-beta (les packages seront automatiquement désinstallés)
        if flatpak remote-delete --force flathub-beta 2>/dev/null; then
            # Supprimer le marqueur
            rm "$MESA_MARKER"
            kdialog --msgbox "Les drivers Mesa beta ont été désinstallés avec succès." --title "Désinstallation terminée"
        else
            kdialog --error "Erreur lors de la suppression du dépôt flathub-beta.\nVérifiez que vous avez les droits nécessaires (mot de passe admin)." --title "Erreur"
        fi
    else
        kdialog --msgbox "Annulation. Les drivers Mesa beta restent installés." --title "Annulé"
    fi
else
    # Proposer l'installation
    kdialog --yesno "Les drivers Mesa beta ne sont pas installés.\n\nVoulez-vous les installer depuis Flathub Beta ?" --title "Installer drivers Mesa beta"

    if [ $? -eq 0 ]; then
        # Installer flathub beta
        if flatpak remote-add --if-not-exists --default-branch=flathub --no-enumerate flathub-beta https://flathub.org/beta-repo/flathub-beta.flatpakrepo; then
            flatpak install --noninteractive flathub-beta runtime/org.freedesktop.Platform.GL.mesa-git/x86_64/25.08
            flatpak install --noninteractive flathub-beta runtime/org.freedesktop.Platform.GL32.mesa-git/x86_64/25.08

            # Créer le marqueur d'installation
            touch "$MESA_MARKER"

            kdialog --msgbox "Les drivers Mesa beta ont été installés avec succès depuis Flathub Beta." --title "Installation terminée"
        else
            kdialog --error "Erreur lors de l'ajout du dépôt flathub-beta.\nVérifiez que vous avez les droits nécessaires (mot de passe admin)." --title "Erreur"
        fi
    else
        kdialog --msgbox "Annulation. Les drivers Mesa beta restent non installés." --title "Annulé"
    fi
fi
