%post --nochroot --erroronfail --log=/tmp/anaconda_custom_logs/install-flatpaks.log
# Sélecteur graphique de flatpaks à installer depuis le live ISO
# Affiche une checklist yad, puis installe uniquement les flatpaks cochés
# --nochroot car on doit accéder au repo flatpak du live ISO et à yad
set -euo pipefail

LIVE_FLATPAK="/var/lib/flatpak/repo"
SYSROOT_FLATPAK="/mnt/sysroot/var/lib/flatpak"
FLATPAK_LIST="/usr/share/gablue/flatpaks-available"
SELECTION_FILE="/tmp/gablue-selected-flatpaks"

# Ajouter le dépôt Flathub sur le système cible pour les mises à jour futures
flatpak remote-add --if-not-exists --system \
    --ostree-dir="$SYSROOT_FLATPAK" \
    flathub https://dl.flathub.org/repo/flathub.flatpakrepo || :

# Vérifier que la liste et le repo live existent
if [ ! -f "$FLATPAK_LIST" ] || [ ! -d "$LIVE_FLATPAK" ] || [ ! -d "$SYSROOT_FLATPAK" ]; then
    echo "Liste de flatpaks ou dépôt live introuvable, abandon."
    exit 0
fi

# Construire les arguments yad : --list --checklist avec ref (cachée) et nom lisible
YAD_ARGS=(--list --checklist
    --width=700 --height=400
    --title="Sélection des Flatpaks"
    --text="Choisissez les flatpaks à installer :"
    --column="Installer" --column="Ref" --column="Application"
    --print-column=2 --hide-column=2)

while IFS= read -r ref; do
    [ -z "$ref" ] && continue
    name=$(flatpak info --system --show-name "$ref" 2>/dev/null || echo "$ref")
    YAD_ARGS+=(TRUE "$ref" "$name")
done < "$FLATPAK_LIST"

# Afficher la checklist dans la session liveuser
SELECTED=$(run0 --user=liveuser yad "${YAD_ARGS[@]}" 2>/dev/null) || {
    echo "Aucun flatpak sélectionné ou dialogue annulé"
    exit 0
}

echo "$SELECTED" > "$SELECTION_FILE"

# Copier le dépôt Flatpak du live vers le système installé
rsync -av "$LIVE_FLATPAK/" "$SYSROOT_FLATPAK/repo/"

# Installer uniquement les flatpaks sélectionnés
while IFS= read -r ref; do
    [ -z "$ref" ] && continue
    echo "Installation de $ref..."
    flatpak install --system --no-pull --noninteractive \
        --ostree-dir="$SYSROOT_FLATPAK" "$ref" || echo "Échec: $ref"
done < "$SELECTION_FILE"
%end
