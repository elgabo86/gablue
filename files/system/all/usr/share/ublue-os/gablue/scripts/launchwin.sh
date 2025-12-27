#!/bin/bash

# Analyse des paramètres
fix_mode=false
args=""
fullpath=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --fix)
            fix_mode=true
            shift
            ;;
        --args)
            args="$2"
            shift 2
            ;;
        *)
            fullpath="$1"
            shift
            ;;
    esac
done

# Vérifier si c'est un fichier .wgp
if [[ "$fullpath" == *.wgp ]]; then
    # Mode pack wgpack
    WGPACK_FILE="$(realpath "$fullpath")"
    WGPACK_NAME="$(basename "$WGPACK_FILE" .wgp)"
    # Nettoyer les points et espaces terminaux ( Wine n'aime pas)
    WGPACK_NAME="${WGPACK_NAME%.}"
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

    # Lire le fichier .args si présent (surcharge les arguments en ligne de commande)
    ARGS_FILE="$MOUNT_DIR/.args"
    if [ -f "$ARGS_FILE" ]; then
        wgp_args=$(cat "$ARGS_FILE")
        if [ -n "$wgp_args" ]; then
            args="$wgp_args"
        fi
    fi

    # Lire le fichier .fix si présent (active le fix manette)
    FIX_FILE="$MOUNT_DIR/.fix"
    if [ -f "$FIX_FILE" ]; then
        fix_mode=true
    fi

    # Gestion des sauvegardes depuis le fichier .savepath
    SAVE_FILE="$MOUNT_DIR/.savepath"
    SAVE_WGP_DIR="$MOUNT_DIR/.save"
    WINDOWS_HOME="$HOME/Windows/UserData"
    SAVES_BASE="$WINDOWS_HOME/$USER/AppData/Local/LocalSaves"
    SAVES_DIR="$SAVES_BASE/$WGPACK_NAME"
    if [ -f "$SAVE_FILE" ]; then
        # Lire ligne par ligne (par dossier/fichier)
        while IFS= read -r SAVE_REL_PATH; do
            if [ -n "$SAVE_REL_PATH" ]; then
                SAVE_WGP_ITEM="$SAVE_WGP_DIR/$SAVE_REL_PATH"
                FINAL_SAVE_ITEM="$SAVES_DIR/$SAVE_REL_PATH"

                # Vérifier si c'est un dossier ou un fichier dans .save
                if [ -d "$SAVE_WGP_ITEM" ]; then
                    # C'est un dossier
                    # Copier vers UserData si n'existe pas
                    if [ ! -d "$FINAL_SAVE_ITEM" ]; then
                        echo "Restauration des sauvegardes: $SAVE_REL_PATH"
                        mkdir -p "$FINAL_SAVE_ITEM"
                        cp -r "$SAVE_WGP_ITEM/." "$FINAL_SAVE_ITEM/"
                    fi
                elif [ -f "$SAVE_WGP_ITEM" ]; then
                    # C'est un fichier
                    # Copier vers UserData si n'existe pas
                    if [ ! -f "$FINAL_SAVE_ITEM" ]; then
                        echo "Restauration des sauvegardes: $SAVE_REL_PATH"
                        mkdir -p "$(dirname "$FINAL_SAVE_ITEM")"
                        cp "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM"
                    fi
                fi
            fi
        done < "$SAVE_FILE"
    fi

    # Gestion des fichiers et dossiers d'extra depuis le fichier .extrapath
    EXTRAPATH_FILE="$MOUNT_DIR/.extrapath"
    EXTRA_WGP_DIR="$MOUNT_DIR/.extra"
    EXTRA_DIR="$HOME/.cache/wgp-extra/$WGPACK_NAME"
    if [ -f "$EXTRAPATH_FILE" ]; then
        # Lire ligne par ligne (par fichier/dossier)
        while IFS= read -r EXTRA_REL_PATH; do
            if [ -n "$EXTRA_REL_PATH" ]; then
                EXTRA_WGP_ITEM="$EXTRA_WGP_DIR/$EXTRA_REL_PATH"
                FINAL_EXTRA_ITEM="$EXTRA_DIR/$EXTRA_REL_PATH"

                # Vérifier si c'est un dossier ou un fichier dans .extra
                if [ -d "$EXTRA_WGP_ITEM" ]; then
                    # C'est un dossier
                    # Copier vers cache si n'existe pas
                    if [ ! -d "$FINAL_EXTRA_ITEM" ]; then
                        echo "Restauration des extra: $EXTRA_REL_PATH"
                        mkdir -p "$FINAL_EXTRA_ITEM"
                        cp -r "$EXTRA_WGP_ITEM/." "$FINAL_EXTRA_ITEM/"
                    fi
                elif [ -f "$EXTRA_WGP_ITEM" ]; then
                    # C'est un fichier
                    # Copier vers cache si n'existe pas
                    if [ ! -f "$FINAL_EXTRA_ITEM" ]; then
                        echo "Restauration des extra: $EXTRA_REL_PATH"
                        mkdir -p "$(dirname "$FINAL_EXTRA_ITEM")"
                        cp "$EXTRA_WGP_ITEM" "$FINAL_EXTRA_ITEM"
                    fi
                fi
            fi
        done < "$EXTRAPATH_FILE"
    fi

    if [ "$fix_mode" = true ]; then
        # Mode fix: désactiver DisableHidraw avant le lancement
        sed -i 's/"DisableHidraw"=dword:00000001/"DisableHidraw"=dword:00000000/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg
    else
        # Mode normal: activer DisableHidraw
        sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg
    fi

    # Lancer le jeu
    echo "Lancement de $WGPACK_NAME..."
    if [ -n "$args" ]; then
        /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$FULL_EXE_PATH" --args " $args"
    else
        /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$FULL_EXE_PATH"
    fi

    if [ "$fix_mode" = true ]; then
        # Réactiver DisableHidraw après le lancement en mode fix
        sleep 2
        sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg
    fi

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

if [ "$fix_mode" = true ]; then
    # Mode fix: désactiver DisableHidraw
    sed -i 's/"DisableHidraw"=dword:00000001/"DisableHidraw"=dword:00000000/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg
else
    # Mode normal: activer DisableHidraw
    sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg
fi

# Lancer le jeu avec le chemin
if [ -n "$args" ]; then
    /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$new_fullpath" --args " $args"
else
    /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$new_fullpath"
fi

if [ "$fix_mode" = true ]; then
    # Réactiver DisableHidraw après le lancement en mode fix
    sleep 2
    sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg
fi

# Nettoyer le dossier temporaire si créé
[ -n "$temp_base" ] && rm -rf "$temp_base"
