#!/bin/bash

# Script d'assemblage de Gwine en un fichier standalone
# Usage: ./build.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT_FILE="$SCRIPT_DIR/gwine-standalone.sh"

# Ordre des fichiers lib (doit correspondre à l'ordre des source dans gwine)
# IMPORTANT: Lister tous les sous-modules directement, pas les fichiers de redirection
# car les fichiers de redirection utilisent des chemins relatifs qui ne fonctionnent pas en standalone
LIB_FILES=(
    "lib/core.sh"
    "lib/parse-args.sh"
    "lib/xbox.sh"
    "lib/runner.sh"
    "lib/dxvk-mode.sh"
    "lib/display-mode.sh"
    "lib/download.sh"
    "lib/gpu.sh"
    "lib/ui.sh"
    "lib/components/utils.sh"
    "lib/components/dxvk.sh"
    "lib/components/mono.sh"
    "lib/components/nvapi.sh"
    "lib/components/winetricks.sh"
    "lib/wincomponents/defs.sh"
    "lib/wincomponents/utils.sh"
    "lib/wincomponents/vcrun.sh"
    "lib/wincomponents/dotnet.sh"
    "lib/wincomponents/directx.sh"
    "lib/wincomponents/fonts.sh"
    "lib/wincomponents/misc.sh"
    "lib/wincomponents/wmp9.sh"
    "lib/wincomponents/main.sh"
    "lib/cache/gwine-runner.sh"
    "lib/cache/dxvk-vkd3d.sh"
    "lib/cache/offline.sh"
    "lib/cache/cachepack.sh"
    "lib/wineprefix.sh"
    "lib/wineserver/init.sh"
    "lib/wineserver/master.sh"
    "lib/wineserver/counter.sh"
    "lib/wineserver/server.sh"
    "lib/wineserver/cleanup.sh"
    "lib/sandbox.sh"
    "lib/wgp/core.sh"
    "lib/wgp/mount.sh"
    "lib/wgp/overlay.sh"
    "lib/wgp/symlinks.sh"
    "lib/wgp/modes.sh"
    "lib/launcher-utils/env.sh"
    "lib/launcher-utils/registry.sh"
    "lib/launcher-utils/pds.sh"
    "lib/launcher-utils/paths.sh"
    "lib/launcher-utils/conflict.sh"
    "lib/launcher-utils/setup.sh"
    "lib/launcher-main.sh"
    "lib/modes/init-main.sh"
    "lib/modes/init-ensure.sh"
    "lib/mode-update.sh"
    "lib/mode-tools.sh"
    "lib/dir-config.sh"
)

echo "Assemblage de Gwine en fichier standalone..."

# Créer l'en-tête
cat > "$OUTPUT_FILE" << 'HEADER'
#!/bin/bash

################################################################################
# Gwine - Lanceur de jeux Windows avec Wine pur (gwine)
#
# Script standalone pour lancer des jeux Windows via Wine.
# Supporte les paquets WGP (.wgp) et les exécutables directs (.exe)
#
# Gwine: Wine TKG modifié avec Proton
# https://github.com/elgabo86/gwine
#
# Fichier généré automatiquement - Ne pas modifier manuellement
################################################################################

# Déterminer le répertoire du script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

HEADER

# Fonction pour concaténer un fichier sans son shebang
concat_file() {
    local file="$1"
    local name
    name=$(basename "$file")
    echo "" >> "$OUTPUT_FILE"
    echo "# =============================================================================" >> "$OUTPUT_FILE"
    echo "# Module: $name" >> "$OUTPUT_FILE"
    echo "# =============================================================================" >> "$OUTPUT_FILE"
    echo "" >> "$OUTPUT_FILE"

    # Lire le fichier et ignorer la première ligne si c'est un shebang
    if head -1 "$file" | grep -q "^#!/"; then
        tail -n +2 "$file" >> "$OUTPUT_FILE"
    else
        cat "$file" >> "$OUTPUT_FILE"
    fi
}

# Concaténer tous les fichiers lib
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

# Ajouter le contenu du fichier principal gwine (sans les source et sans le shebang)
echo ""
echo "  → Ajout du code principal"
echo "" >> "$OUTPUT_FILE"
echo "# =============================================================================" >> "$OUTPUT_FILE"
echo "# Code principal" >> "$OUTPUT_FILE"
echo "# =============================================================================" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"

# Lire gwine et ignorer les lignes de source
awk '
    /^#!/ { next }
    /^source "\$SCRIPT_DIR\/lib\// { next }
    { print }
' "$SCRIPT_DIR/gwine" >> "$OUTPUT_FILE"

# Rendre exécutable
chmod +x "$OUTPUT_FILE"

# Embarquer les shims overlayfs en base64
echo "" >> "$OUTPUT_FILE"
echo "# =============================================================================" >> "$OUTPUT_FILE"
echo "# Overlayfs statfs shim (binaires embarqués)" >> "$OUTPUT_FILE"
echo "# =============================================================================" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"

echo "  → Embedding composefs_statfs_shim.so (64-bit)"
echo "GWINE_SHIM64_DATA='$(base64 -w0 "$SCRIPT_DIR/lib/composefs_statfs_shim.so")'" >> "$OUTPUT_FILE"

echo "  → Embedding composefs_statfs_shim32.so (32-bit)"
echo "GWINE_SHIM32_DATA='$(base64 -w0 "$SCRIPT_DIR/lib/composefs_statfs_shim32.so")'" >> "$OUTPUT_FILE"

cat >> "$OUTPUT_FILE" << 'SHIM_EXTRACT'

# Extraction des shims overlayfs au runtime
_gwine_extract_shims() {
    mkdir -p "$GWINE_LIB_DIR/lib64" "$GWINE_LIB_DIR/lib"
    local shim64="$GWINE_LIB_DIR/lib64/composefs_statfs_shim.so"
    local shim32="$GWINE_LIB_DIR/lib/composefs_statfs_shim.so"
    if [ -n "${GWINE_SHIM64_DATA:-}" ] && [ ! -f "$shim64" ]; then
        echo "$GWINE_SHIM64_DATA" | base64 -d > "$shim64" 2>/dev/null
    fi
    if [ -n "${GWINE_SHIM32_DATA:-}" ] && [ ! -f "$shim32" ]; then
        echo "$GWINE_SHIM32_DATA" | base64 -d > "$shim32" 2>/dev/null
    fi
}
SHIM_EXTRACT

# Calculer les tailles
ORIG_SIZE=$(du -sh "$SCRIPT_DIR" | cut -f1)
OUTPUT_SIZE=$(du -sh "$OUTPUT_FILE" | cut -f1)
LINE_COUNT=$(wc -l < "$OUTPUT_FILE")

echo ""
echo "✓ Assemblage terminé !"
echo ""
echo "Fichier généré: $OUTPUT_FILE"
echo "Lignes: $LINE_COUNT"
echo "Taille: $OUTPUT_SIZE"
echo ""
echo "Usage: ./gwine-standalone.sh [options] [fichier]"
