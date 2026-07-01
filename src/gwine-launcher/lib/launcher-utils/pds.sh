#!/bin/bash

################################################################################
# pds.sh - Configuration ProgramData Saves
################################################################################

setup_pds_symlink() {
    local exe_dir="$1"
    local PDS_FILE="$exe_dir/.pds"

    [ -f "$PDS_FILE" ] || return 0

    local game_name
    game_name=$(cat "$PDS_FILE" 2>/dev/null)
    [ -n "$game_name" ] || return 0

    echo "Configuration ProgramData pour: $game_name"

    local progdata_saves_dir="$WINDOWS_HOME/$USER/ProgramDataSaves/$game_name"
    local progdata_symlink="$WINEPREFIX/drive_c/ProgramData/$game_name"

    if [ ! -d "$progdata_saves_dir" ]; then
        mkdir -p "$progdata_saves_dir"
        echo "Dossier créé: $progdata_saves_dir"
    fi

    mkdir -p "$(dirname "$progdata_symlink")"

    if [ -e "$progdata_symlink" ]; then
        if [ -d "$progdata_symlink" ] && [ ! -L "$progdata_symlink" ]; then
            echo "Dossier existant dans ProgramData: $progdata_symlink (pas de modification)"
            return 0
        elif [ -L "$progdata_symlink" ]; then
            local current_target
            current_target=$(readlink "$progdata_symlink")
            if [ "$current_target" = "$progdata_saves_dir" ]; then
                echo "Symlink déjà existant et correct: $progdata_symlink -> $progdata_saves_dir"
                return 0
            else
                echo "Symlink existant mais incorrect, remplacement..."
                rm -f "$progdata_symlink"
            fi
        fi
    fi

    ln -s "$progdata_saves_dir" "$progdata_symlink"
    echo "Symlink créé: $progdata_symlink -> $progdata_saves_dir"
}
