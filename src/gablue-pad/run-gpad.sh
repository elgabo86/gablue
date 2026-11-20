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
TRIGGER="$HERE/gpad-trigger"
# anciennes copies compilées à la main dans trigger/ : on les accepte encore
[[ -x "$TRIGGER" ]] || TRIGGER="$HERE/trigger/gpad-trigger"
SOCK=/run/gablue-pad/gate.sock
LOG=/tmp/opencode/gpad-gate.log
# Le démon du gate tourne déjà (service système root, `--timeout 0`) : on s'en
# sert au lieu d'en lancer un second — pas de PolicyKit, pas de clang, et c'est
# exactement la gate de production. Repli : démon de banc (binaire à compiler,
# PKexec) si le socket système n'est pas accessible.
GATE_SYSTEME=0
[[ -w "$SOCK" ]] && GATE_SYSTEME=1
# Le service de session lance déjà le VRAI déclencheur (`/usr/bin/gpad-trigger`) :
# deux déclencheurs sur la même manette = deux overlays qui se disputent le grab.
# On l'arrête le temps du banc et on le relance à la sortie.
SERVICE=gablue-pad.service
SERVICE_ETAT="$(systemctl --user is-active "$SERVICE" 2>/dev/null || true)"

[[ -x "$OVERLAY" ]] || { echo "overlay non compilé : (cd gpad && cargo build --release)"; exit 1; }
[[ -x "$TRIGGER" ]] || { echo "gpad-trigger non compilé : (make gpad-trigger)"; exit 1; }

propre() {
    [[ -n "${PW:-}" ]] && kill "$PW" 2>/dev/null
    if [[ "${GATE_SYSTEME:-0}" == 0 ]]; then
        python3 -c "
import socket
try:
    s=socket.socket(socket.AF_UNIX); s.settimeout(2); s.connect('$SOCK'); s.sendall(b'quit'); s.close()
except Exception:
    pass" 2>/dev/null
        sleep 0.4
        pgrep -x gpad-gate > /dev/null 2>&1 && pkexec pkill -x gpad-gate 2>/dev/null
        rm -f "$SOCK"
    fi
    [[ "${SERVICE_ETAT:-}" == active ]] && systemctl --user start "$SERVICE" >/dev/null 2>&1
}
trap propre EXIT INT TERM

if [[ "$SERVICE_ETAT" == active ]]; then
    echo "service $SERVICE arrêté pendant le banc (relancé à la sortie)"
    systemctl --user stop "$SERVICE"
fi

INV=$("$GATE_DIR/padnodes.sh")
[[ -n "$INV" ]] || { echo "aucune manette détectée"; exit 1; }

DA=(); OA=()
while read -r tag node; do
    [[ "$tag" == NODE ]] && { DA+=(--target "$node"); OA+=(--gate-target "$node"); }
done <<< "$INV"

echo "manettes :"
echo "$INV" | sed -n 's/^PAD /  - /p'

if [[ "$GATE_SYSTEME" == 1 ]]; then
    echo "gate : démon système ($SOCK)"
else
    : > "$LOG"; rm -f "$SOCK"
    pgrep -x gpad-gate > /dev/null 2>&1 && pkexec pkill -x gpad-gate 2>/dev/null
    sleep 0.3

    echo "démarrage du démon (PolicyKit)..."
    [[ -x "$GATE_DIR/gpad-gate" ]] || {
        echo "ECHEC : $GATE_DIR/gpad-gate absent (compilé en conteneur) et pas de"
        echo "        socket système $SOCK accessible."
        exit 1
    }
    pkexec "$GATE_DIR/gpad-gate" "${DA[@]}" --socket "$SOCK" --timeout 3600 > "$LOG" 2>&1 &
    for _ in $(seq 1 450); do grep -q '^SOCKET=' "$LOG" && break; sleep 0.1; done
    grep -q '^SOCKET=' "$LOG" || { echo "ECHEC démon"; cat "$LOG"; exit 1; }
    echo "  $(grep -c '^ADD' "$LOG") nodes sous gate"
fi

echo
echo "Appuie sur HOME sur la manette pour ouvrir le panneau."
echo "Overlay résident : il reste lancé, chaque Home l'ouvre sans exec (~10 ms"
echo "jusqu'au premier pixel) ; petit délai au tout premier lancement du script."
echo "B (ou 'QUITTER L OVERLAY') pour le refermer. Ctrl-C ici pour tout arrêter."
echo

# PAD_NODE : force le node à écouter (banc de test avec manette virtuelle)
NODE_ARG=()
[[ -n "${PAD_NODE:-}" ]] && NODE_ARG=("$PAD_NODE")

# Les binaires C sortent à la racine par `make` ; on accepte encore l'ancienne
# copie dans le sous-dossier. Un chemin absent casse le helper correspondant en
# silence (l'inhibition, par ex., était relancée en boucle et inondait le
# terminal) : on vérifie donc que chaque chemin existe.
bin_local() {
    if [[ -x "$HERE/$1" ]]; then
        echo "$HERE/$1"
    else
        echo "$HERE/$2/$1"
    fi
}
MOUSE="$(bin_local gpad-mouse mouse)"
INHIBIT="$(bin_local gpad-inhibit inhibit)"
RUMBLE="$(bin_local gpad-rumble rumble)"
for b in "$MOUSE" "$INHIBIT" "$RUMBLE"; do
    [[ -x "$b" ]] || echo "note : $b absent (make) — le geste correspondant échouera"
done

RECORDER="$HERE/recorder.sh" SCREENSHOT="$HERE/screenshot.sh" \
    MANGOHUD="$HERE/mangohud.sh" VRR="$HERE/vrr.sh" BOOSTER="$HERE/booster.sh" KILLTHEMALL="${KILLTHEMALL:-$HERE/../../files/system/all/usr/share/ublue-os/gablue/scripts/killthemall}" FERMERAPPLIS="$HERE/scripts/fermer-applis" \
    SCRIPTS="$HERE/scripts" MOUSE="$MOUSE" \
    REPLAY="$HERE/replay.sh" INHIBIT="$INHIBIT" \
    RUMBLE="$RUMBLE" "$TRIGGER" "${NODE_ARG[@]}" "$OVERLAY" --gate "$SOCK" "${OA[@]}" "$@" &
PW=$!
wait $PW
