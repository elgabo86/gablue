#!/usr/bin/bash
# Vérification unique de l'objectif GTracker. Tout ce qui décide est ici :
# tests, moteur (rendu hors temps réel), projet (aller-retour), tonalité.
# Un « OK » à la fin = l'objectif est atteint, sans interprétation.
set -euo pipefail
cd "$(dirname "$0")"

# cpal a besoin des en-têtes ALSA de brew + du libasound du système
export PKG_CONFIG_PATH="$PWD/.build/alsa/pkgconfig:${PKG_CONFIG_PATH:-}"
if [[ ! -f .build/alsa/pkgconfig/alsa.pc ]]; then
    ./build.sh >/dev/null
fi

fail() { echo "ÉCHEC : $*"; exit 1; }

run_example() {   # run_example <exemple> <marqueur>
    local out
    out=$(cargo run -q --release -p gtracker-core --example "$1" 2>&1) || {
        echo "$out"
        fail "$1 n'a pas tourné"
    }
    echo "$out"
    grep -q "$2" <<<"$out" || fail "$1 : marqueur « $2 » absent"
}

echo "== tests =="
tests=$(cargo test -q -p gtracker-core -p gtracker 2>&1) || {
    echo "$tests" | tail -20
    fail "tests rouges"
}
echo "$tests" | grep -E "test result: ok" | tail -4
# deux crates testés = deux lignes de résultat ; une seule signifie qu'un
# crate n'a pas tourné (ou a été filtré) et ne prouve rien
n=$(grep -c "test result: ok" <<<"$tests")
[[ $n -ge 2 ]] || fail "un seul crate a été testé ($n résultats)"

echo "== moteur : même rendu hors temps réel que temps réel =="
run_example mixcheck "MIX OK"

echo "== projet : aller-retour .gtrack =="
run_example projcheck "PROJET OK"

echo "== tonalité + transposition =="
run_example keycheck "TONALITE OK"

echo "== stretch : durée exacte, hauteur tenue =="
run_example stretchcheck "STRETCH OK"

echo "== export : mixage → FLAC → relecture =="
run_example exportcheck "EXPORT OK"

echo OK
