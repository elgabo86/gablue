#!/usr/bin/bash
# Qui lit la manette en ce moment ? (diagnostic du canal)
#
# Un grab evdev ne coupe QUE les lecteurs du node eventN. Les jeux qui lisent
# la DS4 via SDL hidapi ou Steam passent par /dev/hidrawN : ils continueront
# de recevoir la manette. Ce script nomme les processus concernés.
#
#   who-reads-pad.sh [node-event]
set -uo pipefail

EV="${1:-}"
if [[ -z "$EV" ]]; then
    for d in /sys/class/input/event*; do
        name=$(tr 'A-Z' 'a-z' < "$d/device/name" 2>/dev/null)
        case "$name" in
            *touchpad*|*touch\ pad*|*sensors*|*motion*) continue;;
            *wireless\ controller*|*dualsense*|*dualshock*|*gamepad*|*xbox*)
                EV="/dev/input/$(basename "$d")"; break;;
        esac
    done
fi
[[ -n "${EV:-}" ]] || { echo "aucune manette détectée"; exit 1; }

base=$(basename "$EV")
uniq=$(cat "/sys/class/input/$base/device/uniq" 2>/dev/null)
hidraw=""
for h in /sys/class/hidraw/hidraw*; do
    [[ -e "$h" ]] || continue
    if [[ -n "$uniq" ]] && grep -qx "HID_UNIQ=$uniq" "$h/device/uevent" 2>/dev/null; then
        hidraw="/dev/$(basename "$h")"
        break
    fi
done

echo "manette     : $EV  (uniq ${uniq:-?})"
echo "hidraw      : ${hidraw:-aucun}"
echo

holders() { # cible
    local target="$1" found=0 pid comm
    for d in /proc/[0-9]*/fd; do
        pid=${d#/proc/}; pid=${pid%/fd}
        [[ "$pid" == "$$" ]] && continue
        for f in "$d"/*; do
            [[ -L "$f" ]] || continue
            if [[ "$(readlink -f "$f" 2>/dev/null)" == "$target" ]]; then
                comm=$(cat "/proc/$pid/comm" 2>/dev/null)
                echo "  $comm (pid $pid)"
                found=1
                break
            fi
        done
    done
    ((found)) || echo "  aucun"
}

echo "lecteurs evdev $EV  -> COUPÉS par le grab :"
holders "$EV"
if [[ -n "$hidraw" ]]; then
    echo
    echo "lecteurs hidraw $hidraw -> NON coupés (le trou) :"
    holders "$hidraw"
fi
