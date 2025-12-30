#!/bin/bash

################################################################################
# check-wgp-fix.sh - Liste les WGP qui ont besoin du fix
#
# Ce script liste tous les fichiers .wgp d'un dossier et indique ceux
# dont les symlinks de sauvegarde ne pointent pas vers /tmp/wgp-saves
################################################################################

#======================================
# Variables globales
#======================================
SCAN_DIR=""
MOUNT_DIR=""
REPACK_NEEDED=()
ALREADY_OK=()

#======================================
# Fonctions de nettoyage
#======================================

cleanup() {
    # Démonter le squashfs si monté
    if [ -n "$MOUNT_DIR" ] && mountpoint -q "$MOUNT_DIR" 2>/dev/null; then
        fusermount -u "$MOUNT_DIR" 2>/dev/null || true
        rm -rf "$MOUNT_DIR"
    fi
}

trap cleanup EXIT

error_exit() {
    echo "Erreur: $1" >&2
    exit 1
}

#======================================
# Fonctions de validation
#======================================

validate_arguments() {
    if [ $# -eq 0 ]; then
        SCAN_DIR="$PWD"
    else
        SCAN_DIR=$(realpath "$1")
    fi

    [ ! -d "$SCAN_DIR" ] && error_exit "Le dossier '$SCAN_DIR' n'existe pas"

    local wgp_count=$(find "$SCAN_DIR" -maxdepth 1 -type f -name "*.wgp" 2>/dev/null | wc -l)

    if [ "$wgp_count" -eq 0 ]; then
        echo ""
        echo "Aucun fichier .wgp trouvé dans: $SCAN_DIR"
        exit 0
    fi
}

#======================================
# Vérification d'un WGP
#======================================

check_wgp() {
    local wgp_file="$1"
    local wgp_name=$(basename "$wgp_file" .wgp)
    local needs_fix=false

    # Monter le WGP
    MOUNT_DIR=$(mktemp -d)
    squashfuse -r "$wgp_file" "$MOUNT_DIR" 2>/dev/null || {
        echo "ERREUR: Impossible de monter $wgp_name.wgp"
        return 1
    }

    # Vérifier le fichier .savepath
    if [ ! -f "$MOUNT_DIR/.savepath" ]; then
        # Pas de savepath = pas de fix nécessaire
        MOUNT_DIR=""
        return 0
    fi

    # Vérifier les symlinks
    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SYMLINK_PATH="$MOUNT_DIR/$SAVE_REL_PATH"

        if [ -L "$SYMLINK_PATH" ]; then
            local current_target
            current_target=$(readlink "$SYMLINK_PATH")

            if [[ "$current_target" != /tmp/wgp-saves/* ]]; then
                needs_fix=true
                break
            fi
        fi
    done < "$MOUNT_DIR/.savepath"

    cat <<< "$MOUNT_DIR"
    MOUNT_DIR=""
    MOUNT_DIR="/tmp/mount_check_$$_$wgp_name"
}

check_wgp_needs_fix() {
    local wgp_file="$1"
    local needs_fix=false

    local mount_temp=$(mktemp -d)
    squashfuse -r "$wgp_file" "$mount_temp" 2>/dev/null || {
        fusermount -u "$mount_temp" 2>/dev/null || true
        rm -rf "$mount_temp"
        return 1
    }

    if [ ! -f "$mount_temp/.savepath" ]; then
        fusermount -u "$mount_temp" 2>/dev/null || true
        rm -rf "$mount_temp"
        return 1
    fi

    while IFS= read -r SAVE_REL_PATH; do
        [ -z "$SAVE_REL_PATH" ] && continue

        local SYMLINK_PATH="$mount_temp/$SAVE_REL_PATH"

        if [ -L "$SYMLINK_PATH" ]; then
            local current_target
            current_target=$(readlink "$SYMLINK_PATH")

            if [[ "$current_target" != /tmp/wgp-saves/* ]]; then
                needs_fix=true
                break
            fi
        fi
    done < "$mount_temp/.savepath"

    fusermount -u "$mount_temp" 2>/dev/null || true
    rm -rf "$mount_temp"

    [ "$needs_fix" = true ]
}

#======================================
# Fonction principale
#======================================

main() {
    cat << 'EOF'
╔══════════════════════════════════════════════════════════════╗
║              Vérification des WGP pour fix                  ║
╚══════════════════════════════════════════════════════════════╝
EOF

    validate_arguments "$@"

    echo ""
    echo "Dossier analysé: $SCAN_DIR"
    echo ""

    if ! command -v squashfuse &> /dev/null; then
        error_exit "squashfuse n'est pas installé"
    fi

    # Trouver tous les WGP
    local total=0
    local needs_fix_count=0
    local ok_count=0

    while IFS= read -r -d '' wgp_file; do
        total=$((total + 1))
        local wgp_name=$(basename "$wgp_file" .wgp)

        echo -en "\r[$total] Analyse: $wgp_name..."

        if check_wgp_needs_fix "$wgp_file"; then
            REPACK_NEEDED+=("$wgp_name")
            needs_fix_count=$((needs_fix_count + 1))
            echo -en "\r[$total] \033[31m[NEED FIX]\033[0m $wgp_name\n"
        else
            ALREADY_OK+=("$wgp_name")
            ok_count=$((ok_count + 1))
            echo -en "\r[$total] \033[32m[OK]\033[0m       $wgp_name\n"
        fi
    done < <(find "$SCAN_DIR" -maxdepth 1 -type f -name "*.wgp" -print0 2>/dev/null | sort -z)

    echo ""
    echo "=== Résumé ==="
    echo "Total WGP:  $total"
    echo -e "Besoin fix: \033[31m$needs_fix_count\033[0m"
    echo -e "Déjà OK:    \033[32m$ok_count\033[0m"
    echo ""

    if [ $needs_fix_count -gt 0 ]; then
        echo "Fichiers WGP qui ont besoin du fix :"
        for wgp in "${REPACK_NEEDED[@]}"; do
            echo "  - $wgp.wgp"
        done
    fi
}

# Lancement du script
main "$@"
