#!/bin/bash

# Vérifier qu'un dossier est fourni en argument
if [ $# -eq 0 ]; then
    echo "Usage: $0 <dossier_du_jeu>"
    exit 1
fi

GAME_DIR="$(realpath "$1")"

# Vérifier que le dossier existe
if [ ! -d "$GAME_DIR" ]; then
    echo "Erreur: le dossier '$GAME_DIR' n'existe pas"
    exit 1
fi

# Nom du paquet (basé sur le nom du dossier)
GAME_NAME="$(basename "$GAME_DIR")"
WGPACK_NAME="$(dirname "$GAME_DIR")/${GAME_NAME}.wgp"

echo "=== Création du paquet pour: $GAME_NAME ==="
echo "Dossier source: $GAME_DIR"
echo ""

# Demander le niveau de compression zstd
DEFAULT_LEVEL=15
if command -v kdialog &> /dev/null; then
    INPUT=$(kdialog --inputbox "Niveau de compression zstd (1-19):\n1 = le plus rapide à lire\n19 = la plus petite taille\n0 = pas de compression" "$DEFAULT_LEVEL")
else
    echo "kdialog non disponible, utilisation du mode console"
    echo "Niveau de compression zstd (1-19):"
    echo "  1 = le plus rapide à lire"
    echo "  19 = la plus petite taille"
    echo "  0 = pas de compression"
    read -p "Niveau [$DEFAULT_LEVEL]: " INPUT
    INPUT=${INPUT:-$DEFAULT_LEVEL}
fi

case "$INPUT" in
    "0"|"none"|"non")
        COMPRESS_CMD=""
        SUFFIX=""
        ;;
    [1-9]|1[0-9])
        COMPRESS_CMD="-comp zstd -Xcompression-level $INPUT"
        SUFFIX="_zstd$INPUT"
        ;;
    *)
        echo "Choix invalide"
        exit 1
        ;;
esac

echo ""
echo "=== Recherche des exécutables .exe ==="

# Scanner le dossier pour trouver les .exe
EXE_LIST=$(find "$GAME_DIR" -type f -iname "*.exe" 2>/dev/null)

if [ -z "$EXE_LIST" ]; then
    echo "Erreur: aucun fichier .exe trouvé dans $GAME_DIR"
    exit 1
fi

# Préparer la liste pour kdialog ou sélection console
COUNT=0
EXE_ARRAY=()
while IFS= read -r exe; do
    REL_PATH="${exe#$GAME_DIR/}"
    EXE_ARRAY+=("$exe")
    EXE_ARRAY+=("$REL_PATH")
    if [ "$COUNT" -eq 0 ]; then
        EXE_ARRAY+=("on") # Premier sélectionné par défaut
    else
        EXE_ARRAY+=("off")
    fi
    COUNT=$((COUNT + 1))
done <<< "$EXE_LIST"

if command -v kdialog &> /dev/null; then
    SELECTED=$(kdialog --radiolist "Sélectionnez l'exécutable principal:" "${EXE_ARRAY[@]}")
else
    echo "Exécutables trouvés:"
    i=0
    while IFS= read -r exe; do
        echo "  $((i+1)). ${exe#$GAME_DIR/}"
        i=$((i + 1))
    done <<< "$EXE_LIST"
    read -p "Entrez le numéro de l'exécutable: " SELECTED_NUM
    SELECTED_NUM=$((SELECTED_NUM - 1))
    SELECTED="${EXE_ARRAY[$((SELECTED_NUM * 3))]}"
fi

if [ ! -f "$SELECTED" ]; then
    echo "Erreur: exécutable non valide"
    exit 1
fi

# Chemin relatif de l'exécutable par rapport au dossier du jeu
EXE_REL_PATH="${SELECTED#$GAME_DIR/}"

echo ""
echo "Exécutable sélectionné: $EXE_REL_PATH"

# Créer le fichier .launch dans le dossier source
LAUNCH_FILE="$GAME_DIR/.launch"
echo "$EXE_REL_PATH" > "$LAUNCH_FILE"
echo "Fichier .launch créé: $LAUNCH_FILE"

echo ""
echo "=== Création du squashfs ==="

# Vérifier si le fichier .wgp existe déjà
if [ -f "$WGPACK_NAME" ]; then
    if command -v kdialog &> /dev/null; then
        kdialog --warningyesno "Le fichier $WGPACK_NAME existe déjà.\n\nVoulez-vous l'écraser ?"
        OVERWRITE=$?
    else
        echo "Attention: le fichier $WGPACK_NAME existe déjà."
        read -p "Voulez-vous l'écraser ? (o/N): " CONFIRM
        [[ "$CONFIRM" =~ ^[oOyY]$ ]] && OVERWRITE=0 || OVERWRITE=1
    fi

    if [ $OVERWRITE -ne 0 ]; then
        echo "Opération annulée."
        exit 0
    fi

    echo "Suppression de l'ancien fichier: $WGPACK_NAME"
    rm -f "$WGPACK_NAME"
fi

if command -v kdialog &> /dev/null; then
    # Fenêtre informative avec bouton Annuler personnalisé
    kdialog --msgbox "Compression en cours...\nAppuyez sur Annuler pour arrêter" --ok-label "Annuler" >/dev/null &
    KDIALOG_PID=$!

    # Lancer mksquashfs (avec sortie pour voir la progression)
    mksquashfs "$GAME_DIR" "$WGPACK_NAME" $COMPRESS_CMD -all-root &
    MKSQUASH_PID=$!

    # Surveiller tant que mksquashfs tourne
    while kill -0 $MKSQUASH_PID 2>/dev/null; do
        # Si kdialog fermé = annulation
        if ! kill -0 $KDIALOG_PID 2>/dev/null; then
            kill -9 $MKSQUASH_PID 2>/dev/null
            pkill -9 mksquashfs 2>/dev/null
            rm -f "$WGPACK_NAME"
            echo ""
            echo "Compression annulée"
            exit 0
        fi
        sleep 0.2
    done

    # Fermer kdialog si encore ouvert
    kill $KDIALOG_PID 2>/dev/null

    # Vérifier le code de retour
    wait $MKSQUASH_PID
    EXIT_CODE=$?

    if [ $EXIT_CODE -ne 0 ]; then
        echo "Erreur lors de la création du squashfs"
        exit 1
    fi
else
    # Mode console: simplement lancer mksquashfs
    echo "Création de $WGPACK_NAME en cours..."
    mksquashfs "$GAME_DIR" "$WGPACK_NAME" $COMPRESS_CMD -all-root

    if [ $? -ne 0 ]; then
        echo "Erreur lors de la création du squashfs"
        exit 1
    fi
fi

# Calcul des tailles
SIZE_BEFORE=$(du -s "$GAME_DIR" | cut -f1)
SIZE_BEFORE_GB=$(echo "scale=2; $SIZE_BEFORE / 1024 / 1024" | bc)
SIZE_AFTER=$(du -s "$WGPACK_NAME" | cut -f1)
SIZE_AFTER_GB=$(echo "scale=2; $SIZE_AFTER / 1024 / 1024" | bc)
COMPRESSION_RATIO=$(echo "scale=1; (1 - $SIZE_AFTER / $SIZE_BEFORE) * 100" | bc)

echo ""
echo "=== Paquet créé avec succès ==="
echo "Fichier: $WGPACK_NAME"
echo "Taille avant: ${SIZE_BEFORE_GB} Go"
echo "Taille après: ${SIZE_AFTER_GB} Go"
echo "Gain: ${COMPRESSION_RATIO}%"
echo "Exécutable: $EXE_REL_PATH"

# Fenêtre de succès
if command -v kdialog &> /dev/null; then
    kdialog --title "Succès" --msgbox "Paquet créé avec succès !\n\nFichier: $WGPACK_NAME\n\nTaille avant: ${SIZE_BEFORE_GB} Go\nTaille après: ${SIZE_AFTER_GB} Go\nGain: ${COMPRESSION_RATIO}%\n\nExécutable: $EXE_REL_PATH"
fi
