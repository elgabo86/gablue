#!/bin/bash

# Vérifier si un chemin est fourni
if [ $# -eq 0 ]; then
    echo "Usage: $0 /chemin/vers/fichier.exe"
    exit 1
fi

# Normaliser le chemin fourni
fullpath="$1"
onlypath=$(dirname "$fullpath")
onlyapp=$(basename "$fullpath" .exe)

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

chmod +x "$output_sh"
echo "Fichier créé : $output_sh"
