#!/usr/bin/bash
# Critère du goal « interface unifiée » : une seule coquille, plus d'onglets
# plein écran, prouvée par des tests headless. Doit échouer tant que l'enum
# `View` et les 4 interfaces coexistent.
set -euo pipefail
cd "$(dirname "$0")"

export PKG_CONFIG_PATH="$PWD/.build/alsa/pkgconfig:${PKG_CONFIG_PATH:-}"
fail() { echo "ÉCHEC : $*"; exit 1; }

# 1. Une seule coquille : l'enum des 4 vues plein écran a disparu
if grep -q "enum View" crates/gui/src/main.rs; then
    fail "l'enum View subsiste (Catalogue/Téléchargés/Découpe/GTracker séparés)"
fi

# 2. Le shell unifié se vérifie sans écran (egui::Context::run_ui) et le dit
if ! out=$(cargo test -q -p gtracker shell -- --nocapture 2>&1); then
    echo "$out" | tail -30
    fail "tests du shell rouges"
fi
grep -q "SHELL OK" <<<"$out" || fail "marqueur « SHELL OK » absent (tests du shell non exécutés)"

# 3. Aucune régression du moteur ni du GTracker
./check.sh

echo "UI OK"
