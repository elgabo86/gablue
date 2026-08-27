#!/bin/bash

# Ajoute un jeu à ES-DE (EmulationStation Desktop Edition) :
# - .exe/.wgp : crée un lanceur .sh (gwine) et un lien dans ~/Roms/windows
# - .lgp      : crée un lien symbolique direct dans ~/Roms/switch ou ~/Roms/desktop
#               (les systèmes switch et desktop d'ES-DE supportent nativement .lgp
#                via la commande launchlin.sh, pas besoin de lanceur .sh)
# Télécharge aussi la cover dans ~/ES-DE/downloaded_media/<section>/covers

# Vérifier si un chemin est fourni
if [ $# -eq 0 ]; then
    echo "Usage: $0 /chemin/vers/fichier.exe, .wgp ou .lgp"
    exit 1
fi

# Normaliser le chemin fourni
fullpath="$1"
onlypath=$(dirname "$fullpath")

# Déterminer le type de fichier et l'extension
case "$fullpath" in
    *.lgp)
        onlyapp=$(basename "$fullpath" .lgp)
        filetype="lgp"
        ;;
    *.wgp)
        onlyapp=$(basename "$fullpath" .wgp)
        filetype="wgp"
        ;;
    *)
        onlyapp=$(basename "$fullpath" .exe)
        filetype="exe"
        ;;
esac

# Demander le nom du jeu avec kdialog, défaut = nom du fichier
sh_name=$(kdialog --inputbox "Entrez le nom du jeu dans ES-DE" "$onlyapp")
if [ $? -ne 0 ] || [ -z "$sh_name" ]; then
    echo "Annulation par l'utilisateur, arrêt du script"
    exit 1
fi

# Section ES-DE de destination (windows pour .exe/.wgp)
# Pour les .lgp, demander la section car certains LGP sont des jeux/mods Switch
es_section="windows"
if [ "$filetype" = "lgp" ]; then
    lower_path=$(echo "$onlypath" | tr '[:upper:]' '[:lower:]')
    if [[ "$lower_path" == *switch* ]]; then
        # Le dossier source semble être un dossier Switch : proposer Switch en premier
        kdialog --yesno "Le dossier source semble être un dossier Switch.\n\nAjouter \"$sh_name\" dans la section Nintendo Switch ?" \
            --yes-label "Oui, section Switch" --no-label "Non, section Desktop"
        answer=$?
    else
        kdialog --yesno "\"$sh_name\" est-il un jeu Nintendo Switch ?" \
            --yes-label "Oui, section Switch" --no-label "Non, section Desktop"
        answer=$?
    fi
    case $answer in
        0) es_section="switch" ;;
        1) es_section="desktop" ;;
        *) echo "Annulation par l'utilisateur, arrêt du script"; exit 1 ;;
    esac
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

# Pour les .exe/.wgp : création du lanceur .sh
# (inutile pour les .lgp, lancés directement par launchlin.sh via le système ES-DE)
if [ "$filetype" != "lgp" ]; then
    # Déterminer le dossier de sortie pour le script
    # Pour les .wgp, utiliser un sous-dossier .es-wgp
    if [ "$filetype" = "wgp" ]; then
        onlypath="$onlypath/.es-wgp"
        mkdir -p "$onlypath"
    fi
    script_sh="$onlypath/$sh_name.sh"

    # Mode de lancement
    if [ "$filetype" = "wgp" ]; then
        # Pour les .wgp, gwine lit .fix/.xbox automatiquement depuis le pack
        choice="normal"
        xbox_choice="off"
    else
        # Pour les .exe, demander le mode fix
        choice=$(kdialog --menu "Choisissez le mode de lancement :" \
            "normal" "Lancement normal" \
            "fix" "Lancement avec fix gamepad")

        # Vérifier si l'utilisateur a annulé
        if [ $? -ne 0 ] || [ -z "$choice" ]; then
            echo "Aucun choix effectué, utilisation du lancement normal par défaut"
            choice="normal"
        fi

        # Demander le mode xbox
        xbox_choice=$(kdialog --menu "Mode Xbox (émulation manettes Sony en Xbox 360) :" \
            "off" "Désactivé" \
            "all" "Tous (DS4+DualSense)" \
            "ds4" "DualShock 4 uniquement" \
            "dualsense" "DualSense uniquement")

        if [ $? -ne 0 ] || [ -z "$xbox_choice" ]; then
            xbox_choice="off"
        fi
    fi

    # Générer le script selon le choix
    echo "#!/bin/bash" > "$script_sh"

    GWINE_ARGS=""
    if [ "$choice" = "fix" ]; then
        GWINE_ARGS="--fix"
    fi
    case "$xbox_choice" in
        all)        GWINE_ARGS="$GWINE_ARGS --xbox" ;;
        ds4)        GWINE_ARGS="$GWINE_ARGS --xbox-ds4" ;;
        dualsense)  GWINE_ARGS="$GWINE_ARGS --xbox-dualsense" ;;
    esac

    echo "exec /usr/bin/gwine $GWINE_ARGS \"$fullpath\"" >> "$script_sh"

    chmod +x "$script_sh"
    echo "Fichier créé : $script_sh"
fi

# Déterminer le dossier de sortie pour le lien symbolique dans Roms (insensible à la casse)
default_dir="$HOME/Roms/$es_section"
link_dir=""

# Chercher une variante existante de ~/Roms/<section>
for dir in "$HOME"/[Rr][Oo][Mm][Ss]/*/; do
    [ -d "$dir" ] || continue
    base_dirname=$(basename "$dir")
    if [ "${base_dirname,,}" = "$es_section" ]; then
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

# Créer le lien symbolique dans le dossier Roms
if [ "$filetype" = "lgp" ]; then
    # Pour les .lgp, lien direct vers le fichier (lancé par launchlin.sh dans ES-DE)
    link_game="$link_dir/$sh_name.lgp"
    ln -sf "$fullpath" "$link_game"
    echo "Lien symbolique créé : $link_game -> $fullpath"
else
    link_game="$link_dir/$sh_name.sh"
    ln -sf "$script_sh" "$link_game"
    echo "Lien symbolique créé : $link_game -> $script_sh"
fi

# Déterminer le dossier pour ES-DE (insensible à la casse)
default_esde="$HOME/ES-DE/downloaded_media/$es_section/covers"
cover_dir=""

# Chercher une variante existante de ~/ES-DE
for esde in "$HOME"/[Ee][Ss]-[Dd][Ee]; do
    if [ -d "$esde/downloaded_media/$es_section/covers" ]; then
        cover_dir="$esde/downloaded_media/$es_section/covers"
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
