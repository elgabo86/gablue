#!/bin/bash
fullpath="$1"

# Vérifier si c'est un fichier .wgp
if [[ "$fullpath" == *.wgp ]]; then
    # Mode pack wgpack
    WGPACK_FILE="$(realpath "$fullpath")"
    WGPACK_NAME="$(basename "$WGPACK_FILE" .wgp)"
    MOUNT_BASE="/tmp/wgpackmount"
    MOUNT_DIR="$MOUNT_BASE/$WGPACK_NAME"

    # Créer le dossier de montage
    mkdir -p "$MOUNT_BASE"

    # Vérifier si déjà monté
    if mountpoint -q "$MOUNT_DIR"; then
        echo "Erreur: $WGPACK_NAME est déjà monté"
        exit 1
    fi

    # Vérifier que squashfuse est disponible
    if ! command -v squashfuse &> /dev/null; then
        echo "Erreur: squashfuse n'est pas installé"
        echo "Installez-le avec: paru -S squashfuse"
        rmdir "$MOUNT_BASE" 2>/dev/null
        exit 1
    fi

    # Créer et monter le squashfs
    mkdir -p "$MOUNT_DIR"
    echo "Montage de $WGPACK_FILE sur $MOUNT_DIR..."
    squashfuse -r "$WGPACK_FILE" "$MOUNT_DIR"

    if [ $? -ne 0 ]; then
        echo "Erreur lors du montage du squashfs"
        rmdir "$MOUNT_DIR"
        exit 1
    fi

    # Fonction de nettoyage
    cleanup() {
        echo "Démontage de $WGPACK_NAME..."
        # D'abord tenter un démontage normal
        if ! fusermount -u "$MOUNT_DIR" 2>/dev/null; then
            # Si échec, tuer le processus squashfuse correspondant
            FUSE_PID=$(fuser -m "$MOUNT_DIR" 2>/dev/null | head -n1)
            if [ -n "$FUSE_PID" ]; then
                kill -9 "$FUSE_PID" 2>/dev/null
                sleep 0.5
            fi
            # Puis force unmount lazy si nécessaire
            fusermount -uz "$MOUNT_DIR" 2>/dev/null
        fi
        # Nettoyer le dossier s'il existe et n'est plus monté
        if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
            umount -f "$MOUNT_DIR" 2>/dev/null
        fi
        rmdir "$MOUNT_DIR" 2>/dev/null
    }

    # Nettoyer en cas d'interruption
    trap cleanup EXIT

    # Lire le fichier .launch pour connaître l'exécutable
    LAUNCH_FILE="$MOUNT_DIR/.launch"
    if [ ! -f "$LAUNCH_FILE" ]; then
        echo "Erreur: fichier .launch introuvable dans le pack"
        cleanup
        exit 1
    fi

    EXE_PATH=$(cat "$LAUNCH_FILE")
    FULL_EXE_PATH="$MOUNT_DIR/$EXE_PATH"

    if [ ! -f "$FULL_EXE_PATH" ]; then
        echo "Erreur: exécutable introuvable: $EXE_PATH"
        cleanup
        exit 1
    fi

    # Appliquer la modification du registre
    sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg

    # Lancer le jeu
    echo "Lancement de $WGPACK_NAME..."
    /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$FULL_EXE_PATH"

    # Nettoyage automatique (le trap EXIT le fera aussi)
    cleanup
    exit 0
fi

# Mode classique (fichier .exe)
dirpath=$(dirname "$fullpath")
filename=$(basename "$fullpath")

# Fonction pour translittérer les caractères accentués
transliterate() {
    local input="$1"
    echo "$input" | iconv -f UTF-8 -t ASCII//TRANSLIT | sed 's/[^a-zA-Z0-9_-]/_/g'
}

# Fonction pour créer un chemin temporaire sans accents
create_temp_path() {
    local path="$1"
    local temp_base="/tmp/game_launcher_$(date +%s)"
    local new_path="$temp_base"
    local current_path=""
    local IFS='/'
    read -ra segments <<< "$path"

    # Parcourir tous les segments du chemin
    for segment in "${segments[@]}"; do
        if [ -n "$segment" ]; then
            current_path="$current_path/$segment"
            clean_segment=$(transliterate "$segment")
            new_path="$new_path/$clean_segment"
            mkdir -p "$new_path"
        fi
    done

    # Créer un lien symbolique pour le contenu du dossier parent final
    ln -sf "$(realpath "$path")"/* "$new_path/"

    echo "$new_path"
}

# Vérifier si le chemin contient des accents
if echo "$dirpath" | grep -P '[^\x00-\x7F]' > /dev/null; then
    new_dirpath=$(create_temp_path "$dirpath")
    new_fullpath="$new_dirpath/$filename"
    temp_base=$(echo "$new_dirpath" | grep -o "/tmp/game_launcher_[0-9]*")
else
    new_fullpath="$fullpath"
    temp_base=""
fi

# Nettoyage du dossier temporaire en cas d'interruption
[ -n "$temp_base" ] && trap 'rm -rf "$temp_base"' EXIT

# Appliquer la modification du registre
sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg

# Lancer le jeu avec le chemin
/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$new_fullpath"

# Nettoyer le dossier temporaire si créé
[ -n "$temp_base" ] && rm -rf "$temp_base"
