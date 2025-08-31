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
    local IFS='/'
    read -ra segments <<< "$path"

    for segment in "${segments[@]}"; do
        if [ -n "$segment" ]; then
            current_path="$current_path/$segment"
            if echo "$segment" | grep -P '[^\x00-\x7F]' > /dev/null; then
                mkdir -p "$temp_base"
                ln -s "$(realpath "$current_path")" "$temp_base/no_accents"
                new_path="$temp_base/no_accents"
                break
            fi
        fi
    done

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

# Désactiver DisableHidraw
sed -i 's/"DisableHidraw"=dword:00000001/"DisableHidraw"=dword:00000000/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg

# Lancer le jeu avec le chemin
/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$new_fullpath"

# Attendre 2 secondes
sleep 2

# Réactiver DisableHidraw
sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg

# Nettoyer le dossier temporaire si créé
[ -n "$temp_base" ] && rm -rf "$temp_base"
