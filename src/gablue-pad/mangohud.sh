#!/usr/bin/bash
# mangohud.sh - réglages MangoHud du banc (remplace changefps + showhidemango).
#
#   ./mangohud.sh fps       cycle 60 -> 90 -> 120 -> 165 -> sans limite
#   ./mangohud.sh toggle    affiché/par défaut -> horizontal -> masqué
#   ./mangohud.sh etat      affiche l'état courant
#
# Ces réglages étaient présentés comme « par jeu » : faux, l'ancien script
# éditait le fichier GLOBAL ~/.config/MangoHud/MangoHud.conf. C'est ce que
# fait celui-ci aussi (MANGOHUD_CONF permet de viser un autre fichier).
set -uo pipefail

CONF=${MANGOHUD_CONF:-$HOME/.config/MangoHud/MangoHud.conf}
mkdir -p "$(dirname "$CONF")"
[[ -f "$CONF" ]] || : > "$CONF"

cycle_fps() {
    local cycle=(60 90 120 165 0)
    local cur next="" found=false
    cur=$(grep -oP '^fps_limit=\K\d+' "$CONF" 2>/dev/null || echo 0)
    for f in "${cycle[@]}"; do
        if $found; then next=$f; break; fi
        [[ "$f" -eq "$cur" ]] && found=true
    done
    [[ -z "$next" ]] && next=${cycle[0]}

    sed -i '/^fps_limit=/d' "$CONF"
    if [[ "$next" -eq 0 ]]; then
        sed -i '/^show_fps_limit/d' "$CONF"
        echo "FPS : sans limite"
    else
        echo "fps_limit=$next" >> "$CONF"
        grep -q "^show_fps_limit" "$CONF" || echo "show_fps_limit" >> "$CONF"
        grep -q "^fps_limit_method=late" "$CONF" || echo "fps_limit_method=late" >> "$CONF"
        echo "FPS : $next"
    fi
}

# Trois états, identiques à l'ancien script (l'ancien affichait des messages
# inversés entre horizontal et vertical ; ici ils disent l'état réel).
toggle_vue() {
    if grep -q "^no_display" "$CONF"; then
        sed -i '/^no_display/d' "$CONF"
        sed -i '/^horizontal/d' "$CONF"
        echo "MangoHud : visible (disposition par défaut)"
    elif ! grep -q "^horizontal" "$CONF"; then
        echo "horizontal" >> "$CONF"
        echo "MangoHud : visible (horizontal)"
    else
        sed -i '/^horizontal/d' "$CONF"
        echo "no_display" >> "$CONF"
        echo "MangoHud : masqué"
    fi
}

# machine-lisible, pour le panneau
get_kv() {
    local vue="defaut" f
    grep -q "^no_display" "$CONF" && vue="masque"
    grep -q "^horizontal" "$CONF" && vue="horizontal"
    f=$(grep -oP '^fps_limit=\K\d+' "$CONF" 2>/dev/null || echo 0)
    echo "fps=${f:-0}"
    echo "vue=$vue"
}

set_fps() {
    local n=$1
    sed -i '/^fps_limit=/d' "$CONF"
    if [[ "$n" -eq 0 ]]; then
        sed -i '/^show_fps_limit/d' "$CONF"
        echo "FPS : sans limite"
    else
        echo "fps_limit=$n" >> "$CONF"
        grep -q "^show_fps_limit" "$CONF" || echo "show_fps_limit" >> "$CONF"
        grep -q "^fps_limit_method=late" "$CONF" || echo "fps_limit_method=late" >> "$CONF"
        echo "FPS : $n"
    fi
}

set_vue() {
    case "$1" in
        defaut)     sed -i '/^no_display/d' "$CONF"; sed -i '/^horizontal/d' "$CONF"
                    echo "MangoHud : visible (defaut)" ;;
        horizontal) sed -i '/^no_display/d' "$CONF"; grep -q "^horizontal" "$CONF" || echo "horizontal" >> "$CONF"
                    echo "MangoHud : visible (horizontal)" ;;
        masque)     sed -i '/^horizontal/d' "$CONF"; grep -q "^no_display" "$CONF" || echo "no_display" >> "$CONF"
                    echo "MangoHud : masque" ;;
        *) echo "vue inconnue: $1"; return 1 ;;
    esac
}

etat() {
    local vue="visible" fps="sans limite"
    grep -q "^no_display" "$CONF" && vue="masqué"
    grep -q "^horizontal" "$CONF" && vue="visible (horizontal)"
    local f; f=$(grep -oP '^fps_limit=\K\d+' "$CONF" 2>/dev/null || echo 0)
    [[ "$f" -ne 0 ]] && fps="$f"
    echo "$vue / FPS $fps"
}

case "${1:-etat}" in
    fps)     cycle_fps ;;
    toggle)  toggle_vue ;;
    etat)    etat ;;
    get)     get_kv ;;
    set-fps) set_fps "${2:-0}" ;;
    set-vue) set_vue "${2:-defaut}" ;;
    *) echo "usage: $0 fps|toggle|etat|get|set-fps N|set-vue defaut|horizontal|masque"; exit 2 ;;
esac
