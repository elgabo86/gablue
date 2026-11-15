#!/bin/bash

################################################################################
# mangohud.sh - Réglages de l'overlay MangoHud (communs à tous les lancements)
#
# Stockés dans ~/.local/share/gwine/options (même fichier que xbox/gamescope) :
#   mangohud=on|off                      (absent = on)
#   mangohud_layout=horizontal|rectangle (absent = on n'impose rien)
#   fps_limit=N                          (absent = on n'impose rien)
#
# Le ~/.config/MangoHud/MangoHud.conf du user n'est JAMAIS réécrit : les
# réglages partent en MANGOHUD_CONFIG au lancement, avec `read_cfg` en premier
# argument. Sans read_cfg, MangoHud ignore complètement les fichiers de config
# dès que MANGOHUD_CONFIG est défini (src/overlay_params.cpp : `if (!env ||
# read_cfg) parseConfigFile(...)`) — la limite fps, la position et les couleurs
# du user seraient perdues. Les valeurs de MANGOHUD_CONFIG sont réappliquées
# APRÈS le fichier, donc elles gagnent, clé par clé.
#
# Variables absentes du fichier d'options = aucune clé envoyée : ce que le user
# a réglé à la main (MangoJuice, GOverlay) continue de s'appliquer.
################################################################################

MANGOHUD_CONFIG_FILE="$GWINE_DIR/options"
MANGOHUD_FPS_LIMIT_MAX=480

# Valeur brute d'une clé du fichier d'options (vide si absente)
_mangohud_option() {
    local key="$1"
    [ -f "$MANGOHUD_CONFIG_FILE" ] || return 0
    grep "^${key}=" "$MANGOHUD_CONFIG_FILE" 2>/dev/null | head -n1 | cut -d'=' -f2- || true
}

# Écrit (ou remplace) une clé du fichier d'options
_mangohud_set_option() {
    local key="$1" value="$2" content=""
    ensure_dir "$GWINE_DIR"
    if [ -f "$MANGOHUD_CONFIG_FILE" ]; then
        content=$(grep -v "^${key}=" "$MANGOHUD_CONFIG_FILE" 2>/dev/null || true)
    fi
    if [ -n "$content" ]; then
        printf '%s\n' "$content" > "$MANGOHUD_CONFIG_FILE"
    else
        : > "$MANGOHUD_CONFIG_FILE"
    fi
    printf '%s=%s\n' "$key" "$value" >> "$MANGOHUD_CONFIG_FILE"
}

# on | off (défaut : on, comportement historique de gwine)
get_mangohud_enabled() {
    if [ "$(_mangohud_option mangohud)" = "off" ]; then
        echo "off"
    else
        echo "on"
    fi
}

# horizontal | rectangle | vide (vide = pas de contrainte, le fichier fait foi)
get_mangohud_layout() {
    local value
    value=$(_mangohud_option mangohud_layout)
    case "$value" in
        horizontal|rectangle) echo "$value" ;;
    esac
}

# N | vide (vide = pas de limite imposée par gwine)
get_mangohud_fps_limit() {
    local value
    value=$(_mangohud_option fps_limit)
    case "$value" in
        ''|*[!0-9]*) return 0 ;;
    esac
    echo "$value"
}

set_mangohud_enabled() {
    case "$1" in
        on|off) _mangohud_set_option mangohud "$1" ;;
        *)
            echo "Erreur: valeur --mangohud invalide '$1'. Utilisez 'on' ou 'off'." >&2
            return 1
            ;;
    esac
}

set_mangohud_layout() {
    case "$1" in
        horizontal|rectangle) _mangohud_set_option mangohud_layout "$1" ;;
        *)
            echo "Erreur: disposition MangoHud invalide '$1'. Utilisez 'horizontal' ou 'rectangle'." >&2
            return 1
            ;;
    esac
}

# set_mangohud_fps_limit <0..480> ; 0 = illimitée
set_mangohud_fps_limit() {
    case "$1" in
        ''|*[!0-9]*)
            echo "Erreur: limite fps invalide '$1'. Indiquez un nombre entier (0 = illimitée)." >&2
            return 1
            ;;
    esac
    if [ "$1" -gt "$MANGOHUD_FPS_LIMIT_MAX" ]; then
        echo "Erreur: limite fps '$1' hors bornes (0…$MANGOHUD_FPS_LIMIT_MAX)." >&2
        return 1
    fi
    _mangohud_set_option fps_limit "$1"
}

# Chaîne MANGOHUD_CONFIG à exporter (vide = aucune contrainte)
build_mangohud_config() {
    local -a parts=("read_cfg")
    local layout limit
    layout=$(get_mangohud_layout)
    limit=$(get_mangohud_fps_limit)

    [ -n "$limit" ] && parts+=("fps_limit=$limit")
    # `horizontal` sans valeur vaut 1 ; `horizontal=0` remet le bloc classique
    # même si le fichier du user porte `horizontal` (booléens lus par strtol).
    case "$layout" in
        horizontal) parts+=("horizontal") ;;
        rectangle)  parts+=("horizontal=0") ;;
    esac

    [ "${#parts[@]}" -le 1 ] && return 0

    local IFS=,
    echo "${parts[*]}"
}

# Applique les réglages stockés à l'environnement du lancement
apply_mangohud_settings() {
    # MANGOHUD=0 déjà posé (gamescope --mangoapp, variable de l'utilisateur) :
    # mangoapp dessine lui-même, l'overlay injecté doit rester coupé.
    if [ "${MANGOHUD:-}" = "0" ]; then
        return 0
    fi

    if [ "$(get_mangohud_enabled)" = "off" ]; then
        export MANGOHUD=0
        echo "MangoHud désactivé (option gwine)"
        return 0
    fi

    local config
    config=$(build_mangohud_config)
    if [ -n "$config" ]; then
        export MANGOHUD_CONFIG="$config"
    fi
}
