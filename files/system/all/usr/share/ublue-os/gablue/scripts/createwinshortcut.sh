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

# Déterminer le script de lancement à utiliser
LAUNCH_SCRIPT="/usr/share/ublue-os/gablue/scripts/launchwin.sh"

# Générer le script selon le choix
output_sh="$onlypath/$onlyapp.sh"
echo "#!/bin/bash" > "$output_sh"

if [ "$choice" = "fix" ]; then
    echo "exec \"$LAUNCH_SCRIPT\" --fix \"$fullpath\"" >> "$output_sh"
else
    echo "exec \"$LAUNCH_SCRIPT\" \"$fullpath\"" >> "$output_sh"
fi

chmod +x "$output_sh"
echo "Fichier créé : $output_sh"
