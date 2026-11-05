# Charge les variables d'environnement depuis un fichier .env
# Format supporté: VAR=val (une par ligne ou séparées par espaces)
# Usage: load_env_file <chemin_fichier>
load_env_file() {
    local env_file="$1"
    
    [ -f "$env_file" ] || return 0
    
    local env_vars=""
    local line
    
    while IFS= read -r line || [ -n "$line" ]; do
        # Ignorer les lignes vides et les commentaires
        [[ -z "$line" || "$line" =~ ^[[:space:]]*# ]] && continue
        
        # Supprimer les espaces/tabs autour
        line="$(echo "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
        
        # Ignorer si pas une affectation valide
        [[ "$line" =~ ^[[:space:]]*$ ]] && continue
        
        # Exporter la variable
        export "$line"
    done < "$env_file"
}

# Charge les fichiers .env (racine + dossier exe) et exporte les variables
# Usage: load_env_files <mount_dir> <exe_dir>
load_env_files() {
    local mount_dir="$1"
    local exe_dir="$2"
    
    local root_env="$mount_dir/.env"
    local exe_env="$exe_dir/.env"
    
    # Charger .env de la racine
    if [ -f "$root_env" ]; then
        load_env_file "$root_env"
    fi
    
    # Charger .env du dossier de l'exe (si différent de la racine)
    if [ -f "$exe_env" ] && [ "$exe_dir" != "$mount_dir" ]; then
        load_env_file "$exe_env"
    fi
}

