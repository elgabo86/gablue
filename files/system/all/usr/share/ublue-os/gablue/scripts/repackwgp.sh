#!/bin/bash

################################################################################
# repackwgp.sh - Script de repackaging de paquets WGP
#
# Ce script permet de mettre à jour un vieux WGP vers la nouvelle architecture
# (symlinks /tmp/wgp-saves au lieu de chemins utilisateur durs) et de modifier
# l'icône custom.
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
COMPRESS_CMD=""
COMPRESS_LEVEL_DEFAULT="15"

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

#======================================
# Fonctions d'affichage et utilitaires
#======================================

# Pose une question oui/non via kdialog ou console
ask_yes_no() {
    local prompt="$1"
    local yes_label="${2:-Oui}"
    local no_label="${3:-Non}"

    if command -v kdialog &> /dev/null; then
        kdialog --yesno "$prompt" --yes-label "$yes_label" --no-label "$no_label"
    else
        read -p "$prompt (o/N): " -r
        [[ "$REPLY" =~ ^[oOyY]$ ]]
    fi
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
# Fonctions de configuration
#======================================

# Configure le niveau de compression
configure_compression() {
    local DEFAULT_LEVEL="$COMPRESS_LEVEL_DEFAULT"

    echo ""
    echo "=== Configuration de la compression ==="

    local INPUT
    if command -v kdialog &> /dev/null; then
        INPUT=$(kdialog --inputbox "Niveau de compression zstd (1-19):\n1 = le plus rapide à lire\n19 = la plus petite taille\n0 = pas de compression" "$DEFAULT_LEVEL")
    else
        echo "Niveau de compression zstd (1-19):"
        echo "  1 = le plus rapide à lire"
        echo "  19 = la plus petite taille"
        echo "  0 = pas de compression"
        read -r
        INPUT="${REPLY:-$DEFAULT_LEVEL}"
    fi

    case "$INPUT" in
        "0"|"none"|"non")
            COMPRESS_CMD=""
            ;;
        [1-9]|1[0-9])
            COMPRESS_CMD="-comp zstd -Xcompression-level $INPUT"
            ;;
        *)
            error_exit "Choix invalide"
            ;;
    esac
}

# Sélectionne le dossier temporaire d'extraction
select_temp_dir() {
    echo ""
    echo "=== Sélection du dossier temporaire ==="

    local CACHE_DIR="$HOME/.cache/repackwgp"
    local WGP_DIR="$(dirname "$WGPACK_FILE")"

    # Calculer la taille du WGP en Mo
    local WGP_SIZE_KB=$(du -s "$WGPACK_FILE" | cut -f1)
    local WGP_SIZE_MB=$((WGP_SIZE_KB / 1024))

    # Calculer la RAM libre disponible en Mo
    local MEM_AVAILABLE_KB=$(grep MemAvailable /proc/meminfo | awk '{print $2}')
    local MEM_AVAILABLE_MB=$((MEM_AVAILABLE_KB / 1024))

    # Besoin estimé : taille du WGP x 1.6 (extraction décompressée) + 60% de marge
    local REQUIRED_MB=$((WGP_SIZE_MB * 8 / 5))

    # Proposer /tmp si la RAM est suffisante
    if [ $MEM_AVAILABLE_MB -gt $REQUIRED_MB ]; then
        if command -v kdialog &> /dev/null; then
            if kdialog --yesno "Utiliser /tmp (RAM) pour l'extraction?\\n\\nTaille WGP: ${WGP_SIZE_MB} Mo\\nBesoin estimé: ${REQUIRED_MB} Mo\\nRAM libre: ${MEM_AVAILABLE_MB} Mo" --yes-label "Oui" --no-label "Choisir autre"; then
                TEMP_DIR="/tmp/${GAME_NAME}_repack_$$"
                echo "Dossier temporaire: $TEMP_DIR"
                return
            fi
        else
            read -p "Utiliser /tmp (RAM) pour l'extraction? (WGP: ${WGP_SIZE_MB} Mo, besoin: ${REQUIRED_MB} Mo, RAM dispo: ${MEM_AVAILABLE_MB} Mo) [O/n]: " -r
            [[ ! "$REPLY" =~ ^[nN]$ ]] && {
                TEMP_DIR="/tmp/${GAME_NAME}_repack_$$"
                echo "Dossier temporaire: $TEMP_DIR"
                return
            }
        fi
    fi

    # Sinon, proposer les autres options (console only)
    echo "Où extraire le WGP?"
    echo "  1. $CACHE_DIR (conseillé)"
    echo "  2. $WGP_DIR (même dossier)"
    read -p "Entrez le numéro (default: 1): " -r
    case "$REPLY" in
        ""|1)
            mkdir -p "$CACHE_DIR"
            TEMP_DIR="$CACHE_DIR/${GAME_NAME}_repack_$$"
            ;;
        2)
            TEMP_DIR="$WGP_DIR/${GAME_NAME}_repack_$$"
            ;;
        *)
            error_exit "Choix invalide"
            ;;
    esac

    echo "Dossier temporaire: $TEMP_DIR"
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
# Fonctions de gestion de l'icône
#======================================

# Extrait la plus grande icône PNG depuis un .exe Windows
extract_icon_from_exe() {
    local exe_path="$1"
    local output_dir="$2"

    # Extraire les groupes icônes depuis l'exécutable
    wrestool -x -t14 "$exe_path" -o "$output_dir" 2>/dev/null || return 1

    # Trouver les fichiers .ico extraits
    local ico_files
    ico_files=$(find "$output_dir" -name "*.ico" 2>/dev/null)
    [ -z "$ico_files" ] && return 1

    # Convertir les .ico en .png
    icotool --extract --output="$output_dir" $ico_files 2>/dev/null

    # Retourner le plus grand PNG
    local biggest_png=$(find "$output_dir" -name "*.png" -exec ls -S {} + 2>/dev/null | head -n1)

    if [ -n "$biggest_png" ] && file "$biggest_png" | grep -q "PNG image"; then
        echo "$biggest_png"
        return 0
    fi
    return 1
}

# Convertit un fichier d'icône (.png, .ico, etc.) vers un PNG standardisé
process_icon_to_standard() {
    local input_file="$1"
    local output_file="$2"

    case "${input_file,,}" in
        *.png)
            # Copier le PNG
            cp "$input_file" "$output_file"
            return 0
            ;;
        *.ico)
            # Convertir .ico en PNG
            TEMP_ICO=$(mktemp -d)
            icotool --extract --output="$TEMP_ICO" "$input_file" 2>/dev/null
            BIGGEST_PNG=$(find "$TEMP_ICO" -name "*.png" -exec ls -S {} + 2>/dev/null | head -n1)
            if [ -n "$BIGGEST_PNG" ]; then
                cp "$BIGGEST_PNG" "$output_file"
                rm -rf "$TEMP_ICO"
                return 0
            fi
            rm -rf "$TEMP_ICO"
            return 1
            ;;
        *)
            # Format non supporté
            return 1
            ;;
    esac
}

# Configure une icône custom pour le WGP
configure_custom_icon() {
    echo ""
    echo "=== Gestion de l'icône ==="

    # Option de conserver l'icône de l'exe de base
    ask_yes_no "Voulez-vous conserver l'icône de l'exe de base du jeu ?\n\nSi non, vous pourrez choisir une nouvelle icône (.exe, .png, .ico)" "Oui" "Non (choisir icône custom)" && {
        rm -f "$TEMP_DIR/.icon.png"
        echo "Icône de l'exe conservée"
        return 0
    }

    while true; do
        # Sélectionner un fichier d'icône
        local icon_file=""
        if command -v kdialog &> /dev/null; then
            icon_file=$(kdialog --getopenfilename "$TEMP_DIR" "*.exe *.png *.ico *.PNG *.ICO | Images Windows (.exe, .png, .ico)")
        else
            read -p "Entrez le chemin relatif (depuis le dossier extraits): " -r
            if [ -n "$REPLY" ]; then
                icon_file="$TEMP_DIR/$REPLY"
            fi
        fi

        [ -z "$icon_file" ] && return 0

        # Traiter selon le type de fichier
        local icon_name=$(basename "$icon_file")

        case "${icon_name,,}" in
            *.exe)
                echo "Extraction de l'icône depuis: $icon_name"
                TEMP_ICO=$(mktemp -d)
                local extracted_png=$(extract_icon_from_exe "$icon_file" "$TEMP_ICO")
                if [ -n "$extracted_png" ]; then
                    cp "$extracted_png" "$TEMP_DIR/.icon.png"
                    rm -rf "$TEMP_ICO"
                    echo "Icône extraite avec succès: .icon.png"
                    return 0
                else
                    rm -rf "$TEMP_ICO"
                    echo "Erreur: aucune icône trouvée dans l'exécutable"
                fi
                ;;
            *.png)
                if process_icon_to_standard "$icon_file" "$TEMP_DIR/.icon.png"; then
                    echo "Icône PNG copiée avec succès: .icon.png"
                    return 0
                else
                    echo "Erreur: impossible de copier l'icône PNG"
                fi
                ;;
            *.ico)
                if process_icon_to_standard "$icon_file" "$TEMP_DIR/.icon.png"; then
                    echo "Icône ICO convertie avec succès: .icon.png"
                    return 0
                else
                    echo "Erreur: impossible de convertir l'icône ICO"
                fi
                ;;
            *)
                echo "Erreur: format non supporté ($icon_name)"
                ;;
        esac

        # Demander si l'utilisateur veut renouveler
        if ! ask_yes_no "Voulez-vous choisir une autre icône ?"; then
            break
        fi
    done
}

#======================================
# Fonctions d'extraction et correction
#======================================

# Extrait le squashfs avec interface graphique
extract_with_dialog() {
    # Fenêtre d'attente avec bouton Annuler
    kdialog --msgbox "Extraction en cours...\nAppuyez sur Annuler pour arrêter" --ok-label "Annuler" >/dev/null &
    local KDIALOG_PID=$!

    # Lancer unsquashfs en arrière-plan
    unsquashfs -f -d "$TEMP_DIR" -no-xattrs "$WGPACK_FILE" &
    local UNSQUASH_PID=$!

    # Surveiller tant que unsquashfs tourne
    while kill -0 $UNSQUASH_PID 2>/dev/null; do
        # Annulation si kdialog fermé
        if ! kill -0 $KDIALOG_PID 2>/dev/null; then
            kill -9 $UNSQUASH_PID 2>/dev/null
            pkill -9 unsquashfs 2>/dev/null
            rm -rf "$TEMP_DIR"
            echo ""
            echo "Extraction annulée"
            exit 0
        fi
        sleep 0.2
    done

    # Fermer kdialog
    kill $KDIALOG_PID 2>/dev/null

    # Vérifier le code de retour
    wait $UNSQUASH_PID
}

# Extrait le squashfs en mode console
extract_console() {
    echo "Extraction de $WGPACK_FILE..."
    unsquashfs -f -d "$TEMP_DIR" -no-xattrs "$WGPACK_FILE"
}

extract_and_fix() {
    # Démonter le mount
    fusermount -u "$MOUNT_DIR" 2>/dev/null || true
    rm -rf "$MOUNT_DIR"
    MOUNT_DIR=""

    # Sélectionner le dossier temporaire
    select_temp_dir
    mkdir -p "$TEMP_DIR"

    echo ""
    echo "=== Extraction du WGP ==="
    echo "Dossier temporaire: $TEMP_DIR"

    local EXIT_CODE
    if command -v kdialog &> /dev/null; then
        extract_with_dialog
        EXIT_CODE=$?
    else
        extract_console
        EXIT_CODE=$?
    fi

    if [ $EXIT_CODE -ne 0 ]; then
        echo "Erreur lors de l'extraction"
        exit 1
    fi

    # Corriger les symlinks si nécessaire
    if $CHANGES_NEEDED; then
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
    fi

    # Proposer de modifier l'icône custom
    configure_custom_icon
}

# Vérifie si le fichier de sortie existe déjà et demande confirmation
check_existing_output() {
    [ ! -f "$OUTPUT_FILE" ] && return 0

    echo ""
    echo "Attention: le fichier de sortie existe déjà"
    echo "Fichier: $OUTPUT_FILE"

    if ask_yes_no "Le fichier de sortie existe déjà.\nVoulez-vous l'écraser ?"; then
        echo "Suppression de l'ancien fichier: $OUTPUT_FILE"
        rm -f "$OUTPUT_FILE"
        return 0
    fi

    # L'utilisateur ne veut pas écraser → annuler et nettoyer
    echo ""
    echo "Annulation demandée."
    cleanup
    exit 0
}

#======================================
# Fonctions de création du squashfs
#======================================

# Crée le squashfs avec interface graphique (annulation possible)
create_squashfs_with_dialog() {
    # Fenêtre informative
    kdialog --msgbox "Recompression en cours pour: $GAME_NAME" --ok-label "Annuler" >/dev/null &
    local KDIALOG_PID=$!

    # Lancer mksquashfs en arrière-plan
    mksquashfs "$TEMP_DIR" "$OUTPUT_FILE" $COMPRESS_CMD -nopad -all-root &
    local MKSQUASH_PID=$!

    # Surveiller jusqu'à ce que mksquashfs se termine
    while kill -0 $MKSQUASH_PID 2>/dev/null; do
        # Annulation si kdialog fermé
        if ! kill -0 $KDIALOG_PID 2>/dev/null; then
            kill -9 $MKSQUASH_PID 2>/dev/null
            pkill -9 mksquashfs 2>/dev/null
            rm -f "$OUTPUT_FILE"
            echo ""
            echo "Recompression annulée"
            exit 0
        fi
        sleep 0.2
    done

    # Fermer kdialog
    kill $KDIALOG_PID 2>/dev/null

    # Vérifier le code de retour
    wait $MKSQUASH_PID
    local EXIT_CODE=$?

    if [ $EXIT_CODE -ne 0 ]; then
        echo "Erreur lors de la création du squashfs"
        exit 1
    fi
}

# Crée le squashfs en mode console
create_squashfs_console() {
    echo "Recompression de $WGPACK_FILE en cours..."
    mksquashfs "$TEMP_DIR" "$OUTPUT_FILE" $COMPRESS_CMD -nopad -all-root

    if [ $? -ne 0 ]; then
        echo "Erreur lors de la création du squashfs"
        exit 1
    fi
}

repack_wgp() {
    echo ""
    echo "=== Création du WGP repackagé ==="

    local EXIT_CODE
    if command -v kdialog &> /dev/null; then
        create_squashfs_with_dialog
        EXIT_CODE=$?
    else
        create_squashfs_console
        EXIT_CODE=$?
    fi

    return $EXIT_CODE
}

#======================================
# Fonctions d'affichage final
#======================================

show_success() {
    local SIZE_BEFORE=$(du -s "$WGPACK_FILE" | cut -f1)
    local SIZE_BEFORE_MB=$(echo "scale=1; $SIZE_BEFORE / 1024" | bc)
    local SIZE_AFTER=$(du -s "$OUTPUT_FILE" | cut -f1)
    local SIZE_AFTER_MB=$(echo "scale=1; $SIZE_AFTER / 1024" | bc)

    echo ""
    echo "=== Succès ==="
    echo "WGP repackagé: $OUTPUT_FILE"
    echo "Taille avant: ${SIZE_BEFORE_MB} Mo"
    echo "Taille après: ${SIZE_AFTER_MB} Mo"

    # Fenêtre de succès KDE
    if command -v kdialog &> /dev/null; then
        local MSG="WGP repackagé avec succès!\n\n"
        MSG+="Fichier: $OUTPUT_FILE\n"
        MSG+="Taille avant: ${SIZE_BEFORE_MB} Mo\n"
        MSG+="Taille après: ${SIZE_AFTER_MB} Mo"

        kdialog --title "Succès" --msgbox "$MSG"
    fi
}

#======================================
# Fonction principale
#======================================

main() {
    cat << 'EOF'
╔══════════════════════════════════════════════════════════════╗
║                    Repackaging de WGP                       ║
╚══════════════════════════════════════════════════════════════╝
EOF

    validate_arguments "$@"

    mount_wgp
    check_symlinks
    check_extras

    # Si aucun fix n'est nécessaire, confirmer quand même le repack
    if ! $CHANGES_NEEDED; then
        echo ""
        echo "Ce WGP est déjà compatible."

        # Message plus détaillé avec kdialog
        local CONFIRM_MSG="Ce WGP n'a pas besoin de fix.\n\n"
        CONFIRM_MSG+="Vous pouvez quand même le repacker pour :\n"
        CONFIRM_MSG+="- Modifier le niveau de compression\n"
        CONFIRM_MSG+="- Changer l'icône custom"

        if ! ask_yes_no "$CONFIRM_MSG" "Repacker" "Annuler"; then
            echo "Annulation demandée."
            cleanup
            exit 0
        fi
    fi

    # Configuration de la compression
    configure_compression

    # Vérifier si le fichier de sortie existe déjà
    check_existing_output

    # Extraction et correction
    extract_and_fix

    # Création du WGP repackagé
    repack_wgp
    show_success
}

# Lancement du script
main "$@"
