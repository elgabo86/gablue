#!/usr/bin/bash
# recorder.sh - enregistrement d'écran (gpad-rec, piloté par sa socket).
#
#   ./recorder.sh start | stop | toggle | status | pending
#
# Variables : RECORD_DIR (défaut ~/Vidéos/Captures), GPAD_REC (binaire du
#             démon, défaut /usr/bin/gpad-rec), GPAD_REC_SOCK (socket).
#
# Ce script ne fait PLUS de pgrep/killall : l'état vit dans le démon, la
# socket est la seule interface (elle répond `recording`, `file`, `t`).
# Les réglages (codec, qualité, résolution, fps, curseur) sont dans
# ~/.config/gablue/recorder.conf, écrits par l'overlay et Gablue Manager.
set -uo pipefail

REC=${GPAD_REC:-/usr/bin/gpad-rec}
RT=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
SOCK=${GPAD_REC_SOCK:-$RT/gablue/gpad-rec.sock}
# Journal du démon lancé à la volée (banc, test) : dans le dossier d'exécution,
# jamais dans un /tmp qui n'existe pas sur un poste installé.
LOG=${GPAD_REC_LOG:-$RT/gablue/gpad-rec.log}

# Aucune notification à l'écran : le buffer de replay tourne pendant
# l'enregistrement, une bulle « sauvegardé » se retrouverait DANS le clip
# suivant. Le retour de fin est SONORE (`stop.wav`), pas haptique : la
# vibration est le signe de la sauvegarde d'un replay, l'utiliser aussi pour
# l'arrêt d'un enregistrement était ambigu (constaté le 21/11/2026). Le son
# part après la finalisation du mp4, donc jamais dans le fichier.
SON_FIN=${SON_FIN:-$(dirname "$(readlink -f "$0")")/sounds/stop.wav}
son_fin() {
    [[ "${GPAD_SANS_BIP:-0}" == 1 ]] && return 0
    [[ -f "$SON_FIN" ]] || return 0
    command -v ffplay > /dev/null || return 0
    ffplay -nodisp -autoexit -loglevel quiet "$SON_FIN" > /dev/null 2>&1
}

# Une ligne JSON par message, réponse sur la même connexion.
sock() {
    timeout 15 python3 - "$SOCK" "$1" <<'PY'
import socket, sys
# pas de démon = pas une erreur : l'appelant décide (il le démarre, ou il dit
# « inactif »). Une trace Python sur stderr à chaque status serait du bruit.
try:
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(12)
    s.connect(sys.argv[1])
except OSError:
    sys.exit(1)
s.sendall(sys.argv[2].encode() + b"\n")
print(s.makefile().readline().strip())
PY
}

# Champ d'une réponse JSON (plat, sans échappement exotique : le protocole
# n'écrit que des booléens, des nombres et un chemin).
champ() { sed -n "s/.*\"$2\":\"\{0,1\}\([^\",}]*\).*/\1/p" <<< "$1"; }

daemon_vivant() { [[ -S "$SOCK" ]] || return 1; local r; r=$(sock '{"cmd":"status"}') || return 1; [[ "$r" == *'"ok":true'* ]]; }

# Le démon est un service de session ; s'il n'est pas là (banc, test), on le
# démarre ici. `--daemon` : socket ouverte, aucun enregistrement en cours.
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
    tail -3 "$LOG" 2>/dev/null
    return 1
}

# Bip de départ : joué AVANT le démarrage, puis un silence — la capture audio
# est le monitor du sink, un bip pendant l'enregistrement se retrouverait DANS
# le fichier.
BIP=${BIP_SON:-$(dirname "$(readlink -f "$0")")/sounds/beep.wav}
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
    local r; r=$(sock '{"cmd":"status"}') || { assure_daemon || return 1; r=$(sock '{"cmd":"status"}'); }
    [[ "$(champ "$r" recording)" == true ]] && { echo "déjà en cours : $(champ "$r" file)"; return 0; }
    bip_depart
    r=$(sock '{"cmd":"record.start"}')
    sleep 0.4
    r=$(sock '{"cmd":"status"}')
    if [[ "$(champ "$r" recording)" == true ]]; then
        echo "enregistrement : $(champ "$r" file)"
    else
        echo "ECHEC: l'enregistreur n'a pas démarré (voir $LOG)"
        return 1
    fi
}

stop() {
    local r; r=$(sock '{"cmd":"status"}') || { echo "pas d'enregistrement en cours"; return 0; }
    [[ "$(champ "$r" recording)" == true ]] || { echo "pas d'enregistrement en cours"; return 0; }
    local f; f=$(champ "$r" file)
    # moins de 2 s : l'encodeur n'a peut-être pas encore d'image (double appui)
    if awk -v t="$(champ "$r" t)" 'BEGIN { exit !(t < 2) }'; then
        echo "trop tôt (moins de 2 s), ignoré"
        return 0
    fi
    sock '{"cmd":"record.stop"}' > /dev/null
    # le démon finalise (moov + faststart) avant de rendre la main : la réponse
    # de `status` suivante ne voit plus `recording`
    for _ in $(seq 1 80); do
        r=$(sock '{"cmd":"status"}')
        [[ "$(champ "$r" recording)" == true ]] || break
        sleep 0.1
    done
    local d="?"
    if [[ -n "$f" ]] && command -v ffprobe > /dev/null; then
        d=$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$f" 2>/dev/null)
        echo "enregistré : $f (${d:-?} s, $(du -h "$f" 2>/dev/null | cut -f1))"
        son_fin
    else
        echo "enregistrement arrêté, fichier: ${f:-?} (non vérifié)"
    fi
}

case "${1:-toggle}" in
    start)  start ;;
    stop)   stop ;;
    toggle)
        r=$(sock '{"cmd":"status"}' 2>/dev/null) || r=""
        if [[ "$(champ "$r" recording)" == true ]]; then stop; else start; fi ;;
    status)
        r=$(sock '{"cmd":"status"}' 2>/dev/null) || { echo "inactif (démon arrêté)"; exit 0; }
        if [[ "$(champ "$r" recording)" == true ]]; then
            echo "actif : $(champ "$r" file) ($(champ "$r" t | cut -d. -f1) s)"
        else echo "inactif"; fi ;;
    # un réglage écrit attend la fermeture du mp4 en cours pour s'appliquer
    pending)
        r=$(sock '{"cmd":"status"}' 2>/dev/null) || { echo "applique"; exit 0; }
        [[ "$(champ "$r" pending)" == true ]] && echo "en-attente" || echo "applique" ;;
    *) echo "usage: $0 start|stop|toggle|status"; exit 2 ;;
esac
