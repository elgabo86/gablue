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

# Sélectionne un élément dans une liste via kdialog ou console
select_from_list() {
    local title="$1"
    shift
    local items=("$@")

    if command -v kdialog &> /dev/null; then
        kdialog --radiolist "$title" "${items[@]}"
    else
        local i=0
        echo "$title:"
        while [ $i -lt ${#items[@]} ]; do
            echo "  $((i/3+1)). ${items[$i+1]}"
            i=$((i + 3))
        done
        read -p "Entrez le numéro: " -r
        local idx=$((REPLY - 1))
        echo "${items[$((idx * 3))]}"
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

    # Sinon, proposer les autres options
    local TEMP_OPTION
    if command -v kdialog &> /dev/null; then
        TEMP_OPTION=$(select_from_list "Où extraire le WGP?" \
            "cache" "$CACHE_DIR (conseillé)" "on" \
            "local" "$WGP_DIR (même dossier)" "off")
    else
        echo "Où extraire le WGP?"
        echo "  1. $CACHE_DIR (conseillé)"
        echo "  2. $WGP_DIR (même dossier)"
        read -p "Entrez le numéro (default: 1): " -r
        case "$REPLY" in
            ""|1) TEMP_OPTION="cache" ;;
            2) TEMP_OPTION="local" ;;
        esac
    fi

    case "$TEMP_OPTION" in
        "cache")
            mkdir -p "$CACHE_DIR"
            TEMP_DIR="$CACHE_DIR/${GAME_NAME}_repack_$$"
            ;;
        "local")
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

# Extrait l'icône depuis un .exe symlinké pour créer .icon.png dans le WGP
extract_icon_from_symlink_exe() {
    # Vérifier si .icon.png existe déjà
    [ -f "$TEMP_DIR/.icon.png" ] && return 0

    # Lire le fichier .launch
    if [ ! -f "$TEMP_DIR/.launch" ]; then
        return 0
    fi

    local EXE_PATH
    EXE_PATH=$(cat "$TEMP_DIR/.launch" | tr -d '\r' | xargs)

    # Chemin complet de l'exécutable dans le WGP extrait
    local WGP_EXE_PATH="$TEMP_DIR/$EXE_PATH"

    # Si ce n'est pas un symlink, pas besoin d'extraire l'icône ici
    [ ! -L "$WGP_EXE_PATH" ] && return 0

    echo ""
    echo "Détection d'un symlink .exe, tentative d'extraction d'icône..."

    # Suivre le symlink pour trouver le vrai .exe
    local SYMLINK_TARGET
    SYMLINK_TARGET=$(readlink "$WGP_EXE_PATH")

    # Si le symlink pointe vers un chemin absolu (hors du WGP)
    if [[ "$SYMLINK_TARGET" == /* ]]; then
        if [ ! -f "$SYMLINK_TARGET" ]; then
            echo "Le .exe cible n'existe plus: $SYMLINK_TARGET"
            echo "Impossible d'extraire l'icône."
            return 1
        fi

        echo "Extraction de l'icône depuis: $SYMLINK_TARGET"

        # Extraire l'icône depuis l'exécutable réel
        TEMP_ICO=$(mktemp -d)
        wrestool -x -t14 "$SYMLINK_TARGET" -o "$TEMP_ICO" 2>/dev/null

        # Trouver les fichiers .ico extraits
        local ico_files
        ico_files=$(find "$TEMP_ICO" -name "*.ico" 2>/dev/null)

        if [ -z "$ico_files" ]; then
            rm -rf "$TEMP_ICO"
            echo "Aucune icône trouvée dans l'exécutable."
            return 1
        fi

        # Convertir les .ico en .png
        icotool --extract --output="$TEMP_ICO" $ico_files 2>/dev/null

        # Trouver le plus grand PNG
        local biggest_png
        biggest_png=$(find "$TEMP_ICO" -name "*.png" -exec ls -S {} + 2>/dev/null | head -n1)

        if [ -n "$biggest_png" ] && file "$biggest_png" 2>/dev/null | grep -q "PNG image"; then
            cp "$biggest_png" "$TEMP_DIR/.icon.png"
            echo "Icône extraite et sauvegardée: .icon.png"
        else
            echo "Impossible d'extraire une icône valide."
        fi

        rm -rf "$TEMP_ICO"
    fi
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

    # Extraire l'icône depuis le .exe symlinké si nécessaire
    extract_icon_from_symlink_exe

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

        # Fenêtre d'information KDE
        if command -v kdialog &> /dev/null; then
            kdialog --title "Rien à faire" --msgbox "Ce WGP est déjà compatible avec la nouvelle architecture."
        fi

        # cleanup() sera appelé par trap EXIT
        exit 0
    fi

    # Configuration du niveau de compression (seulement si des changements sont nécessaires)
    configure_compression

    # Continuer le repackaging sans confirmation
    extract_and_fix
    repack_wgp
    show_success
}

# Lancement du script
main "$@"
