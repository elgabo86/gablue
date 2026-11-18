#!/usr/bin/bash
# replay.sh - buffer de replay de l'écran (gpu-screen-recorder -r).
#
#   ./replay.sh start | stop | save | toggle | status
#
#   start   démarre un buffer de REPLAY_SEC secondes (défaut 30), qui tourne
#           en continu sans écrire de fichier
#   save    sauvegarde le buffer -> Replay_<date>.mp4 dans le dossier de sortie
#   stop    arrête le buffer (rien n'est sauvegardé)
#   toggle  start si arrêté, stop sinon
#   status  "actif <sec>s" ou "inactif"
#
# Pourquoi ces précautions (mesuré sur cette machine) :
#  - le binaire tourne dans un flatpak : signaler le LANCEUR `flatpak run` ne
#    fait rien, il faut lire le binaire DANS le bac à sable -> killall ;
#  - jamais `pgrep -f gpu-screen-recorder` : le motif matche le shell appelant ;
#  - le bac à sable n'écrit pas dans /tmp -> sortie sous $HOME.
set -uo pipefail

APP=com.dec05eba.gpu_screen_recorder
# gpu-screen-recorder ÉCRIT le chemin du replay sauvegardé sur stdout
# (documenté) : plus fiable que de guetter un fichier — le mtime du fichier est
# celui du début du buffer, et la sauvegarde peut dépasser l'attente.
LOG=/tmp/opencode/replay.log
# Retour haptique de confirmation : la vibration ne passe ni par l'écran ni par
# l'audio, donc elle ne peut PAS se retrouver dans le clip. PAD_NODE = manette
# qui a déclenché (vide ou "-" = détection automatique).
RUMBLE=${RUMBLE:-$(dirname "$(readlink -f "$0")")/rumble/rumble}
OUTDIR=${RECORD_DIR:-$HOME/Vidéos/Captures}
SEC=${REPLAY_SEC:-30}
RES=${RECORD_RES:-1920x1080}
FPS=${REPLAY_FPS:-60}
AUDIO=${REPLAY_AUDIO-default_output}

# le binaire (cmdline commençant par son nom), jamais le shell appelant
en_cours() { pgrep -f '^gpu-screen-recorder ' > /dev/null 2>&1; }
# le nôtre : un buffer de replay porte forcément -r <secondes>
replay_actif() { pgrep -f '^gpu-screen-recorder .*-r [0-9]+' > /dev/null 2>&1; }

# Bip de départ : celui de l'enregistrement (`sounds/beep.wav`), joué AVANT le
# lancement du buffer. Il faut donc un silence entre les deux : le monitor du
# sink garde la queue du son quelques centaines de ms, et le buffer qui démarre
# aussitôt l'enregistrerait (le bip se retrouverait dans chaque replay).
BIP=${BIP_SON:-$(dirname "$(readlink -f "$0")")/sounds/beep.wav}
BIP_PAUSE=${BIP_PAUSE:-0.35}

bip_depart() {
    [[ -f "$BIP" ]] || return 0
    command -v ffplay > /dev/null || return 0
    ffplay -nodisp -autoexit -loglevel quiet "$BIP" > /dev/null 2>&1
    sleep "$BIP_PAUSE"
}

start() {
    if replay_actif; then
        echo "replay déjà actif (${SEC}s)"
        return 0
    fi
    if en_cours; then
        echo "ECHEC: un enregistrement est déjà en cours"
        return 1
    fi
    mkdir -p "$OUTDIR"
    # le son d'abord, le buffer ensuite (voir `bip_depart`) : l'ordre inverse
    # filmerait le bip
    bip_depart
    # -c mp4 ET -o <dossier> sont exigés en mode replay : gpu-screen-recorder
    # écrit alors Replay_<date>.mp4 DANS ce dossier (un chemin de fichier
    # créerait un dossier portant ce nom — mesuré).
    local args=(-cursor no -w screen -s "$RES" -f "$FPS" -c mp4
                -r "$SEC" -restart-replay-on-save yes -o "$OUTDIR")
    [[ -n "$AUDIO" ]] && args+=(-a "$AUDIO")

    # journal tronqué : `save` y lit le chemin écrit par le binaire
    : > "$LOG"
    setsid flatpak run --command=gpu-screen-recorder "$APP" "${args[@]}" \
        > "$LOG" 2>&1 < /dev/null &
    for _ in $(seq 1 40); do
        replay_actif && break
        sleep 0.1
    done
    if replay_actif; then
        echo "replay actif : ${SEC}s"
    else
        echo "ECHEC: le replay n'a pas démarré (voir /tmp/opencode/replay.log)"
        return 1
    fi
}

save() {
    if ! replay_actif; then
        echo "ECHEC: replay inactif"
        return 1
    fi
    # lignes déjà présentes : seules les NOUVELLES nous intéressent
    local avant; avant=$(wc -l < "$LOG" 2>/dev/null || echo 0)
    # SIGUSR1 = sauvegarde du buffer (le binaire, pas le lanceur flatpak)
    killall -USR1 gpu-screen-recorder 2>/dev/null || true

    local f=""
    for _ in $(seq 1 50); do
        f=$(tail -n +$((avant + 1)) "$LOG" 2>/dev/null \
            | grep -oE '/[^[:space:]]*Replay_[^[:space:]]*\.mp4' | tail -1)
        [[ -n "$f" ]] && break
        sleep 0.1
    done
    if [[ -n "$f" ]]; then
        # Seul retour : la vibration. Aucune notification, aucun son — tout ce
        # qui s'affiche ou se joue peut atterrir dans un clip (l'écran dans le
        # suivant, le son dans le buffer courant).
        [[ -x "$RUMBLE" ]] && "$RUMBLE" "${PAD_NODE:--}" 140 60
        echo "replay sauvegardé : $f"
    else
        echo "ECHEC: aucun fichier écrit"
        return 1
    fi
}

stop() {
    if ! replay_actif; then
        echo "replay déjà arrêté"
        return 0
    fi
    killall -SIGINT gpu-screen-recorder 2>/dev/null || true
    for _ in $(seq 1 30); do
        replay_actif || break
        sleep 0.1
    done
    echo "replay arrêté"
}

case "${1:-status}" in
    start)  start ;;
    save)   save ;;
    stop)   stop ;;
    toggle) if replay_actif; then stop; else start; fi ;;
    status) if replay_actif; then echo "actif ${SEC}s"; else echo "inactif"; fi ;;
    *) echo "usage: $0 start|stop|save|toggle|status"; exit 2 ;;
esac
