#!/usr/bin/bash
# vrr.sh - VRR (Adaptive Sync) **automatique** pour toutes les sorties qui le
# supportent, ou désactivé.
#
#   ./vrr.sh status    "actif" / "inactif" / "mixte"  (pour l'affichage)
#   ./vrr.sh get       "vrr=auto|never|mixte|indispo" (machine-lisible)
#   ./vrr.sh on        toutes les sorties capables -> Automatic
#   ./vrr.sh off       toutes les sorties capables -> Never
#
# Les valeurs acceptées par kscreen-doctor sont `automatic` / `always` /
# `never` : `auto` est ignoré **en silence** (rc=0, rien ne change) — mesuré.
#   ./vrr.sh toggle    bascule (auto -> never, sinon auto)
#
# Pourquoi `kscreen-doctor` : c'est le seul chemin qui applique la politique VRR
# à chaud, écran par écran (`output.<nom>.vrrpolicy.auto|always|never`), et qui
# l'écrit dans `~/.config/kwinoutputconfig.json` — éditer ce JSON à la main
# demanderait de faire relire la config par KWin, donc un rechargement d'écrans.
#
# Une sortie est « capable » quand `kscreen-doctor -o` lui donne une ligne
# `Vrr:` autre que `incapable`/`unsupported` (une TV HDMI sans Adaptive Sync
# n'en a pas).
set -uo pipefail

# noms des sorties qui gèrent le VRR, une par ligne
sorties() {
    kscreen-doctor -o 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' |
        awk '
        /^Output: [0-9]+/ {
            if (nom != "" && capable(vrr)) print nom;
            nom = $3; vrr = ""; next;
        }
        /^[ \t]*Vrr:/ { vrr = $2 }
        END { if (nom != "" && capable(vrr)) print nom }
        function capable(v) {
            return v != "" && tolower(v) != "incapable" && tolower(v) != "unsupported";
        }'
}

# état par sortie : "nom=Automatic" / "nom=Never" …
etats() {
    kscreen-doctor -o 2>/dev/null | sed 's/\x1b\[[0-9;]*m//g' |
        awk '
        /^Output: [0-9]+/ { nom = $3; next }
        /^[ \t]*Vrr:/ { if (nom != "") print nom "=" $2 }'
}

# kscreen applique de façon ASYNCHRONE : relire juste après renvoie encore
# l'ancien état (constaté — un second `toggle` repartait dans le même sens).
attendre() {
    local voulu=$1
    for _ in $(seq 1 15); do
        [[ "$(get)" == "vrr=$voulu" ]] && return 0
        sleep 0.1
    done
    return 1
}

appliquer() {
    local mode=$1 voulu=$2
    local args=() n
    for n in $(sorties); do
        args+=("output.$n.vrrpolicy.$mode")
    done
    if [[ ${#args[@]} -eq 0 ]]; then
        echo "VRR: aucune sortie compatible"
        return 1
    fi
    kscreen-doctor "${args[@]}" > /dev/null 2>&1
    local rc=$?
    attendre "$voulu" || true
    etat
    return $rc
}

etat() {
    local auto=0 total=0 ligne v
    while read -r ligne; do
        [[ -z "$ligne" ]] && continue
        v=${ligne#*=}
        total=$((total + 1))
        [[ "$v" == "Automatic" ]] && auto=$((auto + 1))
    done < <(etats)
    if [[ $total -eq 0 ]]; then
        echo "VRR: aucune sortie compatible"
        return 1
    fi
    if [[ $auto -eq $total ]]; then
        echo "VRR automatique"
    elif [[ $auto -eq 0 ]]; then
        echo "VRR desactive"
    else
        echo "VRR partiel ($auto/$total)"
    fi
}

get() {
    local auto=0 total=0 ligne v
    while read -r ligne; do
        [[ -z "$ligne" ]] && continue
        v=${ligne#*=}
        total=$((total + 1))
        [[ "$v" == "Automatic" ]] && auto=$((auto + 1))
    done < <(etats)
    if [[ $total -eq 0 ]]; then
        echo "vrr=indispo"
    elif [[ $auto -eq $total ]]; then
        echo "vrr=auto"
    elif [[ $auto -eq 0 ]]; then
        echo "vrr=never"
    else
        echo "vrr=mixte"
    fi
}

case "${1:-get}" in
    on)     appliquer automatic auto ;;
    off)    appliquer never never ;;
    toggle)
        if [[ "$(get)" == "vrr=auto" ]]; then
            appliquer never never
        else
            appliquer automatic auto
        fi
        ;;
    status) etat ;;
    get)    get ;;
    *) echo "usage: $0 on|off|toggle|status|get"; exit 2 ;;
esac
