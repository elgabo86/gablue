#!/bin/bash
fullpath="$1"

# Extraire le dossier parent et le nom du fichier
dirpath=$(dirname "$fullpath")
filename=$(basename "$fullpath")

# Fonction pour remplacer les accents dans un chemin
replace_accents_in_path() {
    local path="$1"
    local temp_base="/tmp/game_launcher_$(date +%s)"
    local new_path=""
    local current_path=""
    local remaining_path=""
    local IFS='/'
    read -ra segments <<< "$path"

    for segment in "${segments[@]}"; do
        if [ -n "$segment" ]; then
            current_path="$current_path/$segment"
            if echo "$segment" | grep -P '[^\x00-\x7F]' > /dev/null; then
                mkdir -p "$temp_base"
                ln -s "$(realpath "$current_path")" "$temp_base/no_accents"
                # Capturer le chemin restant après le dossier avec accents
                remaining_path=$(echo "$path" | sed "s|^$current_path/||" | sed "s|^$current_path$||")
                new_path="$temp_base/no_accents"
                break
            fi
        fi
    done

    # Ajouter le chemin restant (sous-dossiers) si présent
    [ -n "$remaining_path" ] && new_path="$new_path/$remaining_path"
    # Si aucun accent n'a été trouvé, retourner le chemin original
    [ -z "$new_path" ] && new_path="$path"
    echo "$new_path"
}

# Vérifier si le chemin contient des accents
if echo "$dirpath" | grep -P '[^\x00-\x7F]' > /dev/null; then
    new_dirpath=$(replace_accents_in_path "$dirpath")
    new_fullpath="$new_dirpath/$filename"
    temp_base=$(echo "$new_dirpath" | grep -o "/tmp/game_launcher_[0-9]*")
else
    new_fullpath="$fullpath"
    temp_base=""
fi

# Appliquer la modification du registre
sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg

# Lancer le jeu avec le chemin
/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$new_fullpath"

# Nettoyer le dossier temporaire si créé
[ -n "$temp_base" ] && rm -rf "$temp_base"
