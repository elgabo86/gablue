#!/usr/bin/bash
# Test de contrôle déterministe de l'overlay, sans manette physique :
# une manette virtuelle uinput émet des appuis, l'overlay doit réagir.
# Vérifie aussi que le grab evdev rend bien les autres lecteurs aveugles.
set -uo pipefail

HERE="$(dirname "$(readlink -f "$0")")"
EVG="$HERE/../evgrab"
BIN="$HERE/target/release/gpad"
LOG=/tmp/opencode/overlay-ctl.log
: > "$LOG"

pgrep -x vpad | xargs -r kill 2>/dev/null || true
pgrep -x gpad | xargs -r kill 2>/dev/null || true
sleep 0.3

# binaires à jour (vpad doit gérer SIGUSR1 = valider, SIGUSR2 = nav)
gcc -w -O2 -std=c11 -D_GNU_SOURCE -o "$EVG/vpad" "$EVG/vpad.c" || exit 1
gcc -w -O2 -std=c11 -D_GNU_SOURCE -o "$EVG/rdr" "$EVG/rdr.c" || exit 1

setsid "$EVG/vpad" > "$EVG/vpad.out" 2>/dev/null < /dev/null &
PAD=$!
for _ in $(seq 1 50); do grep -q '^PAD_EVENT=' "$EVG/vpad.out" && break; sleep 0.1; done
EV=$(sed -n 's/^PAD_EVENT=//p' "$EVG/vpad.out")
# l'ACL uaccess de udev arrive après la création du node
for _ in $(seq 1 50); do [[ -r "$EV" ]] && break; sleep 0.1; done
[[ -r "$EV" ]] || { echo "ECHEC: node $EV illisible"; exit 1; }
echo "manette virtuelle : $EV"

setsid "$BIN" --grab "$EV" --seconds 25 > "$LOG" 2>&1 < /dev/null &
OV=$!
for _ in $(seq 1 40); do grep -q '^GPAD_UP' "$LOG" && break; sleep 0.1; done
grep -q GRABBED "$LOG" || { echo "ECHEC: pas de grab"; cat "$LOG"; exit 1; }

# 3 appuis "bas" puis 1 "valider"
for _ in 1 2 3; do kill -USR2 "$PAD"; sleep 0.25; done
kill -USR1 "$PAD"; sleep 0.25

echo "--- journal overlay"
cat "$LOG"
echo "--- autres lecteurs evdev (doit être 0)"
"$EVG/rdr" "$EV" 1 EV_PENDANT_OVERLAY || true
echo "overlay encore ouvert : pid $OV (fermeture auto dans 25 s)"
