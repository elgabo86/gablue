#!/bin/bash

################################################################################
# paths.sh - Utilitaires de manipulation de chemins
################################################################################

transliterate() {
    local input="$1"
    echo "$input" | iconv -f UTF-8 -t ASCII//TRANSLIT | sed 's/[^a-zA-Z0-9_-]/_/g'
}

create_temp_path() {
    local path="$1"
    local temp_base="/tmp/game_launcher_$(date +%s)"
    local new_path="$temp_base"
    local current_path=""
    local IFS='/'

    read -ra segments <<< "$path"
    for segment in "${segments[@]}"; do
        if [ -n "$segment" ]; then
            current_path="$current_path/$segment"
            local clean_segment
            clean_segment=$(transliterate "$segment")
            new_path="$new_path/$clean_segment"
            mkdir -p "$new_path"
        fi
    done

    local real_path
    real_path="$(realpath "$path")"
    for item in "$real_path"/* "$real_path"/.*; do
        [[ "$(basename "$item")" == "." ]] && continue
        [[ "$(basename "$item")" == ".." ]] && continue
        [ -e "$item" ] || [ -L "$item" ] && ln -sf "$item" "$new_path/" 2>/dev/null
    done

    echo "$new_path"
}
