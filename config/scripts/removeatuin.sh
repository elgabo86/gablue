#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -ouex pipefail

if [ -f /etc/profile.d/atuin.sh ]; then
    # Supprime le fichier
    rm -f /etc/profile.d/atuin.sh
    echo "Le fichier /etc/profile.d/atuin.sh a été supprimé."
else
    echo "Le fichier /etc/profile.d/atuin.sh n'existe pas."
fi
