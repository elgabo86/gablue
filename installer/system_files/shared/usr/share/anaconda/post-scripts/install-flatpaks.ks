%post --nochroot --erroronfail --log=/tmp/anaconda_custom_logs/install-flatpaks.log
# Installation des flatpaks depuis le live ISO
# Copie brute de /var/lib/flatpak vers le déploiement ostree (même méthode que Bazzite)
# Puis checklist yad pour désinstaller les optionnels non désirés
set -euo pipefail

FLATPAK_OPTIONAL="/usr/share/gablue/flatpaks-optional"
SELECTION_FILE="/tmp/gablue-selected-flatpaks"

# =============================================================================
# PRÉPARATION : RENDRE /var/lib/flatpak ACCESSIBLE EN ÉCRITURE
# =============================================================================

# Le live monte /var/lib/flatpak en read-only via bind mount.
# On doit le démonter pour que flatpak uninstall puisse écrire.
# Note: le remote Flathub est déjà configuré dans /etc/flatpak/remotes.d/
# par build.sh, donc pas besoin de flatpak remote-add ici.
umount /var/lib/flatpak 2>/dev/null || :

# S'assurer que le helper système flatpak est accessible via D-Bus
flatpak --system repair --dry-run 2>/dev/null || :

# =============================================================================
# FLATPAKS OPTIONNELS : CHECKLIST YAD (DÉSINSTALLATION)
# =============================================================================

# Extraire l'ID flatpak d'une full ref
flatpak_id() { local r="$1"; r="${r#*/}"; echo "${r%%/*}"; }

if [ -f "$FLATPAK_OPTIONAL" ]; then
    YAD_ARGS=(--list --checklist
        --width=700 --height=400
        --title="Sélection des Flatpaks"
        --text="Choisissez les flatpaks supplémentaires à conserver.\nLes autres seront désinstallés :"
        --column="Garder" --column="Ref" --column="Application"
        --print-column=2 --hide-column=2)

    while IFS= read -r ref; do
        [ -z "$ref" ] && continue
        name=$(flatpak info --system --show-name "$ref" 2>/dev/null || echo "$ref")
        YAD_ARGS+=(FALSE "$ref" "$name")
    done < "$FLATPAK_OPTIONAL"

    TO_KEEP=$(run0 --user=liveuser yad "${YAD_ARGS[@]}" 2>/dev/null) || {
        echo "Dialogue annulé, désinstallation de tous les flatpaks optionnels..."
        TO_KEEP=""
    }

    echo "$TO_KEEP" > "$SELECTION_FILE"

    # Désinstaller les optionnels non cochés du live (AVANT rsync vers la cible)
    while IFS= read -r ref; do
        [ -z "$ref" ] && continue
        id=$(flatpak_id "$ref")
        if ! echo "$TO_KEEP" | grep -qF "$ref"; then
            echo "Désinstallation de $id..."
            flatpak uninstall --system --noninteractive "$id" || echo "Échec désinstallation: $id"
        fi
    done < "$FLATPAK_OPTIONAL"

    # =========================================================================
    # FLATPAKS CONDITIONNELS (non affichés dans la checklist)
    # =========================================================================

    # Proton-GE : installé uniquement si Steam est conservé
    if ! echo "$TO_KEEP" | grep -q "com.valvesoftware.Steam"; then
        echo "Steam non conservé, désinstallation de Proton-GE..."
        flatpak uninstall --system --noninteractive com.valvesoftware.Steam.CompatibilityTool.Proton-GE || :
    fi

    # OBS VkCapture : installé uniquement si OBS Studio est conservé
    if ! echo "$TO_KEEP" | grep -q "com.obsproject.Studio"; then
        echo "OBS non conservé, désinstallation d'OBS VkCapture..."
        flatpak uninstall --system --noninteractive org.freedesktop.Platform.VulkanLayer.OBSVkCapture || :
    fi
fi

# =============================================================================
# COPIER TOUT /var/lib/flatpak VERS LE DÉPLOIEMENT OSTREE
# =============================================================================

deployment="$(ostree rev-parse --repo=/mnt/sysimage/ostree/repo ostree/0/1/0)"
target="/mnt/sysimage/ostree/deploy/default/deploy/${deployment}.0/var/lib/"
mkdir -p "$target"
rsync -aAXUHKP --open-noatime /var/lib/flatpak "$target"
sync "$target"

# =============================================================================
# RESTAURER LES LABELS SELINUX SUR LE LIVE
# =============================================================================

chcon -R -t var_lib_t /var/lib/flatpak || :
%end
