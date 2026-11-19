#!/usr/bin/env bash
# glauncher — script de test, avec TES jeux (config.toml).
#
#   ./test.sh                  TES jeux, PLEIN ÉCRAN, lancement RÉEL
#   ./test.sh windowed         TES jeux, en fenêtre, lancement réel
#   ./test.sh pads             liste les manettes et leurs axes (diagnostic)
#   ./test.sh play <n|nom>     lance vraiment le jeu n° n (ou dont le nom contient « nom »)
#   ./test.sh list             liste numérotée des jeux (pour play)
#   ./test.sh simulate         TES jeux, fenêtre, lancement SIMULÉ (sleep), pour
#                              voir le fondu au noir sans démarrer un vrai jeu
#   ./test.sh demo             jeux factices de demo-games (UI seule)
#   ./test.sh index [cold]     chronomètre l'index (cold = index vidé d'abord)
#   ./test.sh covers [sec]     compte les covers, tourne N s, recompte
#   ./test.sh clean --force    efface le cache (index, covers, icônes)
#
# Dossiers scannés : ceux de config.toml. Navigation dans le launcher : flèches
# ou croix, Entrée/A lance, V/Tab bascule grille/liste, Échap/B quitte.

set -uo pipefail
cd "$(dirname "$0")"

# Toujours la version optimisée : décoder une image en debug est ~90× plus
# lent (47 ms contre 0,53 ms par miniature), la grille mettait 8 s à se remplir.
BIN=target/release/glauncher
CACHE="${XDG_DATA_HOME:-$HOME/.local/share}/glauncher"
LOGS=/tmp/opencode/glauncher-tests

die() { echo "test.sh : $*" >&2; exit 1; }

usage() {
    cat <<'USAGE'
glauncher — script de test, avec tes jeux (config.toml)

  ./test.sh                  tes jeux, PLEIN ÉCRAN, lancement RÉEL
  ./test.sh windowed         tes jeux, en fenêtre, lancement réel
  ./test.sh pads             liste les manettes et leurs axes (diagnostic)
  ./test.sh play <n|nom>     lance vraiment le jeu n° n (ou nom contenant « nom »)
  ./test.sh list             liste numérotée des jeux (pour play)
  ./test.sh simulate         tes jeux, lancement SIMULÉ (fondu + écran noir)
  ./test.sh demo             jeux factices de demo-games (UI seule)
  ./test.sh index [cold]     chronomètre l'index (cold = index vidé d'abord)
  ./test.sh covers [sec]     compte les covers, tourne N s, recompte
  ./test.sh clean --force    efface le cache (index, covers, icônes)

Dans le launcher : flèches/croix naviguent, Entrée/A lance, V/Tab bascule
grille/liste, Échap/B quitte.
USAGE
}

build() {
    cargo build --release || die "compilation impossible"
    mkdir -p "$LOGS"
}

# Même en debug (`cargo run`), les caisses de décodage d'images sont
# optimisées (voir [profile.dev.package] dans Cargo.toml).

# nombre de paquets vus par l'index (quelques ms)
packs() {
    "$BIN" --reindex | sed 's/^/index : /'
}

start() {
    local name="$1"
    shift
    LOG="$LOGS/$name.log"
    setsid "$BIN" "$@" </dev/null >"$LOG" 2>&1 &
    PID=$!
}

stop() {
    kill "$PID" 2>/dev/null
    wait "$PID" 2>/dev/null
}

show_log() {
    echo "--- journal ($LOG)"
    grep -vE "radv is not a conformant" "$LOG" | tail -n 12
}

case "${1:-full}" in
    full|real)
        build
        packs
        start full
        echo "tes jeux en PLEIN ÉCRAN — Entrée/A lance, Échap quitte"
        wait "$PID"
        show_log
        ;;

    windowed)
        build
        packs
        start windowed --windowed
        echo "tes jeux en fenêtre — Entrée/A lance, Échap quitte"
        wait "$PID"
        show_log
        ;;

    pads)
        build
        "$BIN" --pads
        ;;

    check)
        build
        # diagnostic du stick : GLAUNCHER_PAD_DEBUG affiche chaque axe lu
        GLAUNCHER_PAD_DEBUG=1 "$BIN" --pads
        ;;

    play)
        build
        [[ -n "${2:-}" ]] || die "précise un numéro (./test.sh list) ou un nom"
        target="$2"
        if [[ $target =~ ^[0-9]+$ ]]; then
            position=$target
        else
            position=$("$BIN" --list | grep -iF -- "$target" | head -n 1 | cut -f1)
            [[ -n $position ]] || die "aucun jeu ne contient « $target » (./test.sh list)"
        fi
        echo "lancement réel du jeu n° $position :"
        "$BIN" --list | sed -n "${position}p" | cut -f2
        start play --windowed --play "$position"
        echo "Échap quitte le launcher quand le jeu est terminé"
        wait "$PID"
        show_log
        ;;

    list)
        build
        "$BIN" --list | awk -F'\t' '{ printf "%4s  %s\n", $1, $2 }'
        ;;

    simulate)
        build
        packs
        start simulate --windowed --simulate "${2:-5}"
        echo "tes jeux, lancement simulé ${2:-5}s (aucun vrai jeu) — Échap quitte"
        wait "$PID"
        show_log
        ;;

    demo)
        build
        start demo --windowed --simulate "${2:-5}" --dir demo-games
        echo "jeux factices (demo-games), lancement simulé ${2:-5}s — Échap quitte"
        wait "$PID"
        show_log
        ;;

    index)
        build
        if [[ "${2:-}" == "cold" ]]; then
            rm -f "$CACHE"/index-*.sqlite "$CACHE"/index-*.sqlite-wal "$CACHE"/index-*.sqlite-shm
            echo "index vidé — scan à froid"
        fi
        packs
        echo "--- deuxième passage (index chaud)"
        packs
        ;;

    covers)
        build
        seconds="${2:-12}"
        before=$(find "$CACHE/assets/grids" -type f -name "*.png" -o -type f -name "*.jpg" 2>/dev/null | wc -l)
        echo "covers en cache avant : $before — tes jeux pendant ${seconds}s"
        start covers --windowed
        sleep "$seconds"
        stop
        after=$(find "$CACHE/assets/grids" -type f -name "*.png" -o -type f -name "*.jpg" 2>/dev/null | wc -l)
        echo "covers en cache après : $after (+$((after - before)) en ${seconds}s)"
        echo "--- téléchargements"
        grep "Grid SGDB" "$LOG" | tail -n 20
        ;;

    clean)
        [[ "${2:-}" == "--force" ]] || die "supprime $CACHE (index, covers, icônes) — relance avec : ./test.sh clean --force"
        rm -rf "$CACHE"
        echo "cache effacé : $CACHE"
        ;;

    -h|--help|help)
        usage
        ;;

    *)
        usage
        die "mode inconnu : $1"
        ;;
esac
