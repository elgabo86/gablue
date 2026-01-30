#!/bin/bash

# Vérifier si un chemin .wgp est fourni
if [ $# -eq 0 ] || [[ ! "$1" == *.wgp ]]; then
    echo "Usage: $0 /chemin/vers/fichier.wgp"
    exit 1
fi

#======================================
# Variables globales
#======================================
# Normaliser $HOME vers /var/home (chemin réel sur Silverblue/Kinoite)
HOME_REAL="$(realpath "$HOME")"
WINDOWS_HOME="$HOME_REAL/Windows/UserData"
SAVES_SYMLINK="/tmp/wgp-saves"
SAVES_REAL="$WINDOWS_HOME/$USER/LocalSavesWGP"

WGPACK_FILE="$(realpath "$1")"

# Lire le nom du jeu depuis .gamename ou utiliser le nom du fichier
GAME_INTERNAL_NAME=""
GAMENAME_CONTENT=$(unsquashfs -cat "$WGPACK_FILE" ".gamename" 2>/dev/null)
if [ -n "$GAMENAME_CONTENT" ]; then
    GAME_INTERNAL_NAME="$GAMENAME_CONTENT"
    GAME_INTERNAL_NAME="${GAME_INTERNAL_NAME%.}"
fi
if [ -z "$GAME_INTERNAL_NAME" ]; then
    GAME_INTERNAL_NAME="$(basename "$WGPACK_FILE" .wgp)"
    GAME_INTERNAL_NAME="${GAME_INTERNAL_NAME%.}"
fi

WGPACK_NAME="$GAME_INTERNAL_NAME"
MOUNT_BASE="/tmp/wgpackmount"
MOUNT_DIR="$MOUNT_BASE/$WGPACK_NAME"
EXTRA_BASE="/tmp/wgp-extra"
EXTRA_DIR="$EXTRA_BASE/$WGPACK_NAME"

#======================================
# Fonctions utilitaires
#======================================

# Installe les fichiers .reg trouvés dans un dossier via regedit.exe
install_registry_files() {
    local reg_dir="$1"
    local reg_files
    local temp_reg

    # Chercher les fichiers .reg dans le dossier
    reg_files=()
    while IFS= read -r -d '' file; do
        reg_files+=("$file")
    done < <(find "$reg_dir" -maxdepth 1 -name '*.reg' -print0 2>/dev/null)

    # Si aucun fichier .reg, rien à faire
    [ ${#reg_files[@]} -eq 0 ] && return 0

    # Fusionner tous les fichiers .reg en un seul pour éviter les conflits de verrouillage
    temp_reg="$(mktemp)"

    # Écrire l'en-tête Windows Registry
    echo "Windows Registry Editor Version 5.00" > "$temp_reg"

    # Concaténer tous les fichiers (en sautant leur en-tête)
    for reg_file in "${reg_files[@]}"; do
        local reg_name
        reg_name=$(basename "$reg_file")
        echo "Ajout du fichier de registre: $reg_name"

        # Copier en sautant l'en-tête (première ligne)
        tail -n +2 "$reg_file" >> "$temp_reg"
        echo "" >> "$temp_reg"  # Ligne vide entre les fichiers
    done

    # Installer le fichier unique
    echo "Installation du registre fusionné..."
    run_bottles "$HOME_REAL/Windows/WinDrive/windows/regedit.exe" "/S \"$temp_reg\""

    # Nettoyer le fichier temporaire
    rm -f "$temp_reg"
}

# Construit et exécute la commande bottles avec ou sans mesa-git
run_bottles() {
    local exe="$1"
    local cmd_args="$2"

    # Vérifier si mesa-git est demandé
    local MESA_CONFIG="$HOME_REAL/.config/.mesa-git"
    if [ -f "$MESA_CONFIG" ]; then
        echo "Utilisation de Mesa-Git (détecté: $MESA_CONFIG)"
        FLATPAK_GL_DRIVERS=mesa-git /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$exe" --args "$cmd_args"
    else
        /usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$exe" --args "$cmd_args"
    fi
}

# Configure le registre pour le mode fixmanette
apply_padfix_setting() {
    local SYSTEM_REG="$HOME/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg"
    # Mode normal: activer DisableHidraw
    sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' "$SYSTEM_REG"
}

# Restaurer DisableHidraw après le lancement
restore_padfix_setting() {
    nothing="true"
}

#======================================
# Fonctions de gestion des sauvegardes
#======================================

# Prépare les sauvegardes depuis UserData
prepare_saves() {
    local SAVE_FILE="$MOUNT_DIR/.savepath"
    local SAVE_WGP_DIR="$MOUNT_DIR/.save"
    local SAVES_DIR="$SAVES_REAL/$GAME_INTERNAL_NAME"

    [ -f "$SAVE_FILE" ] || return 0

    # Vérifier si le dossier existe et contient du contenu
    if [ -d "$SAVES_DIR" ]; then
        if [ -n "$(find "$SAVES_DIR" -mindepth 1 -maxdepth 1 2>/dev/null)" ]; then
            return 0
        fi
    fi

    echo "Copie des sauvegardes depuis .save..."

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SAVE_WGP_ITEM="$SAVE_WGP_DIR/$SAVE_REL_PATH"
        local FINAL_SAVE_ITEM="$SAVES_DIR/$SAVE_REL_PATH"

        if [ -d "$SAVE_WGP_ITEM" ]; then
            _copy_dir_with_symlinks "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM"
        elif [ -e "$SAVE_WGP_ITEM" ]; then
            mkdir -p "$(dirname "$FINAL_SAVE_ITEM")"
            if [ -L "$SAVE_WGP_ITEM" ]; then
                _copy_symlink_as_abs "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM"
            else
                cp -n "$SAVE_WGP_ITEM" "$FINAL_SAVE_ITEM"
            fi
        fi
    done < "$SAVE_FILE"
}

# Copie un symlink en absolu si target dans /tmp/wgpackmount
_copy_symlink_as_abs() {
    local src_symlink="$1"
    local dst_symlink="$2"

    local target
    target=$(readlink "$src_symlink")
    [ -z "$target" ] && return 1

    local abs_target

    if [[ "$target" == /* ]]; then
        abs_target="$target"
    else
        abs_target=$(realpath -m "$(dirname "$src_symlink")/$target" 2>/dev/null)
        [ -z "$abs_target" ] && return 1
    fi

    if [[ "$abs_target" == */.save/* ]]; then
        abs_target=$(echo "$abs_target" | sed 's|/.save/|/|g')
    fi

    if [[ "$abs_target" == /tmp/wgpackmount/* ]]; then
        ln -s "$abs_target" "$dst_symlink"
    else
        cp -an "$src_symlink" "$dst_symlink"
    fi
}

# Copie récursive un dossier en convertissant les symlinks relatifs
_copy_dir_with_symlinks() {
    local src_dir="$1"
    local dst_dir="$2"

    mkdir -p "$dst_dir"

    for item in "$src_dir"/*; do
        [ -e "$item" ] || [ -L "$item" ] || continue
        local name
        name=$(basename "$item")

        if [ -L "$item" ]; then
            continue
        elif [ -f "$item" ]; then
            cp -n "$item" "$dst_dir/$name"
        elif [ -d "$item" ]; then
            cp -rn --no-preserve=links "$item" "$dst_dir/$name"
        fi
    done

    for item in "$src_dir"/*; do
        [ -e "$item" ] || [ -L "$item" ] || continue
        if [ -L "$item" ]; then
            local name
            name=$(basename "$item")
            _copy_symlink_as_abs "$item" "$dst_dir/$name"
        fi
    done

    for item in "$dst_dir"/*; do
        [ -d "$item" ] || continue
        local name
        name=$(basename "$item")
        local rel_src="$src_dir/$name"
        if [ -d "$rel_src" ]; then
            _copy_dir_with_symlinks "$rel_src" "$item"
        fi
    done
}

# Prépare les fichiers d'extra depuis .extra vers /tmp
prepare_extras() {
    local EXTRAPATH_FILE="$MOUNT_DIR/.extrapath"
    local EXTRA_WGP_DIR="$MOUNT_DIR/.extra"

    [ -f "$EXTRAPATH_FILE" ] || return 0

    mkdir -p "$EXTRA_DIR"

    while IFS= read -r EXTRA_REL_PATH; do
        [ -z "$EXTRA_REL_PATH" ] && continue

        local EXTRA_WGP_ITEM="$EXTRA_WGP_DIR/$EXTRA_REL_PATH"
        local FINAL_EXTRA_ITEM="$EXTRA_DIR/$EXTRA_REL_PATH"

        if [ -d "$EXTRA_WGP_ITEM" ]; then
            mkdir -p "$FINAL_EXTRA_ITEM"
            echo "Copie des extras: $EXTRA_REL_PATH"
            cp -a "$EXTRA_WGP_ITEM"/. "$FINAL_EXTRA_ITEM/"
        elif [ -f "$EXTRA_WGP_ITEM" ]; then
            mkdir -p "$(dirname "$FINAL_EXTRA_ITEM")"
            echo "Copie des extras: $EXTRA_REL_PATH"
            cp "$EXTRA_WGP_ITEM" "$FINAL_EXTRA_ITEM"
        fi
    done < "$EXTRAPATH_FILE"
}

# Crée le symlink /tmp/wgp-saves/$GAME_INTERNAL_NAME vers UserData
setup_saves_symlink() {
    local GAME_SAVES_DIR="$SAVES_REAL/$GAME_INTERNAL_NAME"
    local GAME_SAVES_SYMLINK="$SAVES_SYMLINK"

    mkdir -p "$GAME_SAVES_DIR"
    mkdir -p "$SAVES_SYMLINK"

    if [ -L "$GAME_SAVES_SYMLINK" ]; then
        rm -f "$GAME_SAVES_SYMLINK"
    fi

    ln -s "$GAME_SAVES_DIR" "$GAME_SAVES_SYMLINK"
    echo "Symlink créé: $GAME_SAVES_SYMLINK -> $GAME_SAVES_DIR"
}

# Supprime le symlink /tmp/wgp-saves/$GAME_INTERNAL_NAME
cleanup_saves_symlink() {
    local GAME_SAVES_SYMLINK="$SAVES_SYMLINK"
    [ -L "$GAME_SAVES_SYMLINK" ] && rm -f "$GAME_SAVES_SYMLINK"
}

#======================================
# Script principal
#======================================

# Créer le dossier de montage
mkdir -p "$MOUNT_BASE"

# Vérifier si déjà monté
if mountpoint -q "$MOUNT_DIR"; then
    # Vérifier si un bwrap est actif
    if ! pgrep -f "bwrap.*$(printf '%s' "$MOUNT_DIR" | sed 's/[[\.*^$()+?{|\\]/\\&/g')" > /dev/null 2>&1; then
        echo "Montage orphelin détecté pour $WGPACK_NAME, nettoyage..."
        fusermount -uz "$MOUNT_DIR" 2>/dev/null
    else
        echo "Erreur: $WGPACK_NAME est déjà lancé"
        exit 1
    fi
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

    cleanup_saves_symlink

    if [ -d "$EXTRA_DIR" ]; then
        rm -rf "$EXTRA_DIR"
    fi

    if ! fusermount -u "$MOUNT_DIR" 2>/dev/null; then
        local FUSE_PID=$(fuser -m "$MOUNT_DIR" 2>/dev/null | head -n1)
        if [ -n "$FUSE_PID" ]; then
            kill -9 "$FUSE_PID" 2>/dev/null
            sleep 0.5
        fi
        fusermount -uz "$MOUNT_DIR" 2>/dev/null
    fi

    sleep 0.2

    if mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        umount -f "$MOUNT_DIR" 2>/dev/null || umount -f -l "$MOUNT_DIR" 2>/dev/null
        sleep 0.1
    fi

    if [ -d "$MOUNT_DIR" ]; then
        rmdir "$MOUNT_DIR" 2>/dev/null
    fi
}

# Nettoyer en cas d'interruption
trap cleanup EXIT

# Préparer les saves et extras
setup_saves_symlink
prepare_saves
prepare_extras

# Lister les fichiers .exe dans le pack
found=0
exe_array=()

while IFS= read -r -d '' exe; do
    exe_array+=("$exe")
    found=$((found + 1))
done < <(find "$MOUNT_DIR" -type f -iname "*.exe" -print0 | head -z -n 20)

if [ $found -eq 0 ]; then
    echo "Aucun fichier .exe trouvé dans le pack"
    exit 1
fi

# Construire le menu kdialog
menu_args=("Choisissez un exécutable à lancer :")

for exe in "${exe_array[@]}"; do
    rel_path="${exe#$MOUNT_DIR/}"
    menu_args+=("$rel_path" "$rel_path")
done

# Afficher le menu
EXE_REL_PATH=$(kdialog --menu "${menu_args[@]}")
exit_status=$?

if [ $exit_status -ne 0 ] || [ -z "$EXE_REL_PATH" ]; then
    echo "Annulé par l'utilisateur"
    exit 0
fi

# Chemin complet de l'exécutable
EXE_FULL_PATH="$MOUNT_DIR/$EXE_REL_PATH"

if [ ! -f "$EXE_FULL_PATH" ]; then
    echo "Erreur: exécutable introuvable: $EXE_FULL_PATH"
    exit 1
fi

echo "Lancement de $EXE_REL_PATH..."

# Installer les fichiers .reg dans le dossier de l'exécutable
install_registry_files "$(dirname "$EXE_FULL_PATH")"

apply_padfix_setting

# Lancer le jeu en arrière-plan
run_bottles "$EXE_FULL_PATH" "" &
FLATPAK_PID=$!

# Attendre que le jeu se termine (bloquant, 0% CPU)
echo "En attente de la fermeture du jeu..."
wait "$FLATPAK_PID" 2>/dev/null

# Vérification rapide de sécurité : attendre que bwrap se termine vraiment
local bwrap_pattern
bwrap_pattern=$(printf '%s' "$EXE_FULL_PATH" | sed 's/[[\.*^$()+?{|\\]/\\&/g')
local timeout=50  # 5 secondes max (50 * 0.1s)
while [ $timeout -gt 0 ] && pgrep -f "bwrap.*$bwrap_pattern" > /dev/null 2>&1; do
    sleep 0.1
    ((timeout--))
done

restore_padfix_setting
cleanup_saves_symlink

exit 0
