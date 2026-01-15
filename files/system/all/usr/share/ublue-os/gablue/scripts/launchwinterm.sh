#!/bin/bash

# Analyse des paramètres
fix_mode=false
fullpath=""

# Vérifier si le paramètre --fix est présent
if [ "$1" = "--fix" ]; then
    fix_mode=true
    fullpath="$2"
elif [ "$2" = "--fix" ]; then
    fullpath="$1"
    fix_mode=true
else
    fullpath="$1"
fi

# Vérifier si c'est un .wgp
if [[ "$fullpath" == *.wgp ]]; then
    # Appeler launchwin.sh avec le paramètre --fix si demandé
    if [ "$fix_mode" = true ]; then
        exec /usr/share/ublue-os/gablue/scripts/launchwin.sh "$fullpath" --fix
    else
        exec /usr/share/ublue-os/gablue/scripts/launchwin.sh "$fullpath"
    fi
fi

# Sinon, lancer avec bottles en mode terminal
dirpath=$(dirname "$fullpath")
filename=$(basename "$fullpath")

# Fonction pour translittérer les caractères accentués
transliterate() {
    local input="$1"
    echo "$input" | iconv -f UTF-8 -t ASCII//TRANSLIT | sed 's/[^a-zA-Z0-9_-]/_/g'
}

# Fonction pour créer un chemin temporaire sans accents
create_temp_path() {
    local path="$1"
    local temp_base="/tmp/game_launcher_$(date +%s)"
    local new_path="$temp_base"
    local current_path=""
    local IFS='/'
    read -ra segments <<< "$path"

    # Parcourir tous les segments du chemin
    for segment in "${segments[@]}"; do
        if [ -n "$segment" ]; then
            current_path="$current_path/$segment"
            clean_segment=$(transliterate "$segment")
            new_path="$new_path/$clean_segment"
            mkdir -p "$new_path"
        fi
    done

    # Créer un lien symbolique pour le contenu du dossier parent final
    ln -sf "$(realpath "$path")"/* "$new_path/"

    echo "$new_path"
}

# Vérifier si le chemin contient des accents
if echo "$dirpath" | grep -P '[^\x00-\x7F]' > /dev/null; then
    new_dirpath=$(create_temp_path "$dirpath")
    new_fullpath="$new_dirpath/$filename"
    temp_base=$(echo "$new_dirpath" | grep -o "/tmp/game_launcher_[0-9]*")
else
    new_fullpath="$fullpath"
    temp_base=""
fi

# Nettoyage du dossier temporaire en cas d'interruption
[ -n "$temp_base" ] && trap 'rm -rf "$temp_base"' EXIT

if [ "$fix_mode" = true ]; then
    # Mode fix: désactiver DisableHidraw
    sed -i 's/"DisableHidraw"=dword:00000001/"DisableHidraw"=dword:00000000/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg
else
    # Mode normal: activer DisableHidraw
    sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg
fi

# Lancer le jeu avec le chemin en mode terminal
/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$new_fullpath"

if [ "$fix_mode" = true ]; then
    # Réactiver DisableHidraw après le lancement en mode fix
    sleep 2
    sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg
fi

# Nettoyer le dossier temporaire si créé
[ -n "$temp_base" ] && rm -rf "$temp_base"
