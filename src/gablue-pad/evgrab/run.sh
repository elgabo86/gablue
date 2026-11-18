#!/usr/bin/bash
# Banc de test : un EVIOCGRAB sur /dev/input/eventN coupe-t-il vraiment les
# autres lecteurs ? Et couvre-t-il aussi joydev (jsN) ?
#
# Matrice testée (manette virtuelle uinput, reproductible, sans hardware) :
#   A  grab sur eventN, rafale émise     -> doit RECEVOIR
#   B  simple lecture de eventN          -> doit NE RIEN recevoir (grab tenu)
#   C  simple lecture de jsN             -> doit RECEVOIR (joydev NON couvert)
#   D  lecture de eventN après relâche   -> doit RECEVOIR (contrôle)
#   E  second grab sur eventN            -> doit ÉCHOUER (EBUSY)
set -uo pipefail

DIR="$(dirname "$(readlink -f "$0")")"
BIN="$DIR"
pad_pid=""
grab_pid=""

cleanup() {
    [[ -n "$grab_pid" ]] && kill "$grab_pid" 2>/dev/null || true
    [[ -n "$pad_pid" ]] && kill "$pad_pid" 2>/dev/null || true
    wait 2>/dev/null || true
}
trap cleanup EXIT

gcc -w -O2 -std=c11 -D_GNU_SOURCE -o "$BIN/vpad" "$DIR/vpad.c"
gcc -w -O2 -std=c11 -D_GNU_SOURCE -o "$BIN/rdr" "$DIR/rdr.c"

# Instances précédentes (lancement interrompu) : plusieurs pads virtuels du
# même nom faussent la résolution de node -> on repart propre.
pgrep -x vpad | xargs -r kill 2>/dev/null || true
sleep 0.3

# rafale déclenchée pendant qu'un lecteur lit
burst_in() { ( sleep "$1"; kill -USR1 "$pad_pid" ) & }

wait_line() { # fichier motif max_decisecondes
    for _ in $(seq 1 "$3"); do
        grep -q "$2" "$1" 2>/dev/null && return 0
        sleep 0.1
    done
    return 1
}

"$BIN/vpad" > "$BIN/vpad.out" 2> "$BIN/vpad.err" &
pad_pid=$!
wait_line "$BIN/vpad.out" '^PAD_EVENT=' 50 || { echo "ECHEC : vpad muet"; cat "$BIN/vpad.err"; exit 1; }
EV=$(sed -n 's/^PAD_EVENT=//p' "$BIN/vpad.out")
JS=$(sed -n 's/^PAD_JS=//p' "$BIN/vpad.out")
echo "manette virtuelle : event=$EV js=$JS"

# l'ACL uaccess arrive ~200 ms après la création du node : attendre l'ouverture
for _ in $(seq 1 30); do [[ -r "$EV" ]] && break; sleep 0.1; done
[[ -r "$EV" ]] || { echo "ECHEC : $EV illisible (ACL uaccess non appliquée)"; exit 1; }

# A : grab tenu 7 s
"$BIN/rdr" -g "$EV" 7 "A_grab_${EV##*/}" > "$BIN/A.out" 2>&1 &
grab_pid=$!
wait_line "$BIN/A.out" 'GRAB_OK' 30 || { echo "ECHEC : grab impossible"; cat "$BIN/A.out"; exit 1; }
burst_in 0.3
sleep 1

# E : second grab concurrent -> EBUSY attendu
"$BIN/rdr" -g "$EV" 1 "E_second_grab" > "$BIN/E.out" 2>&1 || true

# B : lecture simple du node event pendant que A grabbe
burst_in 0.8
"$BIN/rdr" "$EV" 2 "B_event_sans_grab" > "$BIN/B.out" 2>&1 || true

# C : lecture du node joydev (non couvert par le grab evdev)
if [[ -n "$JS" && "$JS" != "(aucun)" ]]; then
    burst_in 0.8
    "$BIN/rdr" "$JS" 2 "C_js_sans_grab" > "$BIN/C.out" 2>&1 || true
else
    echo "C_js_sans_grab                (pas de node joydev)" > "$BIN/C.out"
fi

wait "$grab_pid" 2>/dev/null || true
grab_pid=""

# D : contrôle après relâche
burst_in 0.8
"$BIN/rdr" "$EV" 2 "D_apres_release" > "$BIN/D.out" 2>&1 || true

echo "--- résultats"
cat "$BIN/A.out" "$BIN/B.out" "$BIN/C.out" "$BIN/D.out" "$BIN/E.out"
