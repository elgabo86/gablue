#!/bin/bash

################################################################################
# env.sh - Chargement des fichiers d'environnement
################################################################################

load_env_file() {
    local env_file="$1"
    
    [ -f "$env_file" ] || return 0
    
    local env_vars=""
    local line
    
    while IFS= read -r line || [ -n "$line" ]; do
        [[ -z "$line" || "$line" =~ ^[[:space:]]*# ]] && continue
        line="$(echo "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
        [[ "$line" =~ ^[[:space:]]*$ ]] && continue
        
        if [ -n "$env_vars" ]; then
            env_vars="$env_vars $line"
        else
            env_vars="$line"
        fi
    done < "$env_file"
    
    echo "$env_vars"
}

load_env_files() {
    local mount_dir="$1"
    local exe_dir="$2"
    
    local env_vars=""
    local exe_env="$exe_dir/.env"
    
    if [ -f "$exe_env" ]; then
        env_vars=$(load_env_file "$exe_env")
    fi
    
    echo "$env_vars"
}
