%post --nochroot --erroronfail --log=/tmp/anaconda_custom_logs/install-flatpaks.log
# Installation des flatpaks depuis le live ISO
# 1. Obligatoires : installés automatiquement
# 2. Optionnels  : checklist yad, installés si cochés
# --nochroot car on doit accéder au repo flatpak du live ISO et à yad
set -euo pipefail

LIVE_FLATPAK="/var/lib/flatpak/repo"
SYSROOT_FLATPAK="/mnt/sysroot/var/lib/flatpak"
FLATPAK_REQUIRED="/usr/share/gablue/flatpaks-required"
FLATPAK_OPTIONAL="/usr/share/gablue/flatpaks-optional"
SELECTION_FILE="/tmp/gablue-selected-flatpaks"

# Ajouter le dépôt Flathub sur le système cible pour les mises à jour futures
flatpak remote-add --if-not-exists --system \
    --ostree-dir="$SYSROOT_FLATPAK" \
    flathub https://dl.flathub.org/repo/flathub.flatpakrepo || :

if [ ! -d "$LIVE_FLATPAK" ] || [ ! -d "$SYSROOT_FLATPAK" ]; then
    echo "Dépôt flatpak live introuvable, abandon."
    exit 0
fi

# Copier le dépôt Flatpak du live vers le système installé
rsync -av "$LIVE_FLATPAK/" "$SYSROOT_FLATPAK/repo/"

# =============================================================================
# FLATPAKS OBLIGATOIRES
# =============================================================================

if [ -f "$FLATPAK_REQUIRED" ]; then
    echo "Installation des flatpaks obligatoires..."
    while IFS= read -r ref; do
        [ -z "$ref" ] && continue
        echo "  -> $ref"
        flatpak install --system --no-pull --noninteractive \
            --ostree-dir="$SYSROOT_FLATPAK" "$ref" || echo "Échec: $ref"
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
    echo "Installation de $ref..."
    flatpak install --system --no-pull --noninteractive \
        --ostree-dir="$SYSROOT_FLATPAK" "$ref" || echo "Échec: $ref"
done < "$SELECTION_FILE"
%end
