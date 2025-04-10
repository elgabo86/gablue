#!/bin/bash

# Vérifie qu'au moins un fichier est fourni
if [ $# -eq 0 ]; then
 echo "Erreur : spécifie au moins un fichier média en argument"
 exit 1
fi

# Liste des formats possibles avec descriptions
FORMATS_AUDIO=(
 "opus" "Opus (Audio) - Moderne, excellente qualité à faible bitrate, idéal pour streaming."
 "flac" "FLAC (Audio) - Sans perte, conserve la qualité originale, idéal pour l’archivage."
 "aac" "AAC (Audio) - Évolution du MP3, efficace et compatible (Apple, Android)."
 "mp3" "MP3 (Audio) - Historique, universellement compatible mais moins efficace."
 "ogg" "OGG Vorbis (Audio) - Open-source, bonne qualité, populaire dans le libre."
 "wav" "WAV (Audio) - Sans perte brut, volumineux, pour édition ou pros."
 "wma" "WMA (Audio) - Format Microsoft, décent mais en perte de vitesse."
)

FORMATS_VIDEO=(
 "h264" "H.264 (Vidéo MP4) - Standard, excellente compatibilité, bonne compression."
 "h265" "H.265/HEVC (Vidéo MP4) - Successeur de H.264, meilleure compression, plus lent."
 "webm" "WebM (Vidéo) - Moderne pour le web, utilise VP8/VP9, très efficace."
 "mp4" "MP4 (Vidéo) - Universel, excellente compression, compatible partout."
 "mkv" "MKV (Vidéo) - Polyvalent, supporte plusieurs pistes, pour amateurs."
 "ogv" "OGV Theora (Vidéo) - Open-source, léger mais moins performant."
 "avi" "AVI (Vidéo) - Ancien, compatible mais volumineux."
 "reencapsulate" "Ré-encapsuler - Change le conteneur sans ré-encoder (rapide)."
)

# Détermine si tous les fichiers sont des vidéos
ALL_VIDEO=true
for INPUT_FILE in "$@"; do
 INPUT_EXT="${INPUT_FILE##*.}"
 if [ "$INPUT_EXT" != "ogv" ] && [ "$INPUT_EXT" != "mkv" ] && [ "$INPUT_EXT" != "mp4" ] && [ "$INPUT_EXT" != "webm" ] && [ "$INPUT_EXT" != "avi" ]; then
 ALL_VIDEO=false
 break
 fi
done

# Construit le menu en fonction du type de fichiers
MENU_OPTIONS=()
if [ "$ALL_VIDEO" = true ]; then
 for ((i=0; i<${#FORMATS_AUDIO[@]}; i+=2)); do
 MENU_OPTIONS+=("${FORMATS_AUDIO[$i]}" "${FORMATS_AUDIO[$i+1]}")
 done
 for ((i=0; i<${#FORMATS_VIDEO[@]}; i+=2)); do
 MENU_OPTIONS+=("${FORMATS_VIDEO[$i]}" "${FORMATS_VIDEO[$i+1]}")
 done
else
 for ((i=0; i<${#FORMATS_AUDIO[@]}; i+=2)); do
 MENU_OPTIONS+=("${FORMATS_AUDIO[$i]}" "${FORMATS_AUDIO[$i+1]}")
 done
fi

# Demande le format de sortie une seule fois
OUTPUT_FORMAT=$(kdialog --geometry 600x400 --title "Convertir les médias" --menu "Choisissez le format de sortie pour tous les fichiers :" "${MENU_OPTIONS[@]}")
if [ $? -ne 0 ]; then
 echo "Annulé par l'utilisateur"
 exit 0
fi

# Demande la qualité une seule fois (si applicable)
case "$OUTPUT_FORMAT" in
 "opus")
 QUALITY=$(kdialog --geometry 400x300 --title "Qualité Opus" --menu "Choisissez la qualité (recommandé : 128 kbps) :" \
 "64" "64 kbps" \
 "128" "128 kbps" \
 "256" "256 kbps")
 [ $? -ne 0 ] && exit 0
 ;;
 "aac")
 QUALITY=$(kdialog --geometry 400x300 --title "Qualité AAC" --menu "Choisissez la qualité (recommandé : 256 kbps) :" \
 "64" "64 kbps" \
 "128" "128 kbps" \
 "256" "256 kbps" \
 "vbr" "VBR (qualité variable, niveau 3)")
 [ $? -ne 0 ] && exit 0
 ;;
 "mp3")
 QUALITY=$(kdialog --geometry 400x300 --title "Qualité MP3" --menu "Choisissez la qualité (recommandé : 192 ou 256 kbps) :" \
 "128" "128 kbps" \
 "192" "192 kbps" \
 "256" "256 kbps" \
 "vbr" "VBR (qualité variable)")
 [ $? -ne 0 ] && exit 0
 ;;
 "ogg")
 QUALITY=$(kdialog --geometry 400x300 --title "Qualité OGG" --menu "Choisissez la qualité (recommandé : 6) :" \
 "3" "3 (≈96 kbps)" \
 "6" "6 (≈160 kbps)" \
 "9" "9 (≈320 kbps)")
 [ $? -ne 0 ] && exit 0
 ;;
 "wma")
 QUALITY=$(kdialog --geometry 400x300 --title "Qualité WMA" --menu "Choisissez la qualité (recommandé : 128 kbps) :" \
 "64" "64 kbps" \
 "128" "128 kbps" \
 "192" "192 kbps")
 [ $? -ne 0 ] && exit 0
 ;;
 "h264")
 QUALITY=$(kdialog --geometry 400x300 --title "Qualité H.264" --menu "Choisissez la qualité (recommandé : CRF 23) :" \
 "18" "CRF 18 (haute qualité)" \
 "23" "CRF 23 (standard)" \
 "28" "CRF 28 (basse qualité)")
 [ $? -ne 0 ] && exit 0
 ;;
 "h265")
 QUALITY=$(kdialog --geometry 400x300 --title "Qualité H.265" --menu "Choisissez la qualité (recommandé : CRF 23) :" \
 "18" "CRF 18 (haute qualité)" \
 "23" "CRF 23 (standard)" \
 "28" "CRF 28 (basse qualité)")
 [ $? -ne 0 ] && exit 0
 ;;
 "webm")
 QUALITY=$(kdialog --geometry 400x300 --title "Qualité WebM" --menu "Choisissez la qualité (recommandé : CRF 23) :" \
 "18" "CRF 18 (haute qualité)" \
 "23" "CRF 23 (standard)" \
 "28" "CRF 28 (basse qualité)")
 [ $? -ne 0 ] && exit 0
 ;;
 "reencapsulate")
 CONTAINER=$(kdialog --geometry 600x400 --title "Choisir le conteneur" --menu "Choisissez le nouveau conteneur :" \
 "mp4" "MP4 - Universel, compatible partout." \
 "mkv" "MKV - Polyvalent, supporte plusieurs pistes." \
 "webm" "WebM - Moderne pour le web, efficace." \
 "avi" "AVI - Ancien, compatible mais volumineux.")
 [ $? -ne 0 ] && exit 0
 ;;
 "flac"|"wav"|"mp4"|"mkv"|"ogv"|"avi")
 QUALITY=""
 ;;
 *)
 kdialog --error "Format non supporté : $OUTPUT_FORMAT"
 exit 1
 ;;
esac

# Fonction pour vérifier l'écrasement
check_overwrite() {
    local file="$1"
    if [ -f "$file" ]; then
        kdialog --geometry 400x200 --title "Fichier existant" --yesno "Le fichier $file existe déjà. Voulez-vous l'écraser ?"
        if [ $? -ne 0 ]; then
            echo "DEBUG: Conversion annulée pour $file : fichier non écrasé"
            return 1
        fi
    fi
    return 0
}

# Variables pour regrouper les résultats
SUCCESS_FILES=""
ERROR_FILES=""

# Nombre total de fichiers
TOTAL_FILES=$#

# Affiche une fenêtre d'attente simple avec bouton Annuler
echo "DEBUG: Création de la fenêtre d'attente pour $TOTAL_FILES fichiers"
kdialog --geometry 400x200 --title "Conversion en cours" --msgbox "Conversion en cours...\nCliquez sur Annuler pour arrêter." --ok-label "Annuler" &>/dev/null &
PID_KDIALOG=$!
echo "DEBUG: PID kdialog = $PID_KDIALOG"

# Vérifie que kdialog a bien démarré
if ! kill -0 $PID_KDIALOG 2>/dev/null; then
    echo "DEBUG: Échec du lancement de kdialog"
    echo "Erreur : Impossible de lancer la fenêtre d'attente"
    exit 1
fi

# Boucle sur chaque fichier sélectionné
CURRENT_FILE=0
for INPUT_FILE in "$@"; do
    ((CURRENT_FILE++))
    echo "DEBUG: Traitement de $INPUT_FILE ($CURRENT_FILE/$TOTAL_FILES)"

    # Vérifie si l'utilisateur a annulé (fenêtre fermée)
    if ! kill -0 $PID_KDIALOG 2>/dev/null; then
        echo "DEBUG: Fenêtre fermée ou annulée"
        echo "Conversion annulée par l'utilisateur"
        exit 0
    fi

    if [ ! -f "$INPUT_FILE" ]; then
        echo "DEBUG: Fichier $INPUT_FILE inexistant"
        ERROR_FILES="$ERROR_FILES\n$INPUT_FILE : Fichier inexistant"
        continue
    fi

    INPUT_DIR=$(dirname "$INPUT_FILE")
    INPUT_NAME=$(basename "$INPUT_FILE" | sed 's/\.[^.]*$//')
    INPUT_EXT="${INPUT_FILE##*.}"

    # Si c'est une vidéo et conversion vers audio, traiter toutes les pistes
    if [ "$INPUT_EXT" = "ogv" ] || [ "$INPUT_EXT" = "mkv" ] || [ "$INPUT_EXT" = "mp4" ] || [ "$INPUT_EXT" = "webm" ] || [ "$INPUT_EXT" = "avi" ]; then
        if [[ "$OUTPUT_FORMAT" =~ ^(opus|flac|aac|mp3|ogg|wav|wma)$ ]]; then
            AUDIO_TRACKS=$(ffprobe -v error -show_entries stream=index:stream_tags=language -select_streams a -of csv=p=0 "$INPUT_FILE" 2>/dev/null)
            if [ -z "$AUDIO_TRACKS" ]; then
                echo "DEBUG: Aucune piste audio détectée pour $INPUT_FILE"
                ERROR_FILES="$ERROR_FILES\n$INPUT_FILE : Aucune piste audio détectée"
                continue
            fi

            echo "DEBUG: Pistes audio détectées pour $INPUT_FILE : $AUDIO_TRACKS"
            SUCCESS_COUNT=0
            IFS=$'\n'
            for track in $AUDIO_TRACKS; do
                INDEX=$(echo "$track" | cut -d',' -f1)
                LANG=$(echo "$track" | cut -d',' -f2)
                AUDIO_INDEX=$((INDEX - 1))
                if [ -n "$LANG" ] && [ "$LANG" != "und" ]; then
                    SUFFIX="_$LANG"
                else
                    SUFFIX="_track$AUDIO_INDEX"
                fi
                OUTPUT_FILE="$INPUT_DIR/$INPUT_NAME$SUFFIX.$OUTPUT_FORMAT"

                # Vérifie l'écrasement avant conversion
                if ! check_overwrite "$OUTPUT_FILE"; then
                    continue
                fi

                # Vérifie les permissions d'écriture
                if [ -f "$OUTPUT_FILE" ] && [ ! -w "$OUTPUT_FILE" ]; then
                    echo "DEBUG: Le fichier $OUTPUT_FILE n'est pas accessible en écriture"
                    ERROR_FILES="$ERROR_FILES\n$OUTPUT_FILE : Fichier non accessible en écriture"
                    continue
                fi

                case "$OUTPUT_FORMAT" in
                    "opus")
                        CMD="ffmpeg -y -i \"$INPUT_FILE\" -map 0:a:$AUDIO_INDEX -c:a libopus -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
                        ;;
                    "flac")
                        CMD="ffmpeg -y -i \"$INPUT_FILE\" -map 0:a:$AUDIO_INDEX -c:a flac -vn \"$OUTPUT_FILE\""
                        ;;
                    "aac")
                        if [ "$QUALITY" = "vbr" ]; then
                            CMD="ffmpeg -y -i \"$INPUT_FILE\" -map 0:a:$AUDIO_INDEX -c:a aac -vbr 3 -vn \"$OUTPUT_FILE\""
                        else
                            CMD="ffmpeg -y -i \"$INPUT_FILE\" -map 0:a:$AUDIO_INDEX -c:a aac -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
                        fi
                        ;;
                    "mp3")
                        if [ "$QUALITY" = "vbr" ]; then
                            CMD="ffmpeg -y -i \"$INPUT_FILE\" -map 0:a:$AUDIO_INDEX -c:a libmp3lame -q:a 2 -vn \"$OUTPUT_FILE\""
                        else
                            CMD="ffmpeg -y -i \"$INPUT_FILE\" -map 0:a:$AUDIO_INDEX -c:a libmp3lame -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
                        fi
                        ;;
                    "ogg")
                        CMD="ffmpeg -y -i \"$INPUT_FILE\" -map 0:a:$AUDIO_INDEX -c:a libvorbis -q:a $QUALITY -vn \"$OUTPUT_FILE\""
                        ;;
                    "wav")
                        CMD="ffmpeg -y -i \"$INPUT_FILE\" -map 0:a:$AUDIO_INDEX -c:a pcm_s16le -vn \"$OUTPUT_FILE\""
                        ;;
                    "wma")
                        CMD="ffmpeg -y -i \"$INPUT_FILE\" -map 0:a:$AUDIO_INDEX -c:a wmav2 -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
                        ;;
                esac

                echo "DEBUG: Commande FFmpeg pour piste audio : $CMD"
                eval "$CMD" 2>> /tmp/ffmpeg.log &
                FFMPEG_PID=$!
                echo "DEBUG: PID FFmpeg = $FFMPEG_PID"

                while kill -0 $FFMPEG_PID 2>/dev/null; do
                    if ! kill -0 $PID_KDIALOG 2>/dev/null; then
                        echo "DEBUG: Annulation détectée pendant la conversion de la piste"
                        kill -9 $FFMPEG_PID 2>/dev/null
                        wait $FFMPEG_PID 2>/dev/null
                        [ -f "$OUTPUT_FILE" ] && rm -f "$OUTPUT_FILE"  # Supprime le fichier partiel
                        echo "Conversion annulée pour $INPUT_FILE"
                        exit 0
                    fi
                    sleep 0.5
                done

                wait $FFMPEG_PID
                FFMPEG_RESULT=$?
                echo "DEBUG: Résultat FFmpeg pour piste = $FFMPEG_RESULT"
                if [ $FFMPEG_RESULT -eq 0 ]; then
                    SUCCESS_FILES="$SUCCESS_FILES\n$OUTPUT_FILE"
                    ((SUCCESS_COUNT++))
                else
                    ERROR_FILES="$ERROR_FILES\n$OUTPUT_FILE : Erreur lors de la conversion de la piste $AUDIO_INDEX"
                fi
            done
            continue
        fi
    fi

    # Pour les autres cas (audio -> audio ou vidéo -> vidéo)
    if [ "$OUTPUT_FORMAT" = "h264" ] || [ "$OUTPUT_FORMAT" = "h265" ]; then
        OUTPUT_FILE="$INPUT_DIR/$INPUT_NAME.mp4"
    elif [ "$OUTPUT_FORMAT" = "reencapsulate" ]; then
        OUTPUT_FILE="$INPUT_DIR/$INPUT_NAME.$CONTAINER"
    else
        OUTPUT_FILE="$INPUT_DIR/$INPUT_NAME.$OUTPUT_FORMAT"
    fi

    # Vérifie l'écrasement avant conversion
    if ! check_overwrite "$OUTPUT_FILE"; then
        continue
    fi

    # Vérifie les permissions d'écriture
    if [ -f "$OUTPUT_FILE" ] && [ ! -w "$OUTPUT_FILE" ]; then
        echo "DEBUG: Le fichier $OUTPUT_FILE n'est pas accessible en écriture"
        ERROR_FILES="$ERROR_FILES\n$OUTPUT_FILE : Fichier non accessible en écriture"
        continue
    fi

    case "$OUTPUT_FORMAT" in
        "opus")
            CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:a libopus -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
            ;;
        "flac")
            CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:a flac -vn \"$OUTPUT_FILE\""
            ;;
        "aac")
            if [ "$QUALITY" = "vbr" ]; then
                CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:a aac -vbr 3 -vn \"$OUTPUT_FILE\""
            else
                CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:a aac -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
            fi
            ;;
        "mp3")
            if [ "$QUALITY" = "vbr" ]; then
                CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:a libmp3lame -q:a 2 -vn \"$OUTPUT_FILE\""
            else
                CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:a libmp3lame -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
            fi
            ;;
        "ogg")
            CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:a libvorbis -q:a $QUALITY -vn \"$OUTPUT_FILE\""
            ;;
        "wav")
            CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:a pcm_s16le -vn \"$OUTPUT_FILE\""
            ;;
        "wma")
            CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:a wmav2 -b:a ${QUALITY}k -vn \"$OUTPUT_FILE\""
            ;;
        "h264")
            if [ "$INPUT_EXT" != "ogv" ] && [ "$INPUT_EXT" != "mkv" ] && [ "$INPUT_EXT" != "mp4" ] && [ "$INPUT_EXT" != "webm" ] && [ "$INPUT_EXT" != "avi" ]; then
                ERROR_FILES="$ERROR_FILES\n$INPUT_FILE : Conversion audio vers vidéo non autorisée"
                continue
            fi
            CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:v libx264 -crf $QUALITY -preset medium -c:a aac -b:a 128k \"$OUTPUT_FILE\""
            ;;
        "h265")
            if [ "$INPUT_EXT" != "ogv" ] && [ "$INPUT_EXT" != "mkv" ] && [ "$INPUT_EXT" != "mp4" ] && [ "$INPUT_EXT" != "webm" ] && [ "$INPUT_EXT" != "avi" ]; then
                ERROR_FILES="$ERROR_FILES\n$INPUT_FILE : Conversion audio vers vidéo non autorisée"
                continue
            fi
            CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:v libx265 -crf $QUALITY -preset medium -c:a aac -b:a 128k \"$OUTPUT_FILE\""
            ;;
        "webm")
            if [ "$INPUT_EXT" != "ogv" ] && [ "$INPUT_EXT" != "mkv" ] && [ "$INPUT_EXT" != "mp4" ] && [ "$INPUT_EXT" != "webm" ] && [ "$INPUT_EXT" != "avi" ]; then
                ERROR_FILES="$ERROR_FILES\n$INPUT_FILE : Conversion audio vers vidéo non autorisée"
                continue
            fi
            CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:v libvpx-vp9 -crf $QUALITY -b:v 0 -c:a libopus -b:a 128k \"$OUTPUT_FILE\""
            ;;
        "mp4"|"mkv"|"ogv"|"avi")
            if [ "$INPUT_EXT" != "ogv" ] && [ "$INPUT_EXT" != "mkv" ] && [ "$INPUT_EXT" != "mp4" ] && [ "$INPUT_EXT" != "webm" ] && [ "$INPUT_EXT" != "avi" ]; then
                ERROR_FILES="$ERROR_FILES\n$INPUT_FILE : Conversion audio vers vidéo non autorisée"
                continue
            fi
            CMD="ffmpeg -y -i \"$INPUT_FILE\" -y \"$OUTPUT_FILE\""
            ;;
        "reencapsulate")
            if [ "$INPUT_EXT" != "ogv" ] && [ "$INPUT_EXT" != "mkv" ] && [ "$INPUT_EXT" != "mp4" ] && [ "$INPUT_EXT" != "webm" ] && [ "$INPUT_EXT" != "avi" ]; then
                ERROR_FILES="$ERROR_FILES\n$INPUT_FILE : Conversion audio vers vidéo non autorisée"
                continue
            fi
            CMD="ffmpeg -y -i \"$INPUT_FILE\" -c:v copy -c:a copy \"$OUTPUT_FILE\""
            ;;
    esac

    echo "DEBUG: Commande FFmpeg : $CMD"
    eval "$CMD" 2>> /tmp/ffmpeg.log &
    FFMPEG_PID=$!
    echo "DEBUG: PID FFmpeg = $FFMPEG_PID"

    while kill -0 $FFMPEG_PID 2>/dev/null; do
        if ! kill -0 $PID_KDIALOG 2>/dev/null; then
            echo "DEBUG: Annulation détectée pendant la conversion"
            kill -9 $FFMPEG_PID 2>/dev/null
            wait $FFMPEG_PID 2>/dev/null
            [ -f "$OUTPUT_FILE" ] && rm -f "$OUTPUT_FILE"  # Supprime le fichier partiel
            echo "Conversion annulée pour $INPUT_FILE"
            exit 0
        fi
        sleep 0.5
    done

    wait $FFMPEG_PID
    FFMPEG_RESULT=$?
    echo "DEBUG: Résultat FFmpeg = $FFMPEG_RESULT"
    if [ $FFMPEG_RESULT -eq 0 ]; then
        SUCCESS_FILES="$SUCCESS_FILES\n$OUTPUT_FILE"
    else
        ERROR_FILES="$ERROR_FILES\n$OUTPUT_FILE : Erreur lors de la conversion"
    fi
done

# Ferme la fenêtre d'attente si elle est encore ouverte
if kill -0 $PID_KDIALOG 2>/dev/null; then
    echo "DEBUG: Fermeture de la fenêtre d'attente"
    kill $PID_KDIALOG 2>/dev/null
fi

# Affiche un message final regroupé
FINAL_MESSAGE=""
if [ -n "$SUCCESS_FILES" ]; then
    FINAL_MESSAGE="Fichiers convertis avec succès :$SUCCESS_FILES"
fi
if [ -n "$ERROR_FILES" ]; then
    FINAL_MESSAGE="$FINAL_MESSAGE\n\nErreurs rencontrées :$ERROR_FILES\nConsultez /tmp/ffmpeg.log pour plus de détails."
fi

if [ -n "$FINAL_MESSAGE" ]; then
    echo "DEBUG: Affichage du message final"
    kdialog --geometry 600x400 --title "Résultat de la conversion" --msgbox "$FINAL_MESSAGE"
fi
