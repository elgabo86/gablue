#!/usr/bin/bash
# booster.sh - « optimiser au premier plan » : le booster cgroup de Plasma
# donne à la fenêtre active cpu.weight = 100 x la normale (10 000) et lui
# réserve la mémoire vidéo (dmem.low = capacité de chaque GPU), ou le laisse
# éteint. Rien d'autre : ni quota CPU, ni poids disque (mesuré le 24/11/2026,
# source amont pixelcluster/kcgroups tag booster-dmemcg-experimental-0.2).
#
#   ./booster.sh status   "actif" / "inactif"  (pour l'affichage)
#   ./booster.sh get      "booster=on|off"     (machine-lisible)
#   ./booster.sh on       active + relance le service
#   ./booster.sh off      désactive + arrête le service
#   ./booster.sh toggle   bascule
#
# Réglage persistant : `autostart` dans `~/.config/kcgroupsrc`, groupe
# `[Foreground Booster]` — **absent = activé** (c'est le défaut de Plasma, et
# c'est ce que lit Gablue Manager : les deux doivent lire pareil).
# Effet immédiat en session locale, aucun privilège : la clé s'écrit avec
# `kwriteconfig6` et le service *utilisateur* `plasma-foreground-booster`
# est relancé (actif) ou arrêté (inactif).
set -uo pipefail

CONFIG="${HOME:-/root}/.config/kcgroupsrc"

# Valeur courante : on / off.
valeur() {
    local v=""
    if [[ -f $CONFIG ]]; then
        v=$(awk '
            /^[ \t]*\[/ {
                dans = ($0 ~ /^[ \t]*\[Foreground Booster\]/)
                next
            }
            dans && /^[ \t]*autostart[ \t]*=/ {
                sub(/^[^=]*=[ \t]*/, "")
                print
                exit
            }' "$CONFIG")
    fi
    case "${v,,}" in
        false | 0 | no | off) echo "off" ;;
        *) echo "on" ;;
    esac
}

get() { echo "booster=$(valeur)"; }

statut() {
    if [[ "$(valeur)" == "on" ]]; then echo "actif"; else echo "inactif"; fi
}

appliquer() {
    local value=$1 action=$2
    kwriteconfig6 --file kcgroupsrc --group 'Foreground Booster' \
        --key autostart "$value" || return 1
    # `static` sans [Install] : on démarre/arrête à la main (le redémarrage de
    # session relit `autostart`). Service absent = réglage sans effet, pas une
    # erreur : on ne casse pas la bascule du panneau pour ça.
    systemctl --user "$action" plasma-foreground-booster.service > /dev/null 2>&1 || true
    statut
}

case "${1:-get}" in
    on) appliquer true restart ;;
    off) appliquer false stop ;;
    toggle)
        if [[ "$(valeur)" == "on" ]]; then
            appliquer false stop
        else
            appliquer true restart
        fi
        ;;
    status) statut ;;
    get) get ;;
    *) echo "usage: $0 on|off|toggle|status|get"; exit 2 ;;
esac
