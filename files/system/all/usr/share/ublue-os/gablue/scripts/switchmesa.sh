#!/bin/bash

set -eou pipefail

MESA_MARKER="$HOME/.config/.mesa-git"
MESA_GL="runtime/org.freedesktop.Platform.GL.mesa-git/x86_64/25.08"
MESA_GL32="runtime/org.freedesktop.Platform.GL32.mesa-git/x86_64/25.08"

# Vérifier si Flatpak est disponible
if ! command -v flatpak &>/dev/null; then
    kdialog --error "Flatpak n'est pas installé ou n'est pas disponible." --title "Erreur"
    exit 1
fi

# Vérifier si mesa-git est déjà installé
if [ -f "$MESA_MARKER" ]; then
    # Proposer la désinstallation
    kdialog --yesno "Les drivers Mesa beta sont déjà installés via Flathub Beta.\n\nVoulez-vous les désinstaller ?" --title "Désinstaller drivers Mesa beta"

    if [ $? -eq 0 ]; then
        # Désinstaller les packages Mesa beta AVANT de supprimer le remote
        local_uninstall_success=true

        # Désinstaller les packages si installés
        if flatpak list --app-runtime | grep -q "org.freedesktop.Platform.GL.mesa-git"; then
            if ! flatpak uninstall --noninteractive "$MESA_GL" 2>/dev/null; then
                local_uninstall_success=false
            fi
        fi

        if flatpak list --app-runtime | grep -q "org.freedesktop.Platform.GL32.mesa-git"; then
            if ! flatpak uninstall --noninteractive "$MESA_GL32" 2>/dev/null; then
                local_uninstall_success=false
            fi
        fi

        # Supprimer le remote flathub-beta
        if flatpak remotes | grep -q "flathub-beta"; then
            if ! flatpak remote-delete --force flathub-beta 2>/dev/null; then
                kdialog --error "Erreur lors de la suppression du dépôt flathub-beta.\nVérifiez que vous avez les droits nécessaires (mot de passe admin)." --title "Erreur"
                exit 1
            fi
        fi

        # Supprimer le marqueur
        rm -f "$MESA_MARKER"

        if [ "$local_uninstall_success" = true ]; then
            kdialog --msgbox "Les drivers Mesa beta ont été désinstallés avec succès." --title "Désinstallation terminée"
        else
            kdialog --warning "Les drivers Mesa beta ont été désinstallés mais des erreurs sont survenues." --title "Désinstallation terminée avec avertissements"
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
            # Installer les drivers Mesa beta
            local_install_success=true

            if ! flatpak install --noninteractive flathub-beta "$MESA_GL"; then
                local_install_success=false
            fi

            if ! flatpak install --noninteractive flathub-beta "$MESA_GL32"; then
                local_install_success=false
            fi

            if [ "$local_install_success" = true ]; then
                # Créer le marqueur d'installation
                touch "$MESA_MARKER"
                kdialog --msgbox "Les drivers Mesa beta ont été installés avec succès depuis Flathub Beta." --title "Installation terminée"
            else
                kdialog --warning "L'installation des drivers Mesa beta a rencontré des erreurs. Certains packages peuvent ne pas être installés." --title "Installation terminée avec avertissements"
            fi
        else
            kdialog --error "Erreur lors de l'ajout du dépôt flathub-beta.\nVérifiez que vous avez les droits nécessaires (mot de passe admin)." --title "Erreur"
        fi
    else
        kdialog --msgbox "Annulation. Les drivers Mesa beta restent non installés." --title "Annulé"
    fi
fi
