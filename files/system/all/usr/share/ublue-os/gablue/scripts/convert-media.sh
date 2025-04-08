#!/bin/bash

# Vérifie les arguments : fichier
if [ -z "$1" ] || [ ! -f "$1" ]; then
    echo "Erreur : spécifie un fichier média en argument"
    exit 1
fi

INPUT_FILE="$1"
INPUT_DIR=$(dirname "$INPUT_FILE")
INPUT_NAME=$(basename "$INPUT_FILE" | sed 's/\.[^.]*$//')
INPUT_EXT="${INPUT_FILE##*.}"

# Liste des formats possibles selon le type d'entrée, avec descriptions
if [ "$INPUT_EXT" = "ogv" ] || [ "$INPUT_EXT" = "mkv" ] || [ "$INPUT_EXT" = "mp4" ]; then
    # Vidéo : tous les formats sauf le source
    FORMATS=(
        "opus" "Opus (Audio) - Moderne, excellente qualité à faible bitrate, idéal pour streaming."
        "flac" "FLAC (Audio) - Sans perte, conserve la qualité originale, idéal pour l’archivage."
        "aac" "AAC (Audio) - Évolution du MP3, efficace et compatible (Apple, Android)."
        "mp3" "MP3 (Audio) - Historique, universellement compatible mais moins efficace."
        "ogg" "OGG Vorbis (Audio) - Open-source, bonne qualité, populaire dans le libre."
        "wav" "WAV (Audio) - Sans perte brut, volumineux, pour édition ou pros."
        "wma" "WMA (Audio) - Format Microsoft, décent mais en perte de vitesse."
        "mp4" "MP4 (Vidéo) - Universel, excellente compression, compatible partout."
        "mkv" "MKV (Vidéo) - Polyvalent, supporte plusieurs pistes, pour amateurs."
        "ogv" "OGV Theora (Vidéo) - Open-source, léger mais moins performant."
    )
else
    # Audio (y compris FLAC) : seulement audio, sauf le source
    FORMATS=(
        "opus" "Opus (Audio) - Moderne, excellente qualité à faible bitrate, idéal pour streaming."
        "flac" "FLAC (Audio) - Sans perte, conserve la qualité originale, idéal pour l’archivage."
        "aac" "AAC (Audio) - Évolution du MP3, efficace et compatible (Apple, Android)."
        "mp3" "MP3 (Audio) - Historique, universellement compatible mais moins efficace."
        "ogg" "OGG Vorbis (Audio) - Open-source, bonne qualité, populaire dans le libre."
        "wav" "WAV (Audio) - Sans perte brut, volumineux, pour édition ou pros."
        "wma" "WMA (Audio) - Format Microsoft, décent mais en perte de vitesse."
    )
fi

# Construit le menu en excluant le format source
MENU_OPTIONS=()
for ((i=0; i<${#FORMATS[@]}; i+=2)); do
    if [ "${FORMATS[$i]}" != "$INPUT_EXT" ]; then
        MENU_OPTIONS+=("${FORMATS[$i]}" "${FORMATS[$i+1]}")
    fi
done

# Demande le format de sortie via kdialog avec une largeur augmentée
OUTPUT_FORMAT=$(kdialog --geometry 600x400 --title "Convertir le média" --menu "Choisissez le format de sortie :" "${MENU_OPTIONS[@]}")
if [ $? -ne 0 ]; then
    echo "Annulé par l'utilisateur"
    exit 0
fi

# Définit le fichier de sortie
OUTPUT_FILE="$INPUT_DIR/$INPUT_NAME.$OUTPUT_FORMAT"

# Vérifie l'écrasement
if [ -f "$OUTPUT_FILE" ]; then
    kdialog --geometry 400x200 --title "Fichier existant" --yesno "Le fichier $OUTPUT_FILE existe déjà. Voulez-vous l'écraser ?"
    if [ $? -ne 0 ]; then
        echo "Conversion annulée : fichier non écrasé"
        exit 0
    fi
fi

# Paramètres de conversion selon le format
case "$OUTPUT_FORMAT" in
    "opus")
        QUALITY=$(kdialog --geometry 400x300 --title "Qualité Opus" --menu "Choisissez la qualité (recommandé : 128 kbps) :" \
            "64" "64 kbps" \
            "128" "128 kbps" \
            "256" "256 kbps")
        if [ $? -ne 0 ]; then
            echo "Annulé par l'utilisateur"
            exit 0
        fi
        CMD="ffmpeg -i \"$INPUT_FILE\" -c:a libopus -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
        ;;
    "flac")
        CMD="ffmpeg -i \"$INPUT_FILE\" -c:a flac -vn \"$OUTPUT_FILE\""
        ;;
    "aac")
        QUALITY=$(kdialog --geometry 400x300 --title "Qualité AAC" --menu "Choisissez la qualité (recommandé : 256 kbps) :" \
            "64" "64 kbps" \
            "128" "128 kbps" \
            "256" "256 kbps" \
            "vbr" "VBR (qualité variable, niveau 3)")
        if [ $? -ne 0 ]; then
            echo "Annulé par l'utilisateur"
            exit 0
        fi
        if [ "$QUALITY" = "vbr" ]; then
            CMD="ffmpeg -i \"$INPUT_FILE\" -c:a aac -vbr 3 -vn \"$OUTPUT_FILE\""
        else
            CMD="ffmpeg -i \"$INPUT_FILE\" -c:a aac -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
        fi
        ;;
    "mp3")
        QUALITY=$(kdialog --geometry 400x300 --title "Qualité MP3" --menu "Choisissez la qualité (recommandé : 192 ou 256 kbps) :" \
            "128" "128 kbps" \
            "192" "192 kbps" \
            "256" "256 kbps" \
            "vbr" "VBR (qualité variable)")
        if [ $? -ne 0 ]; then
            echo "Annulé par l'utilisateur"
            exit 0
        fi
        if [ "$QUALITY" = "vbr" ]; then
            CMD="ffmpeg -i \"$INPUT_FILE\" -c:a libmp3lame -q:a 2 -vn \"$OUTPUT_FILE\""
        else
            CMD="ffmpeg -i \"$INPUT_FILE\" -c:a libmp3lame -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
        fi
        ;;
    "ogg")
        QUALITY=$(kdialog --geometry 400x300 --title "Qualité OGG" --menu "Choisissez la qualité (recommandé : 6) :" \
            "3" "3 (≈96 kbps)" \
            "6" "6 (≈160 kbps)" \
            "9" "9 (≈320 kbps)")
        if [ $? -ne 0 ]; then
            echo "Annulé par l'utilisateur"
            exit 0
        fi
        CMD="ffmpeg -i \"$INPUT_FILE\" -c:a libvorbis -q:a $QUALITY -vn \"$OUTPUT_FILE\""
        ;;
    "wav")
        CMD="ffmpeg -i \"$INPUT_FILE\" -c:a pcm_s16le -vn \"$OUTPUT_FILE\""
        ;;
    "wma")
        QUALITY=$(kdialog --geometry 400x300 --title "Qualité WMA" --menu "Choisissez la qualité (recommandé : 128 kbps) :" \
            "64" "64 kbps" \
            "128" "128 kbps" \
            "192" "192 kbps")
        if [ $? -ne 0 ]; then
            echo "Annulé par l'utilisateur"
            exit 0
        fi
        CMD="ffmpeg -i \"$INPUT_FILE\" -c:a wmav2 -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
        ;;
    "ogv"|"mkv"|"mp4")
        # Vidéo uniquement pour les fichiers vidéo en entrée
        if [ "$INPUT_EXT" != "ogv" ] && [ "$INPUT_EXT" != "mkv" ] && [ "$INPUT_EXT" != "mp4" ]; then
            kdialog --error "Seuls les fichiers vidéo peuvent être convertis en $OUTPUT_FORMAT."
            exit 1
        fi
        CMD="ffmpeg -i \"$INPUT_FILE\" -y \"$OUTPUT_FILE\""
        ;;
    *)
        kdialog --error "Format non supporté : $OUTPUT_FORMAT"
        exit 1
        ;;
esac

# Exécute la conversion
eval "$CMD" 2>> /tmp/ffmpeg.log
FFMPEG_RESULT=$?

# Vérifie le résultat
if [ $FFMPEG_RESULT -eq 0 ]; then
    kdialog --geometry 400x200 --msgbox "Conversion terminée : $OUTPUT_FILE"
else
    kdialog --geometry 400x200 --error "Erreur lors de la conversion. Voir /tmp/ffmpeg.log pour plus de détails."
fi
