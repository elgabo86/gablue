#!/usr/bin/bash
# run-gpad.sh - chaîne complète : Home -> panneau -> manettes coupées -> retour.
#
#   ./run-gpad.sh              panneau latéral (défaut)
#   ./run-gpad.sh --fullscreen panneau plein écran assombri
#
# Ce que ça fait :
#   1. inventorie les manettes (hidraw + js) ;
#   2. démarre le démon gpad-gate (une authentification PolicyKit) ;
#   3. lance gpad-trigger : appuie sur HOME -> l'overlay s'ouvre, envoie `open`
#      au démon (les manettes ne répondent plus nulle part) ; B le referme
#      et envoie `close`.
#
# Ctrl-C rend tout : le panneau se ferme, le gate se détache, le démon s'arrête.
set -uo pipefail

HERE="$(dirname "$(readlink -f "$0")")"
GATE_DIR="$HERE/gate"
# Cargo écrit HORS du dépôt (config cargo globale : ~/.cache/opencode) : le
# chemin de l'overlay se demande à cargo au lieu d'être supposé.
cible_cargo() {
    ( cd "$1" && cargo metadata --format-version 1 --no-deps 2>/dev/null |
        python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])' )
}
OVERLAY="$(cible_cargo "$HERE/gpad")/release/gpad"
TRIGGER="$HERE/trigger/gpad-trigger"
SOCK=/tmp/opencode/gpad-gate.sock
LOG=/tmp/opencode/gpad-gate.log

[[ -x "$OVERLAY" ]] || { echo "overlay non compilé : (cd gpad && cargo build --release)"; exit 1; }
[[ -x "$TRIGGER" ]] || { echo "gpad-trigger non compilé : (gcc -w -O2 -std=c11 -D_GNU_SOURCE -o trigger/gpad-trigger trigger/gpad-trigger.c)"; exit 1; }

propre() {
    [[ -n "${PW:-}" ]] && kill "$PW" 2>/dev/null
    python3 -c "
import socket
try:
    s=socket.socket(socket.AF_UNIX); s.settimeout(2); s.connect('$SOCK'); s.sendall(b'quit'); s.close()
except Exception:
    pass" 2>/dev/null
    sleep 0.4
    pgrep -x gpad-gate > /dev/null 2>&1 && pkexec pkill -x gpad-gate 2>/dev/null
    rm -f "$SOCK"
}
trap propre EXIT INT TERM

INV=$("$GATE_DIR/padnodes.sh")
[[ -n "$INV" ]] || { echo "aucune manette détectée"; exit 1; }

DA=(); OA=()
while read -r tag node; do
    [[ "$tag" == NODE ]] && { DA+=(--target "$node"); OA+=(--gate-target "$node"); }
done <<< "$INV"

echo "manettes :"
echo "$INV" | sed -n 's/^PAD /  - /p'

: > "$LOG"; rm -f "$SOCK"
pgrep -x gpad-gate > /dev/null 2>&1 && pkexec pkill -x gpad-gate 2>/dev/null
sleep 0.3

echo "démarrage du démon (PolicyKit)..."
pkexec "$GATE_DIR/gpad-gate" "${DA[@]}" --socket "$SOCK" --timeout 3600 > "$LOG" 2>&1 &
for _ in $(seq 1 450); do grep -q '^SOCKET=' "$LOG" && break; sleep 0.1; done
grep -q '^SOCKET=' "$LOG" || { echo "ECHEC démon"; cat "$LOG"; exit 1; }
echo "  $(grep -c '^ADD' "$LOG") nodes sous gate"

echo
echo "Appuie sur HOME sur la manette pour ouvrir le panneau."
echo "B (ou 'QUITTER L OVERLAY') pour le refermer. Ctrl-C ici pour tout arrêter."
echo

# PAD_NODE : force le node à écouter (banc de test avec manette virtuelle)
NODE_ARG=()
[[ -n "${PAD_NODE:-}" ]] && NODE_ARG=("$PAD_NODE")
RECORDER="$HERE/recorder.sh" SCREENSHOT="$HERE/screenshot.sh" \
    MANGOHUD="$HERE/mangohud.sh" VRR="$HERE/vrr.sh" BOOSTER="$HERE/booster.sh" KILLTHEMALL="${KILLTHEMALL:-$HERE/../../files/system/all/usr/share/ublue-os/gablue/scripts/killthemall}" FERMERAPPLIS="$HERE/scripts/fermer-applis" \
    SCRIPTS="$HERE/scripts" MOUSE="$HERE/mouse/gpad-mouse" \
    KBDNAV="$HERE/kbd/gpad-kbd" REPLAY="$HERE/replay.sh" INHIBIT="$HERE/inhibit/gpad-inhibit" \
    RUMBLE="$HERE/rumble/gpad-rumble" "$TRIGGER" "${NODE_ARG[@]}" "$OVERLAY" --gate "$SOCK" "${OA[@]}" "$@" &
PW=$!
wait $PW
