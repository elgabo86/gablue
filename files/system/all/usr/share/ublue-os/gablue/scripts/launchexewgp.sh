#!/bin/bash

# Vérifier si un chemin .wgp est fourni
if [ $# -eq 0 ] || [[ ! "$1" == *.wgp ]]; then
    echo "Usage: $0 /chemin/vers/fichier.wgp"
    exit 1
fi

WGP_FILE="$(realpath "$1")"
WGP_NAME=$(basename "$WGP_FILE" .wgp)
# Nettoyer les points et espaces terminaux ( Wine n'aime pas)
WGP_NAME="${WGP_NAME%.}"
MOUNT_BASE="/tmp/wgpackmount"
MOUNT_DIR="$MOUNT_BASE/$WGP_NAME"

# Créer le dossier de montage
mkdir -p "$MOUNT_BASE"

# Vérifier si déjà monté
if mountpoint -q "$MOUNT_DIR"; then
    echo "Erreur: $WGP_NAME est déjà monté"
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
echo "Montage de $WGP_FILE sur $MOUNT_DIR..."
squashfuse -r "$WGP_FILE" "$MOUNT_DIR"

if [ $? -ne 0 ]; then
    echo "Erreur lors du montage du squashfs"
    rmdir "$MOUNT_DIR"
    exit 1
fi

# Fonction de nettoyage
cleanup() {
    echo "Démontage de $WGP_NAME..."
    fusermount -u "$MOUNT_DIR" 2>/dev/null
    rmdir "$MOUNT_DIR" 2>/dev/null
    rmdir "$MOUNT_BASE" 2>/dev/null
}

# Nettoyer en cas d'interruption
trap cleanup EXIT

# Lister les fichiers .exe dans le pack
found=0
exe_array=()

while IFS= read -r -d '' exe; do
    exe_array+=("$exe")
    found=$((found + 1))
done < <(find "$MOUNT_DIR" -type f -iname "*.exe" -print0 | head -z -n 20)

if [ $found -eq 0 ]; then
    echo "Aucun fichier .exe trouvé dans le pack"
    cleanup
    exit 1
fi

# Construire le menu kdialog
menu_args=("Choisissez un exécutable à lancer :")

for exe in "${exe_array[@]}"; do
    # Chemin relatif pour l'affichage et pour kdialog
    rel_path="${exe#$MOUNT_DIR/}"
    menu_args+=("$rel_path" "$rel_path")
done

# Afficher le menu
EXE_REL_PATH=$(kdialog --menu "${menu_args[@]}")
exit_status=$?

if [ $exit_status -ne 0 ] || [ -z "$EXE_REL_PATH" ]; then
    echo "Annulé par l'utilisateur"
    cleanup
    exit 0
fi

# Chemin complet de l'exécutable
EXE_FULL_PATH="$MOUNT_DIR/$EXE_REL_PATH"

if [ ! -f "$EXE_FULL_PATH" ]; then
    echo "Erreur: exécutable introuvable: $EXE_FULL_PATH"
    cleanup
    exit 1
fi

echo "Lancement de $EXE_REL_PATH..."

# Lancer le jeu avec launchwin.sh en mode normal
sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg

# Lancer le jeu en arrière-plan
/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$EXE_FULL_PATH" &

# Petite pause pour laisser le bwrap se lancer
sleep 1

# Attendre que le jeu se termine
echo "En attente de la fermeture du jeu..."
while pgrep -f "bwrap.*$EXE_FULL_PATH" > /dev/null 2>&1; do
    sleep 1
done

exit 0
