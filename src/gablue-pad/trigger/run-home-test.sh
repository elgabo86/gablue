#!/usr/bin/bash
# Test du cycle complet Home -> overlay grabé -> retour, sans manette physique.
#   1. manette virtuelle uinput (vpad) émet un Home (SIGHUP)
#   2. gpad-trigger voit BTN_MODE et lance l'overlay avec --grab
#   3. pendant l'overlay, un autre lecteur evdev doit être aveugle
#   4. l'overlay ferme (--seconds), gpad-trigger reprend la veille
set -uo pipefail

HERE="$(dirname "$(readlink -f "$0")")"
EVG="$HERE/../evgrab"
# Cargo écrit HORS du dépôt (config cargo globale : ~/.cache/opencode) : le
# chemin de l'overlay se demande à cargo au lieu d'être supposé.
cible_cargo() {
    ( cd "$1" && cargo metadata --format-version 1 --no-deps 2>/dev/null |
        python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])' )
}
OVERLAY="$(cible_cargo "$HERE/../gpad")/release/gpad"
LOG=/tmp/opencode/home-test.log
LOG_OV=/tmp/opencode/home-test-overlay.log
: > "$LOG"; : > "$LOG_OV"

pgrep -x vpad | xargs -r kill 2>/dev/null || true
pgrep -x gpad-trigger | xargs -r kill 2>/dev/null || true
pgrep -x gpad | xargs -r kill 2>/dev/null || true
sleep 0.3

gcc -w -O2 -std=c11 -D_GNU_SOURCE -o "$EVG/vpad" "$EVG/vpad.c" || exit 1
gcc -w -O2 -std=c11 -D_GNU_SOURCE -o "$EVG/rdr" "$EVG/rdr.c" || exit 1
gcc -w -O2 -std=c11 -D_GNU_SOURCE -o "$HERE/gpad-trigger" "$HERE/gpad-trigger.c" || exit 1

setsid "$EVG/vpad" > "$EVG/vpad.out" 2>/dev/null < /dev/null &
PAD=$!
for _ in $(seq 1 50); do grep -q '^PAD_EVENT=' "$EVG/vpad.out" && break; sleep 0.1; done
EV=$(sed -n 's/^PAD_EVENT=//p' "$EVG/vpad.out")
for _ in $(seq 1 50); do [[ -r "$EV" ]] && break; sleep 0.1; done
[[ -r "$EV" ]] || { echo "ECHEC: node $EV illisible"; exit 1; }
echo "manette virtuelle : $EV"

setsid "$HERE/gpad-trigger" "$EV" "$OVERLAY" --seconds 8 > "$LOG" 2>&1 < /dev/null &
WATCH=$!
for _ in $(seq 1 40); do grep -q "en attente de Home" "$LOG" && break; sleep 0.1; done

# Home
kill -HUP "$PAD"
for _ in $(seq 1 40); do grep -q "HOME ->" "$LOG" && break; sleep 0.1; done

echo "--- gpad-trigger"; cat "$LOG"
echo "--- overlay"; cat "$LOG_OV"
echo "--- autres lecteurs evdev pendant l'overlay (0 attendu)"
"$EVG/rdr" "$EV" 1 EV_PENDANT || true

# fermeture (l'overlay a --seconds 8) puis reprise
for _ in $(seq 1 100); do grep -q "overlay terminé" "$LOG" && break; sleep 0.2; done
echo "--- après fermeture"; tail -2 "$LOG"

kill "$WATCH" 2>/dev/null || true
kill "$PAD" 2>/dev/null || true
true
