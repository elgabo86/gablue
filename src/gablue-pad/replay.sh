#!/usr/bin/bash
# replay.sh - buffer de replay de l'écran (gpad-rec, piloté par sa socket).
#
#   ./replay.sh start | stop | save | toggle | status
#
#   start   démarre le buffer (durée = REPLAY_SEC ou `replay_sec` de la config)
#   save    sauvegarde le buffer -> Replay_<date>.mp4, retour haptique seul
#   stop    arrête le buffer (rien n'est sauvegardé)
#   status  "actif 30s" ou "inactif"
#
# Plus de pgrep/killall : le démon tient l'état, la socket le dit. Le geste
# reste identique (Home+gauche, ou la ligne REPLAY du panneau).
set -uo pipefail

HERE="$(dirname "$(readlink -f "$0")")"
REC=${GPAD_REC:-/usr/bin/gpad-rec}
RT=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
SOCK=${GPAD_REC_SOCK:-$RT/gablue/gpad-rec.sock}
# Journal du démon lancé à la volée (banc, test) : dans le dossier d'exécution,
# jamais dans un /tmp qui n'existe pas sur un poste installé.
LOG=${GPAD_REC_LOG:-$RT/gablue/gpad-rec.log}
SEC=${REPLAY_SEC:-}
# Retour haptique de confirmation : la vibration ne passe ni par l'écran ni par
# l'audio, donc elle ne peut PAS se retrouver dans le clip. PAD_NODE = manette
# qui a déclenché (vide ou "-" = détection automatique).
RUMBLE=${RUMBLE:-$HERE/rumble/rumble}

sock() {
    timeout 20 python3 - "$SOCK" "$1" <<'PY'
import socket, sys
# pas de démon = pas une erreur : l'appelant décide (il le démarre, ou il dit
# « inactif »). Une trace Python sur stderr à chaque status serait du bruit.
try:
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(15)
    s.connect(sys.argv[1])
except OSError:
    sys.exit(1)
s.sendall(sys.argv[2].encode() + b"\n")
print(s.makefile().readline().strip())
PY
}

champ() { sed -n "s/.*\"$2\":\"\{0,1\}\([^\",}]*\).*/\1/p" <<< "$1"; }
bool_oui() { [[ "$1" == true ]]; }

daemon_vivant() { [[ -S "$SOCK" ]] || return 1; local r; r=$(sock '{"cmd":"status"}') || return 1; [[ "$r" == *'"ok":true'* ]]; }

assure_daemon() {
    daemon_vivant && return 0
    [[ -x "$REC" ]] || { echo "ECHEC: $REC absent (enregistreur non installé)"; return 1; }
    mkdir -p "$(dirname "$SOCK")"
    setsid "$REC" --daemon --socket "$SOCK" > "$LOG" 2>&1 < /dev/null &
    for _ in $(seq 1 50); do
        daemon_vivant && return 0
        sleep 0.1
    done
    echo "ECHEC: le démon gpad-rec n'a pas démarré (voir $LOG)"
    return 1
}

SON_FIN=${SON_FIN:-$HERE/sounds/stop.wav}
son_fin() {
    [[ "${GPAD_SANS_BIP:-0}" == 1 ]] && return 0
    [[ -f "$SON_FIN" ]] || return 0
    command -v ffplay > /dev/null || return 0
    ffplay -nodisp -autoexit -loglevel quiet "$SON_FIN" > /dev/null 2>&1
}

BIP=${BIP_SON:-$HERE/sounds/beep.wav}
BIP_PAUSE=${BIP_PAUSE:-0.35}
bip_depart() {
    # `GPAD_SANS_BIP=1` : démarrage silencieux (commande `grec`, entrée de
    # menu). Le bip n'a de sens que manette en main, quand on ne voit pas
    # l'écran — et il s'entendrait dans l'enregistrement.
    [[ "${GPAD_SANS_BIP:-0}" == 1 ]] && return 0
    [[ -f "$BIP" ]] || return 0
    command -v ffplay > /dev/null || return 0
    ffplay -nodisp -autoexit -loglevel quiet "$BIP" > /dev/null 2>&1
    sleep "$BIP_PAUSE"
}

start() {
    assure_daemon || return 1
    local r; r=$(sock '{"cmd":"status"}')
    bool_oui "$(champ "$r" replay)" && { echo "replay déjà actif ($(champ "$r" replay_sec)s)"; return 0; }
    bip_depart
    local cmd='{"cmd":"replay.start"}'
    [[ -n "$SEC" ]] && cmd="{\"cmd\":\"replay.start\",\"sec\":$SEC}"
    r=$(sock "$cmd")
    if bool_oui "$(champ "$r" replay)"; then
        echo "replay actif : $(champ "$r" replay_sec)s"
    else
        echo "ECHEC: le replay n'a pas démarré (voir $LOG)"
        return 1
    fi
}

# --- nom du jeu en métadonnée (sidecar `<fichier>.jeu`) ---------------------
# La galerie du launcher lit le nom du jeu dans ce fichier (et le logo pour la
# copie partageable). Deux sources, dans cet ordre : `running.txt` du launcher
# (`nom`, `logo`), puis celui du lanceur gwine/lgplaunch (`titre`, `nom`). Les
# PID servent de garde : un fichier qui traîne après un plantage ne doit pas
# étiqueter un clip avec le dernier jeu.
champ_fichier() { sed -n "s/^$2=//p" "$1" 2>/dev/null | tail -1; }
pids_vivants() {
    local p q=0
    for p in ${1//,/ }; do kill -0 "$p" 2>/dev/null && q=1; done
    [[ $q == 1 ]]
}
ecrire_sidecar() {
    local f=$1 titre="" logo=""
    local launcher="${XDG_CACHE_HOME:-$HOME/.cache}/glauncher/running.txt"
    if [[ -f "$launcher" ]] && pids_vivants "$(champ_fichier "$launcher" pids)"; then
        titre=$(champ_fichier "$launcher" nom)
        logo=$(champ_fichier "$launcher" logo)
    fi
    local session="$RT/gablue/running.txt"
    if [[ -z "$titre" && -f "$session" ]] && pids_vivants "$(champ_fichier "$session" pids)"; then
        titre=$(champ_fichier "$session" titre)
        [[ -n "$titre" ]] || titre=$(champ_fichier "$session" nom)
    fi
    [[ -n "$titre" ]] || return 0
    { printf 'nom=%s\n' "$titre"
      [[ -n "$logo" ]] && printf 'logo=%s\n' "$logo"
    } > "$f.jeu" 2>/dev/null || true
}

save() {
    local r; r=$(sock '{"cmd":"status"}') || { echo "ECHEC: démon arrêté"; return 1; }
    bool_oui "$(champ "$r" replay)" || { echo "ECHEC: replay inactif"; return 1; }
    r=$(sock '{"cmd":"replay.save"}')
    local f; f=$(champ "$r" file)
    if bool_oui "$(champ "$r" ok)" && [[ -n "$f" ]]; then
        # Seul retour : la vibration. Aucune notification, aucun son — tout ce
        # qui s'affiche ou se joue peut atterrir dans un clip.
        [[ -x "$RUMBLE" ]] && "$RUMBLE" "${PAD_NODE:--}" 140 60
        ecrire_sidecar "$f"
        echo "replay sauvegardé : $f"
    else
        echo "ECHEC: sauvegarde refusée ($(champ "$r" err))"
        return 1
    fi
}

stop() {
    local r; r=$(sock '{"cmd":"status"}') || { echo "replay déjà arrêté"; return 0; }
    bool_oui "$(champ "$r" replay)" || { echo "replay déjà arrêté"; return 0; }
    sock '{"cmd":"replay.stop"}' > /dev/null
    # le buffer est coupé : le son de fin ne peut plus y entrer
    son_fin
    echo "replay arrêté"
}

case "${1:-status}" in
    start)  start ;;
    save)   save ;;
    stop)   stop ;;
    toggle)
        r=$(sock '{"cmd":"status"}' 2>/dev/null) || r=""
        if bool_oui "$(champ "$r" replay)"; then stop; else start; fi ;;
    status)
        r=$(sock '{"cmd":"status"}' 2>/dev/null) || { echo "inactif"; exit 0; }
        if bool_oui "$(champ "$r" replay)"; then echo "actif $(champ "$r" replay_sec)s"; else echo "inactif"; fi ;;
    *) echo "usage: $0 start|stop|save|toggle|status"; exit 2 ;;
esac
