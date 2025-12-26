#!/bin/bash

# Vérifier qu'un fichier est fourni en argument
if [ $# -eq 0 ]; then
    echo "Usage: $0 <fichier_wgp>"
    exit 1
fi

WGPACK_FILE="$(realpath "$1")"

# Vérifier que le fichier existe
if [ ! -f "$WGPACK_FILE" ]; then
    echo "Erreur: le fichier '$WGPACK_FILE' n'existe pas"
    exit 1
fi

# Nom du paquet (sans extension)
GAME_NAME=$(basename "$WGPACK_FILE" .wgp)

# Créer le dossier de sortie avec le même nom que le paquet
OUTPUT_DIR="./$GAME_NAME"

echo "=== Extraction du paquet: $GAME_NAME ==="
echo "Fichier source: $WGPACK_FILE"
echo "Dossier de sortie: $OUTPUT_DIR"
echo ""

# Demander confirmation avant de continuer
if command -v kdialog &> /dev/null; then
    kdialog --yesno "Voulez-vous extraire le contenu de:\n\n$(basename "$WGPACK_FILE")\n\ndans le dossier:\n$OUTPUT_DIR" --yes-label "Extraire" --no-label "Annuler"
    if [ $? -ne 0 ]; then
        echo "Extraction annulée"
        exit 0
    fi
else
    read -p "Extraire dans $OUTPUT_DIR ? (o/N): " CONFIRM
    if [[ ! "$CONFIRM" =~ ^[oOyY]$ ]]; then
        echo "Extraction annulée"
        exit 0
    fi
fi

# Vérifier si le dossier existe déjà
if [ -d "$OUTPUT_DIR" ]; then
    echo "Attention: le dossier '$OUTPUT_DIR' existe déjà"
    if command -v kdialog &> /dev/null; then
        kdialog --warningyesno "Le dossier existe déjà.\nSon contenu sera écrasé.\n\nContinuer ?" --yes-label "Oui" --no-label "Non"
        if [ $? -ne 0 ]; then
            echo "Extraction annulée"
            exit 0
        fi
        # Supprimer l'ancien dossier
        rm -rf "$OUTPUT_DIR"
    else
        read -p "Supprimer et remplacer ? (o/N): " REPLACE
        if [[ ! "$REPLACE" =~ ^[oOyY]$ ]]; then
            echo "Extraction annulée"
            exit 0
        fi
        rm -rf "$OUTPUT_DIR"
    fi
fi

# Créer le dossier temporaire pour l'extraction
TEMP_DIR="$OUTPUT_DIR.tmp"

echo ""
echo "=== Extraction en cours ==="

if command -v kdialog &> /dev/null; then
    # Fenêtre d'attente avec bouton Annuler
    kdialog --msgbox "Extraction en cours...\nAppuyez sur Annuler pour arrêter" --ok-label "Annuler" >/dev/null &
    KDIALOG_PID=$!

    # Lancer unsquashfs
    unsquashfs -f -d "$TEMP_DIR" -no-xattrs "$WGPACK_FILE" &
    UNSQUASH_PID=$!

    # Surveiller tant que unsquashfs tourne
    while kill -0 $UNSQUASH_PID 2>/dev/null; do
        # Si kdialog fermé = annulation
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

    # Fermer kdialog si encore ouvert
    kill $KDIALOG_PID 2>/dev/null

    # Vérifier le code de retour
    wait $UNSQUASH_PID
    EXIT_CODE=$?

    if [ $EXIT_CODE -ne 0 ]; then
        echo "Erreur lors de l'extraction"
        rm -rf "$TEMP_DIR"
        exit 1
    fi
else
    # Mode console
    echo "Extraction de $WGPACK_FILE..."
    unsquashfs -f -d "$TEMP_DIR" -no-xattrs "$WGPACK_FILE"

    if [ $? -ne 0 ]; then
        echo "Erreur lors de l'extraction"
        rm -rf "$TEMP_DIR"
        exit 1
    fi
fi

# Renommer le dossier temporaire en dossier final
mv "$TEMP_DIR" "$OUTPUT_DIR"

echo ""
echo "=== Extraction terminée avec succès ==="
echo "Dossier: $OUTPUT_DIR"

# Gestion des sauvegardes depuis le fichier .savepath
SAVE_FILE="$OUTPUT_DIR/.savepath"
if [ -f "$SAVE_FILE" ]; then
    # Lire ligne par ligne (par dossier/fichier)
    while IFS= read -r SAVE_REL_PATH; do
        if [ -n "$SAVE_REL_PATH" ]; then
            SAVE_ITEM_NAME=$(basename "$SAVE_REL_PATH")
            OUTPUT_SAVE_ITEM="$OUTPUT_DIR/$SAVE_REL_PATH"

            # Chemin vers le dossier de saves externe avec la structure complète
            WINDOWS_HOME="$HOME/Windows/UserData"
            SAVES_BASE="$WINDOWS_HOME/$USER/AppData/Local/LocalSaves"
            SAVES_DIR="$SAVES_BASE/$GAME_NAME"

            # Vérifier si c'est un fichier dans le dossier de sauvegardes
            FINAL_SAVE_ITEM="$SAVES_DIR/$SAVE_REL_PATH"
            if [ -f "$FINAL_SAVE_ITEM" ]; then
                echo ""
                echo "Copie du fichier de sauvegardes ($SAVE_ITEM_NAME) depuis $FINAL_SAVE_ITEM..."
                mkdir -p "$(dirname "$OUTPUT_SAVE_ITEM")"
                cp "$FINAL_SAVE_ITEM" "$OUTPUT_SAVE_ITEM"
                echo "Fichier de sauvegardes copié avec succès."
            # Sinon vérifier si c'est un dossier
            elif [ -d "$FINAL_SAVE_ITEM" ]; then
                echo ""
                echo "Copie du dossier de sauvegardes ($SAVE_ITEM_NAME) depuis $FINAL_SAVE_ITEM..."
                mkdir -p "$OUTPUT_SAVE_ITEM"
                cp -r "$FINAL_SAVE_ITEM/"* "$OUTPUT_SAVE_ITEM/" 2>/dev/null
                echo "Dossier de sauvegardes copié avec succès."
            else
                echo ""
                echo "Avertissement: l'élément de sauvegardes n'existe pas: $FINAL_SAVE_ITEM"
            fi
        fi
    done < "$SAVE_FILE"
fi

# Gestion des fichiers et dossiers d'options depuis le fichier .keeppath
KEEPPATH_FILE="$OUTPUT_DIR/.keeppath"
if [ -f "$KEEPPATH_FILE" ]; then
    # Lire ligne par ligne (par fichier/dossier)
    while IFS= read -r KEEP_REL_PATH; do
        if [ -n "$KEEP_REL_PATH" ]; then
            KEEP_ITEM_NAME=$(basename "$KEEP_REL_PATH")
            OUTPUT_KEEP_ITEM="$OUTPUT_DIR/$KEEP_REL_PATH"

            # Chemin vers le dossier de saves externe avec la structure complète
            WINDOWS_HOME="$HOME/Windows/UserData"
            SAVES_BASE="$WINDOWS_HOME/$USER/AppData/Local/LocalSaves"
            SAVES_DIR="$SAVES_BASE/$GAME_NAME"

            # Vérifier si c'est un fichier dans le dossier de sauvegardes
            FINAL_KEEP_ITEM="$SAVES_DIR/$KEEP_REL_PATH"
            if [ -f "$FINAL_KEEP_ITEM" ]; then
                echo ""
                echo "Copie du fichier d'options ($KEEP_ITEM_NAME) depuis $FINAL_KEEP_ITEM..."
                mkdir -p "$(dirname "$OUTPUT_KEEP_ITEM")"
                cp "$FINAL_KEEP_ITEM" "$OUTPUT_KEEP_ITEM"
                echo "Fichier d'options copié avec succès."
            # Sinon vérifier si c'est un dossier
            elif [ -d "$FINAL_KEEP_ITEM" ]; then
                echo ""
                echo "Copie du dossier d'options ($KEEP_ITEM_NAME) depuis $FINAL_KEEP_ITEM..."
                mkdir -p "$OUTPUT_KEEP_ITEM"
                cp -r "$FINAL_KEEP_ITEM/"* "$OUTPUT_KEEP_ITEM/" 2>/dev/null
                echo "Dossier d'options copié avec succès."
            else
                echo ""
                echo "Avertissement: l'élément d'options n'existe pas: $FINAL_KEEP_ITEM"
            fi
        fi
    done < "$KEEPPATH_FILE"
fi

# Supprimer les fichiers temporaires
rm -f "$OUTPUT_DIR/.launch"
rm -f "$OUTPUT_DIR/.args"
rm -f "$OUTPUT_DIR/.fix"
rm -f "$OUTPUT_DIR/.savepath"
rm -f "$OUTPUT_DIR/.keeppath"
rm -rf "$OUTPUT_DIR/.keep"
rm -rf "$OUTPUT_DIR/.save"

# Afficher le nombre de fichiers extraits
FILE_COUNT=$(find "$OUTPUT_DIR" -type f | wc -l)
echo "Fichiers extraits: $FILE_COUNT"

# Fenêtre de succès
if command -v kdialog &> /dev/null; then
    MSG="Paquet extrait avec succès !\n\n"
    MSG+="Fichier: $(basename "$WGPACK_FILE")\n"
    MSG+="Dossier: $OUTPUT_DIR\n"
    MSG+="Fichiers: $FILE_COUNT"
    kdialog --title "Succès" --msgbox "$MSG"
fi
