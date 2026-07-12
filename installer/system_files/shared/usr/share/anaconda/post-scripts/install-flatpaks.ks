%post --nochroot --erroronfail --log=/tmp/anaconda_custom_logs/install-flatpaks.log
# Installation des flatpaks depuis le live ISO
# 1. Lecture de la sélection faite en %pre-install (gablue-questions.ks)
# 2. Copie de /var/lib/flatpak (live) vers le déploiement ostree via rsync
# 3. Désinstallation des optionnels non désirés DIRECTEMENT dans la cible
#
# Le choix des optionnels à conserver est fait plus tôt (yad en
# %pre-install) et écrit dans SELECTION_FILE. Ici on ne fait que lire ce
# fichier (fichier absent => aucun optionnel conservé).
#
# Pourquoi désinstaller dans la cible et non dans le live :
# - Le live monte /var/lib/flatpak en overlayfs : flatpak uninstall échoue
#   en EXDEV ("Invalid cross-device link") car les hardlinks entre
#   repo/objects et les checkouts ne traversent pas les couches overlay.
# - La cible ostree est sur btrfs (RW, monolithique) : uninstall fonctionne.
set -euo pipefail

FLATPAK_OPTIONAL="/usr/share/gablue/flatpaks-optional"
SELECTION_FILE="/tmp/gablue-selected-flatpaks"
TARGET_INSTALLATION="gtarget"

# Extraire l'ID flatpak d'une full ref (app/ID/arch/branch -> ID)
flatpak_id() { local r="$1"; r="${r#*/}"; echo "${r%%/*}"; }

# =============================================================================
# LECTURE DE LA SÉLECTION (faite en %pre-install)
# =============================================================================

# TO_KEEP = liste des refs à conserver (une par ligne). Fichier absent
# (pre-install non exécuté / annulé) => aucun optionnel conservé.
TO_KEEP=""
if [ -f "$SELECTION_FILE" ]; then
    TO_KEEP="$(cat "$SELECTION_FILE")"
fi

# =============================================================================
# COPIER TOUT /var/lib/flatpak VERS LE DÉPLOIEMENT OSTREE
# =============================================================================

# --filter="-x security.selinux" : ne pas synchroniser le xattr SELinux.
# Les fichiers du live sont étiquetés unlabeled_t et SELinux enforcing
# refuse le lremovexattr/lsetxattr sur la cible (Permission denied -> rsync
# code 23 -> échec du %post -> crash Anaconda "Message recipient
# disconnected"). Les autres xattrs (user.ostree*) restent copiés.
deployment="$(ostree rev-parse --repo=/mnt/sysimage/ostree/repo ostree/0/1/0)"
target="/mnt/sysimage/ostree/deploy/default/deploy/${deployment}.0/var/lib/"
mkdir -p "$target"
rsync -aAXUHKP --open-noatime --filter="-x security.selinux" /var/lib/flatpak "$target"
sync "$target"

# =============================================================================
# DÉSINSTALLER LES OPTIONNELS NON DÉSIRÉS DANS LA CIBLE
# =============================================================================

flatpak_target="${target}flatpak"

# Enregistrer une installation flatpak pointant sur la cible ostree.
# flatpak, lancé en root avec --installation=<nom>, opère directement sur
# le dépôt (pas besoin du helper D-Bus système), et la cible étant sur
# btrfs il n'y a pas d'erreur EXDEV.
mkdir -p /etc/flatpak/installations.d
cat > "/etc/flatpak/installations.d/${TARGET_INSTALLATION}.conf" << EOF
[Installation "${TARGET_INSTALLATION}"]
Path=${flatpak_target}
EOF

# Désinstaller toutes les refs (toutes branches) correspondant à un ID donné.
uninstall_target_id() {
    local id="$1"
    flatpak --installation="$TARGET_INSTALLATION" list --columns=ref 2>/dev/null \
        | awk -F/ -v id="$id" '$1 == id' \
        | while IFS= read -r fullref; do
            [ -z "$fullref" ] && continue
            echo "Désinstallation de $fullref..."
            flatpak --installation="$TARGET_INSTALLATION" uninstall --noninteractive "$fullref" \
                || echo "Échec désinstallation: $fullref"
        done
}

if [ -f "$FLATPAK_OPTIONAL" ]; then
    # Optionnels non cochés
    while IFS= read -r ref; do
        [ -z "$ref" ] && continue
        if ! echo "$TO_KEEP" | grep -qF "$ref"; then
            uninstall_target_id "$(flatpak_id "$ref")"
        fi
    done < "$FLATPAK_OPTIONAL"

    # =========================================================================
    # FLATPAKS CONDITIONNELS (non affichés dans la checklist)
    # =========================================================================

    # Proton-GE : conservé uniquement si Steam l'est
    if ! echo "$TO_KEEP" | grep -q "com.valvesoftware.Steam"; then
        echo "Steam non conservé, désinstallation de Proton-GE..."
        uninstall_target_id "com.valvesoftware.Steam.CompatibilityTool.Proton-GE"
    fi

    # OBS VkCapture : conservé uniquement si OBS Studio l'est
    if ! echo "$TO_KEEP" | grep -q "com.obsproject.Studio"; then
        echo "OBS non conservé, désinstallation d'OBS VkCapture..."
        uninstall_target_id "org.freedesktop.Platform.VulkanLayer.OBSVkCapture"
    fi

    # Nettoyer les runtimes devenus orphelins par les désinstallations
    echo "Nettoyage des runtimes inutilisés..."
    flatpak --installation="$TARGET_INSTALLATION" uninstall --unused --noninteractive || :
fi

sync "$flatpak_target"
%end
