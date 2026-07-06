%post --nochroot --erroronfail --log=/tmp/anaconda_custom_logs/install-flatpaks.log
# Installation des flatpaks depuis le live ISO (offline)
# 1. Obligatoires : installés automatiquement
# 2. Optionnels  : checklist yad, installés si cochés
# Utilise create-usb + sideload-repo pour une install fiable sans réseau
set -euo pipefail

SYSROOT_FLATPAK="/mnt/sysroot/var/lib/flatpak"
SIDELOAD_DIR="/run/gablue-sideload"
FLATPAK_REQUIRED="/usr/share/gablue/flatpaks-required"
FLATPAK_OPTIONAL="/usr/share/gablue/flatpaks-optional"
SELECTION_FILE="/tmp/gablue-selected-flatpaks"

mkdir -p "$SIDELOAD_DIR" "$SYSROOT_FLATPAK"

# Ajouter le dépôt Flathub sur le système cible
flatpak remote-add --if-not-exists --system \
    --ostree-dir="$SYSROOT_FLATPAK" \
    flathub https://dl.flathub.org/repo/flathub.flatpakrepo || :

# Fonction : extraire l'ID flatpak d'une full ref
flatpak_id() { local r="$1"; r="${r#*/}"; echo "${r%%/*}"; }

# =============================================================================
# EXPORTER TOUS LES FLATPAKS PRÉ-INSTALLÉS DU LIVE VERS UN REPO SIDELOAD
# =============================================================================

echo "Export des flatpaks vers le repo sideload..."

if [ -f "$FLATPAK_REQUIRED" ]; then
    while IFS= read -r ref; do
        [ -z "$ref" ] && continue
        id=$(flatpak_id "$ref")
        echo "  -> $id"
        flatpak create-usb --system --allow-partial "$SIDELOAD_DIR" "$id" || echo "Export échoué: $id"
    done < "$FLATPAK_REQUIRED"
fi

if [ -f "$FLATPAK_OPTIONAL" ]; then
    while IFS= read -r ref; do
        [ -z "$ref" ] && continue
        id=$(flatpak_id "$ref")
        echo "  -> $id"
        flatpak create-usb --system --allow-partial "$SIDELOAD_DIR" "$id" || echo "Export échoué: $id"
    done < "$FLATPAK_OPTIONAL"
fi

# =============================================================================
# FLATPAKS OBLIGATOIRES
# =============================================================================

if [ -f "$FLATPAK_REQUIRED" ]; then
    echo "Installation des flatpaks obligatoires..."
    while IFS= read -r ref; do
        [ -z "$ref" ] && continue
        id=$(flatpak_id "$ref")
        echo "  -> $id"
        flatpak install --system --sideload-repo="$SIDELOAD_DIR" --noninteractive \
            --ostree-dir="$SYSROOT_FLATPAK" "$id" || echo "Échec: $id"
    done < "$FLATPAK_REQUIRED"
fi

# =============================================================================
# FLATPAKS OPTIONNELS (CHECKLIST YAD)
# =============================================================================

if [ ! -f "$FLATPAK_OPTIONAL" ]; then
    exit 0
fi

YAD_ARGS=(--list --checklist
    --width=700 --height=400
    --title="Sélection des Flatpaks"
    --text="Choisissez les flatpaks supplémentaires à installer :"
    --column="Installer" --column="Ref" --column="Application"
    --print-column=2 --hide-column=2)

while IFS= read -r ref; do
    [ -z "$ref" ] && continue
    name=$(flatpak info --system --show-name "$ref" 2>/dev/null || echo "$ref")
    YAD_ARGS+=(FALSE "$ref" "$name")
done < "$FLATPAK_OPTIONAL"

SELECTED=$(run0 --user=liveuser yad "${YAD_ARGS[@]}" 2>/dev/null) || {
    echo "Aucun flatpak optionnel sélectionné"
    exit 0
}

echo "$SELECTED" > "$SELECTION_FILE"

while IFS= read -r ref; do
    [ -z "$ref" ] && continue
    id=$(flatpak_id "$ref")
    echo "Installation de $id..."
    flatpak install --system --sideload-repo="$SIDELOAD_DIR" --noninteractive \
        --ostree-dir="$SYSROOT_FLATPAK" "$id" || echo "Échec: $id"
done < "$SELECTION_FILE"
%end
