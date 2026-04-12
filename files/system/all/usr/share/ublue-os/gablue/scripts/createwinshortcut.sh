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

# Générer le script (gwine remplace launchwin.sh)
output_sh="$onlypath/$onlyapp.sh"
echo "#!/bin/bash" > "$output_sh"

GWINE_ARGS=""
if [ "$choice" = "fix" ]; then
    GWINE_ARGS="--fix"
fi
case "$xbox_choice" in
    all)        GWINE_ARGS="$GWINE_ARGS --xbox" ;;
    ds4)        GWINE_ARGS="$GWINE_ARGS --xbox-ds4" ;;
    dualsense)  GWINE_ARGS="$GWINE_ARGS --xbox-dualsense" ;;
esac

echo "exec /usr/bin/gwine $GWINE_ARGS \"$fullpath\"" >> "$output_sh"

chmod +x "$output_sh"
echo "Fichier créé : $output_sh"
