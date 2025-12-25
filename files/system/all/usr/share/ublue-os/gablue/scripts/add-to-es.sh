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

# Demander le nom du .sh avec kdialog, défaut = nom du .exe
sh_name=$(kdialog --inputbox "Entrez le nom du fichier .sh" "$onlyapp")
if [ $? -ne 0 ] || [ -z "$sh_name" ]; then
    echo "Annulation par l'utilisateur, arrêt du script"
    exit 1
fi

# Demander si le jeu doit être catégorisé
category_choice=$(kdialog --yesno "Voulez-vous ajouter ce jeu à une catégorie ?" --yes-label "Oui" --no-label "Non")
if [ $? -eq 0 ]; then
    category=$(kdialog --inputbox "Entrez le nom de la catégorie" "")
    if [ $? -ne 0 ] || [ -z "$category" ]; then
        echo "Aucune catégorie spécifiée, le jeu sera placé dans le dossier principal"
        category=""
    fi
else
    echo "Le jeu sera placé dans le dossier principal"
    category=""
fi

# Déterminer le dossier de sortie pour le script dans le même répertoire que le .exe
script_sh="$onlypath/$sh_name.sh"

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
echo "#!/bin/bash" > "$script_sh"

if [ "$filetype" = "wgp" ]; then
    # Script pour fichier .wgp
    cat >> "$script_sh" << WGPEOF
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
        cat >> "$script_sh" << WGPEOF
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
        cat >> "$script_sh" << WGPEOF
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
            echo "sed -i 's/\"DisableHidraw\"=dword:00000000/\"DisableHidraw\"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg" >> "$script_sh"
            echo "/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable \"$fullpath\"" >> "$script_sh"
            ;;
        "fix")
            echo "sed -i 's/\"DisableHidraw\"=dword:00000001/\"DisableHidraw\"=dword:00000000/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg" >> "$script_sh"
            echo "/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable \"$fullpath\" ;" >> "$script_sh"
            echo "sleep 2" >> "$script_sh"
            echo "sed -i 's/\"DisableHidraw\"=dword:00000000/\"DisableHidraw\"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg" >> "$script_sh"
            ;;
    esac
fi

chmod +x "$script_sh"
echo "Fichier créé : $script_sh"

# Déterminer le dossier de sortie pour le lien symbolique dans Roms (insensible à la casse)
default_dir="$HOME/Roms/windows"
link_dir=""

# Chercher une variante existante de ~/Roms/windows
for dir in "$HOME"/[Rr][Oo][Mm][Ss]/[Ww][Ii][Nn][Dd][Oo][Ww][Ss]; do
    if [ -d "$dir" ]; then
        link_dir="$dir"
        break
    fi
done

# Si aucune variante n'existe, utiliser le défaut et créer si nécessaire
if [ -z "$link_dir" ]; then
    link_dir="$default_dir"
    mkdir -p "$link_dir"
fi

# Si une catégorie est spécifiée, créer les sous-dossiers
if [ -n "$category" ]; then
    link_dir="$link_dir/$category"
    mkdir -p "$link_dir"
fi

# Créer le lien symbolique dans le dossier Windows
link_sh="$link_dir/$sh_name.sh"
ln -sf "$script_sh" "$link_sh"
echo "Lien symbolique créé : $link_sh -> $script_sh"

# Déterminer le dossier pour ES-DE (insensible à la casse)
default_esde="$HOME/ES-DE/downloaded_media/windows/covers"
cover_dir=""

# Chercher une variante existante de ~/ES-DE
for esde in "$HOME"/[Ee][Ss]-[Dd][Ee]/downloaded_media/windows/covers; do
    if [ -d "$esde" ]; then
        cover_dir="$esde"
        break
    fi
done

# Si aucune variante n'existe, utiliser le défaut et créer si nécessaire
if [ -z "$cover_dir" ]; then
    cover_dir="$default_esde"
    mkdir -p "$cover_dir"
fi

# Si une catégorie est spécifiée, créer le sous-dossier correspondant pour les couvertures
if [ -n "$category" ]; then
    cover_dir="$cover_dir/$category"
    mkdir -p "$cover_dir"
fi

# Encoder le nom pour l'URL
encoded_name=$(echo "$sh_name" | sed 's/ /%20/g')
search_url="https://steamgrid.usebottles.com/api/search/$encoded_name"
response=$(curl -s "$search_url")

if [ -z "$response" ]; then
    echo "Erreur : impossible de contacter l'API pour $sh_name"
    exit 1
fi

# Extraire l'URL de l'image
image_url=$(echo "$response" | grep -o 'https://[^"]*\.\(jpg\|png\)' | head -n 1)
if [ -z "$image_url" ]; then
    echo "Aucune image trouvée pour $sh_name"
    exit 1
fi

# Déterminer l'extension et le fichier de sortie
ext=$(echo "$image_url" | grep -o '\.\(jpg\|png\)$')
output_cover="$cover_dir/$sh_name$ext"

# Télécharger l'image
curl -s "$image_url" -o "$output_cover"
if [ $? -eq 0 ]; then
    echo "Cover téléchargé : $output_cover"
else
    echo "Échec du téléchargement du cover pour $sh_name"
    rm -f "$output_cover"
    exit 1
fi

exit 0
