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

# Déterminer le dossier de sortie pour le script
# Pour les .wgp, utiliser un sous-dossier .es-wgp
if [ "$filetype" = "wgp" ]; then
    onlypath="$onlypath/.es-wgp"
    mkdir -p "$onlypath"
fi
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

# Déterminer le script de lancement à utiliser
LAUNCH_SCRIPT="/usr/share/ublue-os/gablue/scripts/launchwin.sh"
if [ "$choice" = "fix" ]; then
    echo "exec \"$LAUNCH_SCRIPT\" --fix \"$fullpath\"" >> "$script_sh"
else
    echo "exec \"$LAUNCH_SCRIPT\" \"$fullpath\"" >> "$script_sh"
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
