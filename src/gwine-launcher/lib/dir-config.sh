#!/bin/bash

################################################################################
# dir-config.sh - Gestion des répertoires bind pour le sandbox bubblewrap
#
# Ce module permet d'ajouter, supprimer, lister et réinitialiser les
# répertoires qui seront bindés dans le sandbox bubblewrap en plus des
# répertoires par défaut.
#
# Les chemins sont stockés dans ~/.config/gwine/bind-dirs.conf
################################################################################

# Fichier de configuration pour les répertoires bind
BIND_DIRS_CONFIG_FILE="${HOME}/.config/gwine/bind-dirs.conf"

# =============================================================================
# Fonctions utilitaires
# =============================================================================

# Normalise un chemin : convertit /var/home/ en /home/ et résout les ~
normalize_path() {
    local path="$1"
    
    # Expandre le ~ en $HOME
    if [[ "$path" =~ ^~ ]]; then
        path="${HOME}${path:1}"
    fi
    
    # Convertir /var/home/ en /home/ pour la compatibilité
    if [[ "$path" =~ ^/var/home/ ]]; then
        path="/home${path:9}"
    fi
    
    # Convertir en chemin absolu si relatif
    if [[ "$path" != /* ]]; then
        path="$(pwd)/$path"
    fi
    
    # Résoudre les .. et . et les liens symboliques
    path="$(realpath -m "$path" 2>/dev/null || echo "$path")"
    
    echo "$path"
}

# Vérifie si un chemin existe et est un répertoire
validate_directory() {
    local path="$1"
    
    if [ ! -e "$path" ]; then
        echo "Erreur: Le chemin n'existe pas: $path" >&2
        return 1
    fi
    
    if [ ! -d "$path" ]; then
        echo "Erreur: Le chemin n'est pas un répertoire: $path" >&2
        return 1
    fi
    
    return 0
}

# S'assure que le fichier de configuration existe
ensure_config_file() {
    local config_dir
    config_dir="$(dirname "$BIND_DIRS_CONFIG_FILE")"
    
    if [ ! -d "$config_dir" ]; then
        mkdir -p "$config_dir" 2>/dev/null || {
            echo "Erreur: Impossible de créer le répertoire de configuration: $config_dir" >&2
            return 1
        }
    fi
    
    if [ ! -f "$BIND_DIRS_CONFIG_FILE" ]; then
        touch "$BIND_DIRS_CONFIG_FILE" 2>/dev/null || {
            echo "Erreur: Impossible de créer le fichier de configuration: $BIND_DIRS_CONFIG_FILE" >&2
            return 1
        }
    fi
    
    return 0
}

# =============================================================================
# Commandes de gestion des répertoires
# =============================================================================

# Ajoute un répertoire à la liste
dir_config_add() {
    local raw_path="$1"
    
    if [ -z "$raw_path" ]; then
        echo "Usage: gwine --dir add <chemin>" >&2
        return 1
    fi
    
    local normalized_path
    normalized_path="$(normalize_path "$raw_path")"
    
    if ! validate_directory "$normalized_path"; then
        return 1
    fi
    
    ensure_config_file || return 1
    
    # Vérifier si le répertoire est déjà dans la liste
    if grep -Fxq "$normalized_path" "$BIND_DIRS_CONFIG_FILE" 2>/dev/null; then
        echo "Le répertoire est déjà dans la liste: $normalized_path"
        return 0
    fi
    
    # Vérifier les doublons potentiels (même répertoire via chemins différents)
    local existing_path
    existing_path="$(readlink -f "$normalized_path" 2>/dev/null)"
    if [ -n "$existing_path" ]; then
        while IFS= read -r line; do
            if [ -n "$line" ]; then
                local existing_normalized
                existing_normalized="$(readlink -f "$line" 2>/dev/null)"
                if [ "$existing_normalized" = "$existing_path" ]; then
                    echo "Le répertoire est déjà dans la liste (via un autre chemin): $line"
                    return 0
                fi
            fi
        done < "$BIND_DIRS_CONFIG_FILE"
    fi
    
    # Ajouter le répertoire
    echo "$normalized_path" >> "$BIND_DIRS_CONFIG_FILE"
    echo "Répertoire ajouté: $normalized_path"
    
    return 0
}

# Supprime un répertoire de la liste
dir_config_del() {
    local raw_path="$1"
    
    if [ -z "$raw_path" ]; then
        echo "Usage: gwine --dir del <chemin>" >&2
        return 1
    fi
    
    local normalized_path
    normalized_path="$(normalize_path "$raw_path")"
    
    if [ ! -f "$BIND_DIRS_CONFIG_FILE" ]; then
        echo "Aucun répertoire configuré."
        return 1
    fi
    
    # Vérifier si le fichier est vide
    if [ ! -s "$BIND_DIRS_CONFIG_FILE" ]; then
        echo "Aucun répertoire configuré."
        return 1
    fi
    
    # Chercher le répertoire exact ou partiel
    local found=false
    local matched_path=""
    
    # D'abord essayer une correspondance exacte
    if grep -Fxq "$normalized_path" "$BIND_DIRS_CONFIG_FILE" 2>/dev/null; then
        found=true
        matched_path="$normalized_path"
    else
        # Essayer de trouver par résolution de liens symboliques
        local target_real
        target_real="$(readlink -f "$normalized_path" 2>/dev/null)"
        if [ -n "$target_real" ]; then
            # Utiliser un file descriptor explicite
            exec 3< "$BIND_DIRS_CONFIG_FILE" || {
                echo "Erreur: Impossible de lire le fichier de configuration" >&2
                return 1
            }
            while IFS= read -r line <&3; do
                if [ -n "$line" ]; then
                    local line_real
                    line_real="$(readlink -f "$line" 2>/dev/null)"
                    if [ "$line_real" = "$target_real" ]; then
                        found=true
                        matched_path="$line"
                        break
                    fi
                fi
            done
            exec 3<&-
        fi
    fi
    
    if [ "$found" = false ]; then
        echo "Répertoire non trouvé dans la liste: $normalized_path"
        return 1
    fi
    
    # Supprimer le répertoire
    local temp_file
    temp_file="$(mktemp)"
    grep -Fxv "$matched_path" "$BIND_DIRS_CONFIG_FILE" > "$temp_file" || true
    mv "$temp_file" "$BIND_DIRS_CONFIG_FILE"
    
    echo "Répertoire supprimé: $matched_path"
    
    return 0
}

# Liste tous les répertoires configurés
dir_config_list() {
    if [ ! -f "$BIND_DIRS_CONFIG_FILE" ] || [ ! -s "$BIND_DIRS_CONFIG_FILE" ]; then
        echo "Aucun répertoire configuré."
        echo "Utilisez 'gwine --dir add <chemin>' pour ajouter un répertoire."
        return 0
    fi
    
    echo "Répertoires bindés dans le sandbox :"
    echo "====================================="
    
    local count=0
    # Utiliser un file descriptor explicite pour éviter de lire stdin
    exec 3< "$BIND_DIRS_CONFIG_FILE" || {
        echo "Erreur: Impossible de lire le fichier de configuration" >&2
        return 1
    }
    while IFS= read -r line <&3; do
        if [ -n "$line" ]; then
            ((count++))
            if [ -d "$line" ]; then
                echo "  [$count] $line"
            else
                echo "  [$count] $line (INEXISTANT)"
            fi
        fi
    done
    exec 3<&-
    
    echo "====================================="
    echo "Total: $count répertoire(s)"
    
    return 0
}

# Réinitialise la liste (supprime tout) avec confirmation
dir_config_reset() {
    if [ ! -f "$BIND_DIRS_CONFIG_FILE" ] || [ ! -s "$BIND_DIRS_CONFIG_FILE" ]; then
        echo "Aucun répertoire à supprimer."
        return 0
    fi
    
    # Afficher les répertoires qui seront supprimés
    echo "Les répertoires suivants seront supprimés de la liste :"
    echo "======================================================"
    dir_config_list | grep "^  \["
    echo ""
    
    # Demander confirmation
    echo -n "Voulez-vous vraiment supprimer tous les répertoires ? [o/N] "
    read -r response
    
    if [[ "$response" =~ ^[Oo]$ ]] || [[ "$response" =~ ^[Oo][Uu][Ii]$ ]]; then
        rm -f "$BIND_DIRS_CONFIG_FILE"
        touch "$BIND_DIRS_CONFIG_FILE"
        echo "Tous les répertoires ont été supprimés."
    else
        echo "Opération annulée."
    fi
    
    return 0
}

# =============================================================================
# Fonction pour le sandbox
# =============================================================================

# Récupère les chemins des répertoires personnalisés (un par ligne)
get_custom_bind_dirs_paths() {
    if [ ! -f "$BIND_DIRS_CONFIG_FILE" ] || [ ! -s "$BIND_DIRS_CONFIG_FILE" ]; then
        return 0
    fi
    
    # Utiliser un file descriptor explicite
    exec 3< "$BIND_DIRS_CONFIG_FILE" 2>/dev/null || return 0
    while IFS= read -r line <&3; do
        if [ -n "$line" ] && [ -d "$line" ]; then
            # Vérifier que le répertoire n'est pas déjà bindé par défaut
            if [[ "$line" == "$SAVES_REAL" ]] || [[ "$line" == "$SAVES_REAL/"* ]] || \
               [[ "$line" == "$EXTRA_REAL" ]] || [[ "$line" == "$EXTRA_REAL/"* ]] || \
               [[ "$line" == "$CACHE_DIR" ]] || [[ "$line" == "$CACHE_DIR/"* ]] || \
               [[ "$line" == "$GWINE_DIR" ]] || [[ "$line" == "$GWINE_DIR/"* ]] || \
               [[ "$line" == "$HOME_REAL/Windows" ]] || [[ "$line" == "$HOME_REAL/Windows/"* ]]; then
                continue
            fi
            
            echo "$line"
        fi
    done
    exec 3<&-
}

# Exporte les fonctions pour utilisation dans d'autres modules
export -f normalize_path
export -f validate_directory
export -f ensure_config_file
export -f dir_config_add
export -f dir_config_del
export -f dir_config_list
export -f dir_config_reset
export -f get_custom_bind_dirs_paths
