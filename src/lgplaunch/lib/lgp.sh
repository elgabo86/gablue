#======================================
# Fonctions de gestion des LGP
#======================================

# Initialise les variables pour le mode LGP
init_lgp_variables() {
    LGPACK_FILE="$(realpath "$fullpath")"
    GAME_INTERNAL_NAME=""

    # Lire le fichier .gamename depuis le lgp pour avoir le nom du jeu pour le montage
    local GAMENAME_CONTENT
    GAMENAME_CONTENT=$(unsquashfs -cat "$LGPACK_FILE" ".gamename" 2>/dev/null)
    if [ -n "$GAMENAME_CONTENT" ]; then
        GAME_INTERNAL_NAME="$GAMENAME_CONTENT"
        # Nettoyer les points et espaces terminaux
        GAME_INTERNAL_NAME="${GAME_INTERNAL_NAME%.}"
    fi

    # Fichier .gamename absent ou vide : utiliser le nom du fichier .lgp
    if [ -z "$GAME_INTERNAL_NAME" ]; then
        GAME_INTERNAL_NAME="$(basename "$LGPACK_FILE" .lgp)"
        GAME_INTERNAL_NAME="${GAME_INTERNAL_NAME%.}"
    fi

    LGPACK_NAME="$GAME_INTERNAL_NAME"

    MOUNT_BASE="/tmp/lgpackmount-$UID"
    MOUNT_DIR="$MOUNT_BASE/$LGPACK_NAME"
    EXTRA_BASE="/tmp/lgp-extra-$UID"
    EXTRA_DIR="$EXTRA_BASE/$LGPACK_NAME"
}

# Monte le squashfs du paquet LGP
mount_lgp() {
    mkdir -p "$MOUNT_BASE"

    # Vérifier si déjà monté
    if mountpoint -q "$MOUNT_DIR"; then
        # Vérifier si un processus utilise encore le montage
        if ! lsof +D "$MOUNT_DIR" > /dev/null 2>&1; then
            # Pas de processus actif : montage orphelin, nettoyer automatiquement
            echo "Montage orphelin détecté pour $LGPACK_NAME, nettoyage..."
            fusermount -uz "$MOUNT_DIR" 2>/dev/null || umount -f "$MOUNT_DIR" 2>/dev/null || true
        else
            # Le jeu tourne vraiment, demander à l'utilisateur
            local QUESTION="$LGPACK_NAME est déjà lancé.\n\nVoulez-vous arrêter l'instance en cours et relancer le jeu ?"
            local RELAUNCH=false

            if command -v kdialog &> /dev/null; then
                kdialog --warningyesno "$QUESTION" --yes-label "Oui, relancer" --no-label "Non, annuler" && RELAUNCH=true
            else
                read -p "$QUESTION (o/N): " -r
                [[ "$REPLY" =~ ^[oOyY]$ ]] && RELAUNCH=true
            fi

            if [ "$RELAUNCH" = true ]; then
                echo "Arrêt de l'instance en cours..."
                # Trouver et tuer les processus utilisant ce mount
                local PIDs
                PIDs=$(lsof +D "$MOUNT_DIR" 2>/dev/null | awk 'NR>1 {print $2}' | sort -u)
                if [ -n "$PIDs" ]; then
                    for pid in $PIDs; do
                        echo "Arrêt du processus $pid"
                        kill -9 "$pid" 2>/dev/null
                    done
                    sleep 1
                fi
                # Vérifier si toujours monté et forcer le démontage
                if mountpoint -q "$MOUNT_DIR"; then
                    fusermount -uz "$MOUNT_DIR" 2>/dev/null || umount -f "$MOUNT_DIR" 2>/dev/null || true
                fi
            else
                error_exit "$LGPACK_NAME est déjà en cours d'exécution"
            fi
        fi
    fi

    # Si le dossier existe mais n'est pas monté et n'est pas vide, le nettoyer
    if [ -d "$MOUNT_DIR" ] && ! mountpoint -q "$MOUNT_DIR"; then
        if [ -n "$(ls -A "$MOUNT_DIR" 2>/dev/null)" ]; then
            echo "Dossier de montage existant et non vide détecté, nettoyage..."
            rm -rf "$MOUNT_DIR"/* "$MOUNT_DIR"/.* 2>/dev/null || true
        fi
        # Supprimer le dossier vide
        rmdir "$MOUNT_DIR" 2>/dev/null || rm -rf "$MOUNT_DIR" 2>/dev/null || true
    fi

    # Vérifier que squashfuse est disponible
    if ! command -v squashfuse &> /dev/null; then
        error_exit "squashfuse n'est pas installé"
    fi

    # Créer et monter le squashfs via FUSE (pas besoin de sudo)
    mkdir -p "$MOUNT_DIR"
    echo "Montage de $LGPACK_FILE sur $MOUNT_DIR (squashfuse)..."
    squashfuse -r "$LGPACK_FILE" "$MOUNT_DIR"

    if [ $? -ne 0 ]; then
        rmdir "$MOUNT_DIR"
        error_exit "Erreur lors du montage du squashfs"
    fi
}

# Nettoie en démontant le LGP et les extras
cleanup_lgp() {
    # Vérifier si le mount est encore utilisé par un nouveau processus
    if mountpoint -q "$MOUNT_DIR" && lsof +D "$MOUNT_DIR" > /dev/null 2>&1; then
        # Une nouvelle instance a pris la main, ne surtout pas démonter
        echo "Une nouvelle instance de $LGPACK_NAME a pris la main, pas de démontage."
        return 0
    fi

    echo "Démontage de $LGPACK_NAME..."

    # Nettoyer les symlinks /tmp/lgp-saves et /tmp/lgp-extra
    cleanup_saves_symlink
    cleanup_extras_symlink
    cleanup_temp_symlink

    # Démontage du squashfs (FUSE)
    if ! fusermount -u "$MOUNT_DIR" 2>/dev/null; then
        # Si échec, tuer le processus squashfuse
        local FUSE_PID=$(fuser -m "$MOUNT_DIR" 2>/dev/null | head -n1)
        if [ -n "$FUSE_PID" ]; then
            kill -9 "$FUSE_PID" 2>/dev/null
            sleep 0.5
        fi
        # Force unmount lazy si nécessaire
        fusermount -uz "$MOUNT_DIR" 2>/dev/null || umount -f -l "$MOUNT_DIR" 2>/dev/null
    fi

    # Attendre un peu que le démontage se termine complètement
    sleep 0.2

    # Vérifier et forcer le démontage si encore monté
    if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        umount -f "$MOUNT_DIR" 2>/dev/null || umount -f -l "$MOUNT_DIR" 2>/dev/null
        sleep 0.1
    fi

    # Supprimer le dossier de montage
    if [ -d "$MOUNT_DIR" ]; then
        rmdir "$MOUNT_DIR" 2>/dev/null || true
    fi
}

# Lit les fichiers de configuration du LGP
read_lgp_config() {
    # Fichier .launch
    local LAUNCH_FILE="$MOUNT_DIR/.launch"
    if [ ! -f "$LAUNCH_FILE" ]; then
        echo "Erreur: fichier .launch introuvable dans le pack" >&2
        cleanup_lgp
        exit 1
    fi

    local launch_content
    launch_content="$(cat "$LAUNCH_FILE" | tr -d '\n\r')"
    FULL_EXE_PATH="$MOUNT_DIR/$launch_content"

    # Résoudre le chemin (les symlinks du namespace rendent realpath fonctionnel)
    if [ -L "$FULL_EXE_PATH" ]; then
        REAL_EXE_PATH="$(realpath "$FULL_EXE_PATH" 2>/dev/null)"
        echo "Symlink détecté: $FULL_EXE_PATH -> $REAL_EXE_PATH"
    else
        REAL_EXE_PATH="$(realpath "$FULL_EXE_PATH" 2>/dev/null)"
        if [ -z "$REAL_EXE_PATH" ]; then
            REAL_EXE_PATH="$FULL_EXE_PATH"
        fi
    fi

    # Vérifier existence
    if [ ! -e "$REAL_EXE_PATH" ]; then
        echo "Erreur: exécutable introuvable: $launch_content" >&2
        cleanup_lgp
        exit 1
    fi

    # Fichier .args (surcharge les arguments en ligne de commande)
    read_lgp_args
}

# Lit le fichier .args du pack (surcharge les arguments en ligne de commande)
read_lgp_args() {
    local ARGS_FILE="$MOUNT_DIR/.args"
    if [ -f "$ARGS_FILE" ]; then
        local lgp_args
        lgp_args=$(cat "$ARGS_FILE")
        if [ -n "$lgp_args" ]; then
            args="$lgp_args"
        fi
    fi
}
