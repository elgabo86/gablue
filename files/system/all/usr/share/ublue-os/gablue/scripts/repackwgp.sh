#!/bin/bash

################################################################################
# repackwgp.sh - Script de repackaging de paquets WGP
#
# Ce script permet de mettre à jour un vieux WGP vers la nouvelle architecture
# (symlinks /tmp/wgp-saves au lieu de chemins utilisateur durs).
#
# Le script monte d'abord le WGP pour vérifier, et ne l'extrait que si
# des corrections sont nécessaires.
################################################################################

#======================================
# Variables globales
#======================================
WGPACK_FILE=""
GAME_NAME=""
GAME_INTERNAL_NAME=""
OUTPUT_FILE=""
TEMP_DIR=""
MOUNT_DIR=""
CHANGES_NEEDED=false

#======================================
# Fonctions de nettoyage
#======================================

cleanup() {
    # Démonter le squashfs si monté
    if [ -n "$MOUNT_DIR" ] && mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        fusermount -u "$MOUNT_DIR" 2>/dev/null || true
        rm -rf "$MOUNT_DIR"
    fi
    # Nettoyer l'extraction si faite
    if [ -n "$TEMP_DIR" ]; then
        rm -rf "$TEMP_DIR" 2>/dev/null
    fi
}

trap cleanup EXIT

error_exit() {
    echo "Erreur: $1" >&2
    exit 1
}

ask_yes_no() {
    local prompt="$1"
    read -p "$prompt (o/N): " -r
    [[ "$REPLY" =~ ^[oOyY]$ ]]
}

#======================================
# Fonctions de validation
#======================================

validate_arguments() {
    [ $# -eq 0 ] && error_exit "Usage: $0 <fichier_wgp> [fichier_sortie]"

    WGPACK_FILE="$(realpath "$1")"

    [ ! -f "$WGPACK_FILE" ] && error_exit "Le fichier '$WGPACK_FILE' n'existe pas"

    GAME_NAME=$(basename "$WGPACK_FILE" .wgp)

    if [ -n "$2" ]; then
        OUTPUT_FILE="$2"
    else
        OUTPUT_FILE="${WGPACK_FILE%.wgp}_repacked.wgp"
    fi
}

#======================================
# Fonctions de montage et vérification
#======================================

mount_wgp() {
    MOUNT_DIR="/tmp/wgp_mount_$$"
    mkdir -p "$MOUNT_DIR"

    echo ""
    echo "=== Montage du WGP pour vérification ==="
    echo "Fichier: $WGPACK_FILE"
    echo "Dossier de montage: $MOUNT_DIR"

    if ! command -v squashfuse &> /dev/null; then
        error_exit "squashfuse n'est pas installé"
    fi

    squashfuse -r "$WGPACK_FILE" "$MOUNT_DIR"

    # Lire le nom interne du jeu
    GAMENAME_FILE="$MOUNT_DIR/.gamename"
    if [ -f "$GAMENAME_FILE" ]; then
        GAME_INTERNAL_NAME=$(cat "$GAMENAME_FILE")
    else
        GAME_INTERNAL_NAME="$GAME_NAME"
    fi

    echo "Nom interne du jeu: $GAME_INTERNAL_NAME"
}

check_symlinks() {
    [ ! -f "$MOUNT_DIR/.savepath" ] && return 0

    echo ""
    echo "=== Vérification des symlinks de sauvegarde ==="

    local symlinks_count=0

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SYMLINK_PATH="$MOUNT_DIR/$SAVE_REL_PATH"

        if [ -L "$SYMLINK_PATH" ]; then
            local current_target
            current_target=$(readlink "$SYMLINK_PATH")
            symlinks_count=$((symlinks_count + 1))

            # Vérifier si le symlink pointe déjà vers /tmp/wgp-saves
            if [[ "$current_target" == /tmp/wgp-saves/* ]]; then
                echo "Symlink: $SAVE_REL_PATH → $current_target (OK)"
            else
                echo "Symlink: $SAVE_REL_PATH → $current_target (à corriger)"
                CHANGES_NEEDED=true
            fi
        fi
    done < "$MOUNT_DIR/.savepath"

    if [ $symlinks_count -eq 0 ]; then
        echo "Aucun symlink de sauvegarde trouvé."
    fi

    if ! $CHANGES_NEEDED; then
        echo ""
        echo "✓ Tous les symlinks pointent déjà vers /tmp/wgp-saves"
    fi
}

check_extras() {
    [ ! -f "$MOUNT_DIR/.extrapath" ] && return 0

    echo ""
    echo "=== Extras ==="
    echo "Les extras utilisent /tmp/wgp-extra avec le nom du dossier."
    echo "Ils ne sont pas concernés par ce repackaging."
}

#======================================
# Fonctions d'extraction et correction
#======================================

extract_and_fix() {
    # Démonter le mount
    fusermount -u "$MOUNT_DIR" 2>/dev/null || true
    rm -rf "$MOUNT_DIR"
    MOUNT_DIR=""

    # Extraire
    TEMP_DIR="/tmp/wgp_repack_$$"
    mkdir -p "$TEMP_DIR"

    echo ""
    echo "=== Extraction du WGP ==="
    echo "Dossier temporaire: $TEMP_DIR"

    unsquashfs -f -d "$TEMP_DIR" -no-xattrs "$WGPACK_FILE"

    # Corriger les symlinks
    echo ""
    echo "=== Correction des symlinks de sauvegarde ==="

    local NEW_SAVE_BASE="/tmp/wgp-saves/$GAME_INTERNAL_NAME"

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SYMLINK_PATH="$TEMP_DIR/$SAVE_REL_PATH"

        if [ -L "$SYMLINK_PATH" ]; then
            local current_target
            current_target=$(readlink "$SYMLINK_PATH")

            if [[ "$current_target" != /tmp/wgp-saves/* ]]; then
                rm -f "$SYMLINK_PATH"
                ln -s "$NEW_SAVE_BASE/$SAVE_REL_PATH" "$SYMLINK_PATH"
                echo "Corrigé: $SAVE_REL_PATH → /tmp/wgp-saves/$GAME_INTERNAL_NAME/$SAVE_REL_PATH"
            fi
        fi
    done < "$TEMP_DIR/.savepath"
}

#======================================
# Fonctions de création du squashfs
#======================================

repack_wgp() {
    echo ""
    echo "=== Création du WGP repackagé ==="

    mksquashfs "$TEMP_DIR" "$OUTPUT_FILE" -nopad -all-root
}

#======================================
# Affichage du résumé
#======================================

show_summary() {
    if $CHANGES_NEEDED; then
        echo ""
        echo "=== Résumé ==="
        echo "Fichier source: $WGPACK_FILE"
        echo "Fichier sortie: $OUTPUT_FILE"
        echo ""
        echo "Des corrections de symlinks sont nécessaires. Ce script va:"
        echo "  1. Extraire le WGP"
        echo "  2. Corriger les symlinks de sauvegarde pour pointer vers /tmp/wgp-saves"
        echo "  3. Recréer le fichier WGP"
    fi
}

#======================================
# Fonction principale
#======================================

main() {
    cat << 'EOF'
╔══════════════════════════════════════════════════════════════╗
║          Repackaging de WGP vers nouvelle architecture       ║
╚══════════════════════════════════════════════════════════════╝
EOF

    validate_arguments "$@"
    mount_wgp
    check_symlinks
    check_extras

    show_summary

    # Si aucun changement n'est nécessaire, quitter
    if ! $CHANGES_NEEDED; then
        echo ""
        echo "=== Aucun changement nécessaire ==="
        echo "Ce WGP est déjà compatible avec la nouvelle architecture."
        #leanup() sera appelé par trap EXIT
        exit 0
    fi

    # Demander confirmation avant d'extraire et repackager
    if ask_yes_no "Continuer le repackaging?"; then
        extract_and_fix
        repack_wgp

        echo ""
        echo "=== Succès ==="
        echo "WGP repackagé: $OUTPUT_FILE"
    else
        echo ""
        echo "Annulation."
        exit 0
    fi
}

# Lancement du script
main "$@"
