#!/usr/bin/bash
# recorder.sh - enregistreur d'écran du banc (remplace startstoprecord).
#
#   ./recorder.sh start | stop | toggle | status
#
# Variables : RECORD_DIR (défaut ~/Vidéos/Captures), RECORD_RES (1920x1080),
#             RECORD_FPS (60), RECORD_AUDIO (default_output, vide = pas d'audio)
#
# Pourquoi ces précautions (mesuré sur cette machine) :
#  - l'enregistreur tourne dans un flatpak : SIGINT sur le lanceur `flatpak run`
#    ne finalise RIEN (le fichier n'apparaît jamais). Il faut signaler le
#    binaire DANS le bac à sable -> killall -SIGINT gpu-screen-recorder ;
#  - le bac à sable n'écrit pas dans /tmp (permissions du flatpak) : la sortie
#    doit être sous $HOME ;
#  - on ne cherche jamais le processus avec `pgrep -f gpu-screen-recorder` :
#    ce motif matche le shell qui lance la commande (il s'est tué lui-même).
set -uo pipefail

STATE=/tmp/opencode/recorder.state
OUTDIR=${RECORD_DIR:-$HOME/Vidéos/Captures}
RES=${RECORD_RES:-1920x1080}
# comme l'ancien script : fps_limit de MangoHud.conf, sinon 60
if [[ -z "${RECORD_FPS:-}" ]]; then
    CONF=${MANGOHUD_CONF:-$HOME/.config/MangoHud/MangoHud.conf}
    FPS=$(grep -oP '^fps_limit=\K\d+' "$CONF" 2>/dev/null | head -1)
    [[ -z "$FPS" || "$FPS" -eq 0 ]] 2>/dev/null && FPS=60
else
    FPS=$RECORD_FPS
fi
AUDIO=${RECORD_AUDIO-default_output}
APP=com.dec05eba.gpu_screen_recorder

# le vrai binaire (cmdline commence par son nom) — jamais le shell appelant
en_cours() { pgrep -f '^gpu-screen-recorder ' > /dev/null 2>&1; }

fichier() { sed -n 's/^file=//p' "$STATE" 2>/dev/null; }

notify() { command -v notify-send > /dev/null && notify-send -a recorder -t 2500 "$@" || true; }

# Bip de départ : `sounds/beep.wav`, joué AVANT de lancer l'encodeur, puis un
# silence (BIP_PAUSE) avant le démarrage — la capture audio est le monitor du
# sink, donc un bip joué pendant l'enregistrement se retrouverait DANS le
# fichier (c'est pour ça qu'il avait été retiré). Même schéma que `replay.sh`.
# L'arrêt reste muet : la notification donne le chemin du fichier.
BIP=${BIP_SON:-$(dirname "$(readlink -f "$0")")/sounds/beep.wav}
BIP_PAUSE=${BIP_PAUSE:-0.35}

bip_depart() {
    [[ -f "$BIP" ]] || return 0
    command -v ffplay > /dev/null || return 0
    ffplay -nodisp -autoexit -loglevel quiet "$BIP" > /dev/null 2>&1
    sleep "$BIP_PAUSE"
}

start() {
    if en_cours; then
        echo "déjà en cours : $(fichier)"
        return 0
    fi
    mkdir -p "$OUTDIR"
    # le son d'abord, l'encodeur ensuite : dans l'autre ordre le bip est filmé
    bip_depart
    local f="$OUTDIR/Video_$(date +%Y-%m-%d_%H-%M-%S).mp4"
    local args=(-cursor no -w screen -s "$RES" -f "$FPS" -o "$f")
    [[ -n "$AUDIO" ]] && args+=(-a "$AUDIO")

    setsid flatpak run --command=gpu-screen-recorder "$APP" "${args[@]}" \
        > /tmp/opencode/recorder.log 2>&1 < /dev/null &
    printf 'pid=%s\nfile=%s\nstart=%s\n' "$!" "$f" "$(date +%s)" > "$STATE"

    # attend que l'encodeur soit réellement parti (ou échoue)
    for _ in $(seq 1 30); do
        en_cours && break
        sleep 0.1
    done
    if en_cours; then
        echo "enregistrement : $f"
    else
        echo "ECHEC: l'enregistreur n'a pas démarré"
        tail -3 /tmp/opencode/recorder.log 2>/dev/null
        rm -f "$STATE"
        return 1
    fi
}

stop() {
    local f; f=$(fichier)
    if ! en_cours; then
        echo "pas d'enregistrement en cours"
        rm -f "$STATE"
        return 0
    fi
    local t0; t0=$(sed -n 's/^start=//p' "$STATE" 2>/dev/null || echo 0)
    local now; now=$(date +%s)
    if (( now - t0 < 2 )); then
        echo "trop tôt (moins de 2 s : l'encodeur peut ne pas être prêt), ignoré"
        return 0
    fi

    killall -SIGINT -q gpu-screen-recorder 2>/dev/null
    for _ in $(seq 1 80); do
        en_cours || break
        sleep 0.1
    done
    if en_cours; then
        echo "l'enregistreur ne s'arrête pas (SIGINT ignoré)"
        return 1
    fi

    # finalisation : le fichier doit être complet et lisible
    local ok=0
    for _ in $(seq 1 30); do
        [[ -s "$f" ]] && ok=1 && break
        sleep 0.1
    done
    if (( ok )) && command -v ffprobe > /dev/null; then
        local d; d=$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$f" 2>/dev/null)
        echo "enregistré : $f (${d:-?} s, $(du -h "$f" | cut -f1))"
        notify "Enregistrement sauve dans $f"
    else
        echo "enregistrement arrêté, fichier: ${f:-?} (non vérifié)"
    fi
    rm -f "$STATE"
    touch /tmp/opencode/recorder.cooldown
    ( sleep 1.5; rm -f /tmp/opencode/recorder.cooldown ) &
}

# cooldown après un arrêt : évite un double déclenchement de la touche
if [[ -f /tmp/opencode/recorder.cooldown ]]; then
    echo "cooldown actif, ignoré"
    exit 0
fi

case "${1:-toggle}" in
    start)  start ;;
    stop)   stop ;;
    toggle) if en_cours; then stop; else start; fi ;;
    status)
        if en_cours; then echo "actif : $(fichier)"; else echo "inactif"; fi ;;
    *) echo "usage: $0 start|stop|toggle|status"; exit 2 ;;
esac
