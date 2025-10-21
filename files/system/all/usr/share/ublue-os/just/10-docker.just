# Enable docker
docker-enable:
    #!/usr/bin/bash
    sudo systemctl enable --now docker.service &&
    clear &&
    echo "Docker is now activated" ||
    echo "Error..."

# Disable docker
docker-disable:
    #!/usr/bin/bash
    sudo systemctl disable --now docker.service &&
    clear &&
    echo "Docker is now disabled" ||
    echo "Error..."

# Add dx-group to user
dx-group:
    #!/usr/bin/env bash

    # Sauvegarder le nom d'utilisateur avant d'exécuter les commandes privilégiées
    CURRENT_USER=$(logname)

    # Script temporaire pour les opérations privilégiées
    TEMP_SCRIPT=$(mktemp)
    cat > "$TEMP_SCRIPT" << 'EOF'
    #!/bin/bash
    setup_privileged_groups() {
        local current_user="$1"
        shift
        local groups_to_add=("$@")
        
        for group_name in "${groups_to_add[@]}"; do
            if ! grep -q "^$group_name:" /etc/group; then
                if grep -q "^$group_name:" /usr/lib/group; then
                    echo "Appending $group_name to /etc/group"
                    grep "^$group_name:" /usr/lib/group >> /etc/group
                else
                    echo "Warning: Group $group_name does not exist in /usr/lib/group"
                    continue
                fi
            fi
            
            # Ajouter l'utilisateur au groupe
            usermod -aG "$group_name" "$current_user"
            echo "Added $current_user to group $group_name"
        done
    }

    setup_privileged_groups "$@"
    EOF

    # Rendre le script exécutable
    chmod +x "$TEMP_SCRIPT"

    # Exécuter toutes les opérations privilégiées via pkexec
    pkexec "$TEMP_SCRIPT" "$CURRENT_USER" "docker" "incus-admin" "libvirt" "dialout"

    # Nettoyer
    rm "$TEMP_SCRIPT"

    echo "Reboot system and log back in to use docker, libvirt, incus, and serial connections."
