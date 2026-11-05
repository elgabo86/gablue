#!/bin/bash

# Script d'assemblage de lgplaunch en un fichier standalone
# Usage: ./build.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT_FILE="$SCRIPT_DIR/lgplaunch-standalone.sh"

# Ordre des modules (identique à l'ordre du fichier d'origine)
LIB_FILES=(
    "lib/core.sh"
    "lib/overlay.sh"
    "lib/args.sh"
    "lib/lgp.sh"
    "lib/exe.sh"
    "lib/saves.sh"
    "lib/copy.sh"
    "lib/extras.sh"
    "lib/temp.sh"
    "lib/symlinks.sh"
    "lib/env.sh"
    "lib/launch.sh"
    "lib/modes.sh"
)

echo "Assemblage de lgplaunch en fichier standalone..."

cat > "$OUTPUT_FILE" << 'HEADER'
#!/bin/bash

################################################################################
# lgplaunch - Script de lancement de jeux Linux (ex-launchlin.sh)
#
# Ce script permet de lancer des jeux/applications Linux via des paquets LGP
# compressés, avec support :
# - Des paquets LGP (.lgp) compressés en squashfs via squashfuse
# - Des exécutables directs (binaires ELF, AppImages, scripts .sh/.py)
# - Sélection d'un exécutable dans le pack : menu (--exe) ou chemin direct
#   (--exe=game.sh, relatif à la racine du pack)
# - Script de pré-lancement .script.sh
# - Kernel overlayfs natif via unshare pour les fichiers temporaires (user namespaces, pas de sudo)
#
# NOTE: Utilise squashfuse (FUSE) pour le .lgp et kernel overlayfs pour .temp
# Avantage : pas besoin de sudo, copy-on-write rapide pour les gros fichiers
# Un symlink de compat launchlin.sh (ancien emplacement scripts/) -> /usr/bin/lgplaunch
# est fourni pour les raccourcis .desktop créés par les anciennes versions.
################################################################################
HEADER

concat_file() {
    local file="$1"
    echo "" >> "$OUTPUT_FILE"
    echo "# =============================================================================" >> "$OUTPUT_FILE"
    echo "# Module: $(basename "$file")" >> "$OUTPUT_FILE"
    echo "# =============================================================================" >> "$OUTPUT_FILE"
    echo "" >> "$OUTPUT_FILE"
    if head -1 "$file" | grep -q "^#!/"; then
        tail -n +2 "$file" >> "$OUTPUT_FILE"
    else
        cat "$file" >> "$OUTPUT_FILE"
    fi
}

for lib_file in "${LIB_FILES[@]}"; do
    full_path="$SCRIPT_DIR/$lib_file"
    if [ -f "$full_path" ]; then
        echo "  → Ajout de $lib_file"
        concat_file "$full_path"
    else
        echo "ERREUR: Fichier manquant: $lib_file"
        exit 1
    fi
done

echo "  → Ajout du code principal"
echo "" >> "$OUTPUT_FILE"
echo "#======================================" >> "$OUTPUT_FILE"
echo "# Fonction principale" >> "$OUTPUT_FILE"
echo "#======================================" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"
sed -n '/^main() {$/,$p' "$SCRIPT_DIR/lgplaunch" >> "$OUTPUT_FILE"

chmod +x "$OUTPUT_FILE"

LINE_COUNT=$(wc -l < "$OUTPUT_FILE")
echo ""
echo "✓ Assemblage terminé: $OUTPUT_FILE ($LINE_COUNT lignes)"
