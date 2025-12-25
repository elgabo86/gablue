#!/bin/bash

# Vérifier si un chemin est fourni
if [ $# -eq 0 ]; then
    echo "Usage: $0 /chemin/vers/fichier.exe ou .wgp"
    exit 1
fi

# Normaliser le chemin fourni
fullpath="$1"
onlypath=$(dirname "$fullpath")

# Déterminer le type de fichier et l'extension
if [[ "$fullpath" == *.wgp ]]; then
    onlyapp=$(basename "$fullpath" .wgp)
    filetype="wgp"
else
    onlyapp=$(basename "$fullpath" .exe)
    filetype="exe"
fi

# Demander le mode de lancement avec un menu
choice=$(kdialog --menu "Choisissez le mode de lancement :" \
    "normal" "Lancement normal" \
    "fix" "Lancement avec fix gamepad")

# Vérifier si l'utilisateur a annulé
if [ $? -ne 0 ] || [ -z "$choice" ]; then
    echo "Aucun choix effectué, utilisation du lancement normal par défaut"
    choice="normal"
fi

# Générer le script selon le choix
output_sh="$onlypath/$onlyapp.sh"
echo "#!/bin/bash" > "$output_sh"

if [ "$filetype" = "wgp" ]; then
    # Script pour fichier .wgp
    cat >> "$output_sh" << WGPEOF
WGP_FILE="\$(realpath "$fullpath")"
WGP_NAME="$onlyapp"
MOUNT_BASE="/tmp/wgpackmount"
MOUNT_DIR="\$MOUNT_BASE/\$WGP_NAME"

# Créer le dossier de montage
mkdir -p "\$MOUNT_BASE"

# Vérifier si déjà monté
if mountpoint -q "\$MOUNT_DIR"; then
    echo "Erreur: \$WGP_NAME est déjà monté"
    exit 1
fi

# Vérifier que squashfuse est disponible
if ! command -v squashfuse &> /dev/null; then
    echo "Erreur: squashfuse n'est pas installé"
    echo "Installez-le avec: paru -S squashfuse"
    rmdir "\$MOUNT_BASE" 2>/dev/null
    exit 1
fi

# Créer et monter le squashfs
mkdir -p "\$MOUNT_DIR"
echo "Montage de \$WGP_FILE sur \$MOUNT_DIR..."
squashfuse -r "\$WGP_FILE" "\$MOUNT_DIR"

if [ \$? -ne 0 ]; then
    echo "Erreur lors du montage du squashfs"
    rmdir "\$MOUNT_DIR"
    exit 1
fi

# Fonction de nettoyage
cleanup() {
    echo "Démontage de \$WGP_NAME..."
    fusermount -u "\$MOUNT_DIR" 2>/dev/null
    rmdir "\$MOUNT_DIR" 2>/dev/null
}

# Nettoyer en cas d'interruption
trap cleanup EXIT

# Lire le fichier .launch pour connaître l'exécutable
LAUNCH_FILE="\$MOUNT_DIR/.launch"
if [ ! -f "\$LAUNCH_FILE" ]; then
    echo "Erreur: fichier .launch introuvable dans le pack"
    cleanup
    exit 1
fi

EXE_PATH=\$(cat "\$LAUNCH_FILE")
FULL_EXE_PATH="\$MOUNT_DIR/\$EXE_PATH"

if [ ! -f "\$FULL_EXE_PATH" ]; then
    echo "Erreur: exécutable introuvable: \$EXE_PATH"
    cleanup
    exit 1
fi

WGPEOF

    if [ "$choice" = "normal" ]; then
        cat >> "$output_sh" << WGPEOF
# Appliquer la modification du registre
sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg

# Lancer le jeu
echo "Lancement de \$WGP_NAME..."
/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "\$FULL_EXE_PATH"

# Nettoyage automatique (le trap EXIT le fera aussi)
cleanup
exit 0
WGPEOF
    else
        cat >> "$output_sh" << WGPEOF
# Désactiver DisableHidraw
sed -i 's/"DisableHidraw"=dword:00000001/"DisableHidraw"=dword:00000000/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg

# Lancer le jeu
echo "Lancement de \$WGP_NAME..."
/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "\$FULL_EXE_PATH"

# Attendre 2 secondes
sleep 2

# Réactiver DisableHidraw
sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg

# Nettoyage automatique (le trap EXIT le fera aussi)
cleanup
exit 0
WGPEOF
    fi

else
    # Script pour fichier .exe (mode classique)
    case "$choice" in
        "normal")
            echo "sed -i 's/\"DisableHidraw\"=dword:00000000/\"DisableHidraw\"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg" >> "$output_sh"
            echo "/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable \"$fullpath\"" >> "$output_sh"
            ;;
        "fix")
            echo "sed -i 's/\"DisableHidraw\"=dword:00000001/\"DisableHidraw\"=dword:00000000/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg" >> "$output_sh"
            echo "/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable \"$fullpath\" ;" >> "$output_sh"
            echo "sleep 2" >> "$output_sh"
            echo "sed -i 's/\"DisableHidraw\"=dword:00000000/\"DisableHidraw\"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg" >> "$output_sh"
            ;;
    esac
fi

chmod +x "$output_sh"
echo "Fichier créé : $output_sh"
