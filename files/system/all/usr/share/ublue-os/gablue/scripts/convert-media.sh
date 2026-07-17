#!/usr/bin/bash

################################################################################
# convert-media.sh - Conversion de fichiers médias (audio/vidéo)
#
# Utilise ffmpeg pour convertir des fichiers audio et vidéo avec une
# interface graphique kdialog (barre de progression, choix du format).
################################################################################

set -euo pipefail

# =============================================================================
# Configuration
# =============================================================================

FFMPEG_LOG="/tmp/ffmpeg-convert-$$.log"

# Extensions vidéo reconnues
VIDEO_EXTS=("ogv" "mkv" "mp4" "webm" "avi")

# =============================================================================
# Dépendances
# =============================================================================

for dep in ffmpeg ffprobe; do
    if ! command -v "$dep" &>/dev/null; then
        kdialog --error "$dep n'est pas installé. Installez-le avec : brew install $dep" 2>/dev/null || \
            echo "Erreur: $dep n'est pas installé" >&2
        exit 1
    fi
done

HAS_GUI=false
if command -v kdialog >/dev/null 2>&1 && [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
    HAS_GUI=true
fi

# =============================================================================
# Utilitaires
# =============================================================================

# Détecte la commande qdbus disponible
_get_qdbus_cmd() {
    for cmd in qdbus6 qdbus-qt6 qdbus-qt5 qdbus; do
        if command -v "$cmd" &>/dev/null; then
            echo "$cmd"
            return
        fi
    done
    echo ""
}

# Vérifie si un fichier est une vidéo
_is_video() {
    local ext="${1##*.}"
    ext=$(echo "$ext" | tr '[:upper:]' '[:lower:]')
    for ve in "${VIDEO_EXTS[@]}"; do
        [[ "$ext" == "$ve" ]] && return 0
    done
    return 1
}

# =============================================================================
# Barre de progression kdialog
# =============================================================================

_progress_qdbus=""

_progress_create() {
    local title="$1" steps="${2:-10}"
    local qdbus_cmd
    qdbus_cmd=$(_get_qdbus_cmd)
    if [ "$HAS_GUI" = true ] && [ -n "$qdbus_cmd" ]; then
        local ref
        ref=$(kdialog --title "$title" --progressbar "Préparation..." "$steps" 2>/dev/null)
        ref=$(printf '%s' "$ref" | tr -d '\n\r' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
        if [ -n "$ref" ]; then
            _progress_qdbus="$qdbus_cmd"
            local service path
            read service path <<< "$(echo "$ref" | awk '{print $1, $2}')"
            _progress_service="$service"
            _progress_path="$path"
            $qdbus_cmd "$service" "$path" showCancelButton true >/dev/null 2>&1 || true
        fi
    fi
}

_progress_update() {
    local value="$1" message="${2:-}"
    if [ -n "${_progress_service:-}" ] && [ -n "${_progress_qdbus:-}" ]; then
        $_progress_qdbus "${_progress_service}" "${_progress_path}" Set "" value "$value" >/dev/null 2>&1 || true
        if [ -n "$message" ]; then
            $_progress_qdbus "${_progress_service}" "${_progress_path}" setLabelText "$message" >/dev/null 2>&1 || true
        fi
    fi
}

_progress_close() {
    if [ -n "${_progress_service:-}" ] && [ -n "${_progress_qdbus:-}" ]; then
        $_progress_qdbus "${_progress_service}" "${_progress_path}" close >/dev/null 2>&1 || true
    fi
    _progress_qdbus=""
    unset _progress_service _progress_path 2>/dev/null || true
}

_progress_is_cancelled() {
    if [ -n "${_progress_service:-}" ] && [ -n "${_progress_qdbus:-}" ]; then
        local cancelled
        cancelled=$($_progress_qdbus "${_progress_service}" "${_progress_path}" wasCancelled 2>/dev/null || true)
        if [ "$cancelled" = "true" ] || [ -z "$cancelled" ]; then
            return 0
        fi
    fi
    return 1
}

# =============================================================================
# Vérification des arguments
# =============================================================================

if [ $# -eq 0 ]; then
    echo "Usage: $0 <fichier média> [fichier média...]" >&2
    exit 1
fi

# Valider que les fichiers existent
for f in "$@"; do
    if [ ! -f "$f" ]; then
        echo "Erreur: fichier introuvable: $f" >&2
        exit 1
    fi
done

# =============================================================================
# Sélection du format de sortie
# =============================================================================

# Déterminer si tous les fichiers sont des vidéos
ALL_VIDEO=true
for f in "$@"; do
    if ! _is_video "$f"; then
        ALL_VIDEO=false
        break
    fi
done

# Construire le menu
MENU_OPTIONS=()
MENU_OPTIONS+=("opus"   "Opus - Excellente qualité à faible bitrate (recommandé)")
MENU_OPTIONS+=("flac"   "FLAC - Sans perte, qualité originale")
MENU_OPTIONS+=("aac"    "AAC - Évolution du MP3, compatible")
MENU_OPTIONS+=("mp3"    "MP3 - Universellement compatible")
MENU_OPTIONS+=("ogg"    "OGG Vorbis - Open-source")
MENU_OPTIONS+=("wav"    "WAV - Sans perte brut")
MENU_OPTIONS+=("wma"    "WMA - Format Microsoft")

if [ "$ALL_VIDEO" = true ]; then
    MENU_OPTIONS+=("h264"  "H.264 (MP4) - Standard, excellente compatibilité")
    MENU_OPTIONS+=("h265"  "H.265/HEVC (MP4) - Meilleure compression")
    MENU_OPTIONS+=("webm"  "WebM - Moderne pour le web (VP9)")
    MENU_OPTIONS+=("mp4"   "MP4 - Ré-encapsuler sans ré-encoder")
    MENU_OPTIONS+=("mkv"   "MKV - Ré-encapsuler sans ré-encoder")
    MENU_OPTIONS+=("ogv"   "OGV - Ré-encapsuler sans ré-encoder")
    MENU_OPTIONS+=("avi"   "AVI - Ré-encapsuler sans ré-encoder")
    MENU_OPTIONS+=("reencapsulate" "Ré-encapsuler - Change le conteneur (rapide)")
fi

if [ "$HAS_GUI" = true ]; then
    OUTPUT_FORMAT=$(kdialog --geometry 600x400 --title "Convertir les médias" \
        --menu "Choisissez le format de sortie pour tous les fichiers :" "${MENU_OPTIONS[@]}" 2>/dev/null) || exit 0
else
    echo "Formats disponibles :" >&2
    for ((i=0; i<${#MENU_OPTIONS[@]}; i+=2)); do
        echo "  ${MENU_OPTIONS[$i]} - ${MENU_OPTIONS[$i+1]}" >&2
    done
    read -rp "Format de sortie : " OUTPUT_FORMAT
fi

# =============================================================================
# Sélection de la qualité
# =============================================================================

QUALITY=""
CONTAINER=""

_select_quality_menu() {
    local title="$1" recommended="$2"
    shift 2
    local options=("$@")

    if [ "$HAS_GUI" = true ]; then
        local result
        result=$(kdialog --geometry 400x300 --title "$title" --menu "Choisissez la qualité :" "${options[@]}" 2>/dev/null) || exit 0
        echo "$result"
    else
        echo "Options de qualité pour $title :" >&2
        for ((i=0; i<${#options[@]}; i+=2)); do
            echo "  ${options[$i]} - ${options[$i+1]}" >&2
        done
        read -rp "Qualité [$recommended] : " result
        echo "${result:-$recommended}"
    fi
}

_select_quality_custom() {
    local prompt="$1" default="$2"
    if [ "$HAS_GUI" = true ]; then
        local result
        result=$(kdialog --inputbox "$prompt" "$default" 2>/dev/null) || exit 0
        echo "$result"
    else
        read -rp "$prompt [$default] : " result
        echo "${result:-$default}"
    fi
}

case "$OUTPUT_FORMAT" in
    opus)
        QUALITY=$(_select_quality_menu "Qualité Opus" "128" \
            "64"  "64 kbps" \
            "96"  "96 kbps" \
            "128" "128 kbps (recommandé)" \
            "256" "256 kbps" \
            "320" "320 kbps" \
            "custom" "Valeur personnalisée (kbps)")
        [ "$QUALITY" = "custom" ] && QUALITY=$(_select_quality_custom "Qualité en kbps (ex: 192) :" "128")
        ;;
    aac)
        QUALITY=$(_select_quality_menu "Qualité AAC" "256" \
            "64"   "64 kbps" \
            "128"  "128 kbps" \
            "192"  "192 kbps" \
            "256"  "256 kbps (recommandé)" \
            "320"  "320 kbps" \
            "vbr"  "VBR (qualité variable, niveau 3)" \
            "custom" "Valeur personnalisée (kbps)")
        [ "$QUALITY" = "custom" ] && QUALITY=$(_select_quality_custom "Qualité en kbps (ex: 192) :" "128")
        ;;
    mp3)
        QUALITY=$(_select_quality_menu "Qualité MP3" "192" \
            "128" "128 kbps" \
            "192" "192 kbps (recommandé)" \
            "256" "256 kbps" \
            "320" "320 kbps" \
            "vbr" "VBR (qualité variable)" \
            "custom" "Valeur personnalisée (kbps)")
        [ "$QUALITY" = "custom" ] && QUALITY=$(_select_quality_custom "Qualité en kbps (ex: 192) :" "192")
        ;;
    ogg)
        QUALITY=$(_select_quality_menu "Qualité OGG" "6" \
            "3" "3 (~96 kbps)" \
            "6" "6 (~160 kbps, recommandé)" \
            "9" "9 (~320 kbps)" \
            "custom" "Valeur personnalisée (0-10)")
        [ "$QUALITY" = "custom" ] && QUALITY=$(_select_quality_custom "Qualité (0-10, ex: 5) :" "6")
        ;;
    wma)
        QUALITY=$(_select_quality_menu "Qualité WMA" "128" \
            "64"  "64 kbps" \
            "128" "128 kbps (recommandé)" \
            "192" "192 kbps" \
            "custom" "Valeur personnalisée (kbps)")
        [ "$QUALITY" = "custom" ] && QUALITY=$(_select_quality_custom "Qualité en kbps (ex: 128) :" "128")
        ;;
    h264)
        QUALITY=$(_select_quality_menu "Qualité H.264" "23" \
            "18" "CRF 18 (haute qualité)" \
            "23" "CRF 23 (standard, recommandé)" \
            "28" "CRF 28 (basse qualité)" \
            "custom" "Valeur personnalisée (CRF)")
        [ "$QUALITY" = "custom" ] && QUALITY=$(_select_quality_custom "Valeur CRF (0-51, ex: 23) :" "23")
        ;;
    h265)
        QUALITY=$(_select_quality_menu "Qualité H.265" "23" \
            "18" "CRF 18 (haute qualité)" \
            "23" "CRF 23 (standard, recommandé)" \
            "28" "CRF 28 (basse qualité)" \
            "custom" "Valeur personnalisée (CRF)")
        [ "$QUALITY" = "custom" ] && QUALITY=$(_select_quality_custom "Valeur CRF (0-51, ex: 23) :" "23")
        ;;
    webm)
        QUALITY=$(_select_quality_menu "Qualité WebM" "23" \
            "18" "CRF 18 (haute qualité)" \
            "23" "CRF 23 (standard, recommandé)" \
            "28" "CRF 28 (basse qualité)" \
            "custom" "Valeur personnalisée (CRF)")
        [ "$QUALITY" = "custom" ] && QUALITY=$(_select_quality_custom "Valeur CRF (0-51, ex: 23) :" "23")
        ;;
    reencapsulate)
        if [ "$HAS_GUI" = true ]; then
            CONTAINER=$(kdialog --geometry 400x300 --title "Conteneur" \
                --menu "Choisissez le nouveau conteneur :" \
                "mp4"  "MP4 - Universel" \
                "mkv"  "MKV - Polyvalent" \
                "webm" "WebM - Moderne" \
                "avi"  "AVI - Ancien" 2>/dev/null) || exit 0
        else
            read -rp "Conteneur (mp4/mkv/webm/avi) [mp4] : " CONTAINER
            CONTAINER="${CONTAINER:-mp4}"
        fi
        ;;
    flac|wav|mp4|mkv|ogv|avi)
        ;;
    *)
        echo "Format non supporté: $OUTPUT_FORMAT" >&2
        exit 1
        ;;
esac

# =============================================================================
# Calcul du nombre total d'étapes pour la barre de progression
# =============================================================================

TOTAL_STEPS=0
for f in "$@"; do
    if _is_video "$f" && [[ "$OUTPUT_FORMAT" =~ ^(opus|flac|aac|mp3|ogg|wav|wma)$ ]]; then
        # Vidéo vers audio : compter les pistes audio
        tracks=$(ffprobe -v error -show_entries stream=index -select_streams a -of csv=p=0 "$f" 2>/dev/null | wc -l)
        TOTAL_STEPS=$((TOTAL_STEPS + tracks))
    else
        TOTAL_STEPS=$((TOTAL_STEPS + 1))
    fi
done

# =============================================================================
# Fonction de conversion
# =============================================================================

# Exécute ffmpeg et gère l'annulation
_run_ffmpeg() {
    local output_file="$1"
    shift
    local ffmpeg_args=("$@")

    ffmpeg -y "${ffmpeg_args[@]}" 2>> "$FFMPEG_LOG" &
    local ffmpeg_pid=$!

    while kill -0 "$ffmpeg_pid" 2>/dev/null; do
        if _progress_is_cancelled; then
            kill -9 "$ffmpeg_pid" 2>/dev/null || true
            wait "$ffmpeg_pid" 2>/dev/null || true
            [ -f "$output_file" ] && rm -f "$output_file"
if [ "$TOTAL_STEPS" -gt 1 ]; then
    _progress_close
fi
            echo "Conversion annulée."
            exit 0
        fi
        sleep 0.5
    done

    wait "$ffmpeg_pid"
    return $?
}

# =============================================================================
# Construction de la commande ffmpeg
# =============================================================================

_build_ffmpeg_cmd() {
    local input="$1" output="$2" format="$3" quality="$4"
    local -a cmd=(-i "$input")

    case "$format" in
        opus)   cmd+=(-c:a libopus -b:a "${quality}k" -vn) ;;
        flac)   cmd+=(-c:a flac -vn) ;;
        aac)    if [ "$quality" = "vbr" ]; then cmd+=(-c:a aac -vbr 3 -vn)
                else cmd+=(-c:a aac -b:a "${quality}k" -vn); fi ;;
        mp3)    if [ "$quality" = "vbr" ]; then cmd+=(-c:a libmp3lame -q:a 2 -vn)
                else cmd+=(-c:a libmp3lame -b:a "${quality}k" -vn); fi ;;
        ogg)    cmd+=(-c:a libvorbis -q:a "$quality" -vn) ;;
        wav)    cmd+=(-c:a pcm_s16le -vn) ;;
        wma)    cmd+=(-c:a wmav2 -b:a "${quality}k" -vn) ;;
        h264)   cmd+=(-c:v libx264 -crf "$quality" -preset medium -c:a aac -b:a 128k) ;;
        h265)   cmd+=(-c:v libx265 -crf "$quality" -preset medium -c:a aac -b:a 128k) ;;
        webm)   cmd+=(-c:v libvpx-vp9 -crf "$quality" -b:v 0 -c:a libopus -b:a 128k) ;;
        mp4|mkv|ogv|avi)
                cmd+=(-c copy) ;;
        reencapsulate)
                cmd+=(-c:v copy -c:a copy) ;;
    esac

    cmd+=("$output")
    printf '%s\n' "${cmd[@]}"
}

# =============================================================================
# Barre de progression
# =============================================================================

if [ "$TOTAL_STEPS" -le 1 ]; then
    # Un seul fichier : notification simple, pas de barre de progression
    if [ "$HAS_GUI" = true ]; then
        kdialog --passivepopup "Conversion en cours..." 3 2>/dev/null || true
    fi
else
    _progress_create "Conversion de médias" "$TOTAL_STEPS"
fi

# =============================================================================
# Boucle de conversion
# =============================================================================

SUCCESS_FILES=""
ERROR_FILES=""
CURRENT_STEP=0

for INPUT_FILE in "$@"; do

    INPUT_DIR="$(dirname "$INPUT_FILE")"
    INPUT_NAME="$(basename "$INPUT_FILE" | sed 's/\.[^.]*$//')"

    # Conversion vidéo vers audio : traiter toutes les pistes
    if _is_video "$INPUT_FILE" && [[ "$OUTPUT_FORMAT" =~ ^(opus|flac|aac|mp3|ogg|wav|wma)$ ]]; then

        audio_tracks=$(ffprobe -v error -show_entries stream=index:stream_tags=language -select_streams a -of csv=p=0 "$INPUT_FILE" 2>/dev/null)

        if [ -z "$audio_tracks" ]; then
            ERROR_FILES="$ERROR_FILES\n$INPUT_FILE : Aucune piste audio"
            continue
        fi

        while IFS= read -r track; do
            track_index=$(echo "$track" | cut -d',' -f1)
            track_lang=$(echo "$track" | cut -d',' -f2)
            # ffprobe donne l'index absolu (0=vidéo, 1+ = audio)
            # -map 0:a:N utilise l'index ordinal des pistes audio
            audio_index=$((track_index - 1))

            suffix=""
            if [ -n "$track_lang" ] && [ "$track_lang" != "und" ]; then
                suffix="_$track_lang"
            else
                suffix="_track$audio_index"
            fi

            output_file="$INPUT_DIR/$INPUT_NAME$suffix.$OUTPUT_FORMAT"

            # Vérifier l'écrasement
            if [ -f "$output_file" ]; then
                if [ "$HAS_GUI" = true ]; then
                    if ! kdialog --yesno "Le fichier $(basename "$output_file") existe déjà.\nL'écraser ?" 2>/dev/null; then
                        continue
                    fi
                else
                    read -rp "Écraser $output_file ? (o/N) : " confirm
                    [[ "$confirm" =~ ^[oOyY]$ ]] || continue
                fi
            fi

            CURRENT_STEP=$((CURRENT_STEP + 1))
            if [ "$TOTAL_STEPS" -gt 1 ]; then
                _progress_update "$CURRENT_STEP" "$(basename "$INPUT_FILE") → $suffix.$OUTPUT_FORMAT"
            fi

            # Construire les arguments via _build_ffmpeg_cmd puis les lire dans un tableau
            mapfile -t ffmpeg_args < <(_build_ffmpeg_cmd "$INPUT_FILE" "$output_file" "$OUTPUT_FORMAT" "$QUALITY")
            # Ajouter -map pour la piste spécifique
            # Insérer -map après -i (index 1)
            final_args=("${ffmpeg_args[@]:0:2}" "-map" "0:a:$audio_index" "${ffmpeg_args[@]:2}")

            if _run_ffmpeg "$output_file" "${final_args[@]}"; then
                SUCCESS_FILES="$SUCCESS_FILES\n$output_file"
            else
                ERROR_FILES="$ERROR_FILES\n$output_file : Erreur de conversion"
            fi
        done <<< "$audio_tracks"

        continue
    fi

    # Conversion standard (audio→audio ou vidéo→vidéo)
    if [ "$OUTPUT_FORMAT" = "h264" ] || [ "$OUTPUT_FORMAT" = "h265" ]; then
        output_file="$INPUT_DIR/$INPUT_NAME.mp4"
    elif [ "$OUTPUT_FORMAT" = "reencapsulate" ]; then
        output_file="$INPUT_DIR/$INPUT_NAME.$CONTAINER"
    else
        output_file="$INPUT_DIR/$INPUT_NAME.$OUTPUT_FORMAT"
    fi

    # Vérifier que le fichier de sortie est différent de l'entrée
    if [ "$output_file" = "$INPUT_FILE" ]; then
        ERROR_FILES="$ERROR_FILES\n$INPUT_FILE : Conversion ignorée (format identique à la source)"
        continue
    fi

    # Empêcher la conversion audio vers vidéo
    if [[ "$OUTPUT_FORMAT" =~ ^(h264|h265|webm|mp4|mkv|ogv|avi|reencapsulate)$ ]] && ! _is_video "$INPUT_FILE"; then
        ERROR_FILES="$ERROR_FILES\n$INPUT_FILE : Conversion audio vers vidéo non autorisée"
        continue
    fi

    # Vérifier l'écrasement
    if [ -f "$output_file" ]; then
        if [ "$HAS_GUI" = true ]; then
            if ! kdialog --yesno "Le fichier $(basename "$output_file") existe déjà.\nL'écraser ?" 2>/dev/null; then
                continue
            fi
        else
            read -rp "Écraser $output_file ? (o/N) : " confirm
            [[ "$confirm" =~ ^[oOyY]$ ]] || continue
        fi
    fi

    CURRENT_STEP=$((CURRENT_STEP + 1))
    if [ "$TOTAL_STEPS" -gt 1 ]; then
        _progress_update "$CURRENT_STEP" "$(basename "$INPUT_FILE") → $(basename "$output_file")"
    fi

    mapfile -t ffmpeg_args < <(_build_ffmpeg_cmd "$INPUT_FILE" "$output_file" "$OUTPUT_FORMAT" "$QUALITY")

    if _run_ffmpeg "$output_file" "${ffmpeg_args[@]}"; then
        SUCCESS_FILES="$SUCCESS_FILES\n$output_file"
    else
        ERROR_FILES="$ERROR_FILES\n$output_file : Erreur de conversion"
    fi
done

# =============================================================================
# Résultat
# =============================================================================

_progress_close

FINAL_MESSAGE=""
if [ -n "$SUCCESS_FILES" ]; then
    FINAL_MESSAGE="Fichiers convertis avec succès :$SUCCESS_FILES"
fi
if [ -n "$ERROR_FILES" ]; then
    FINAL_MESSAGE="$FINAL_MESSAGE\n\nErreurs rencontrées :$ERROR_FILES\nConsultez $FFMPEG_LOG pour les détails."
fi

if [ -n "$FINAL_MESSAGE" ]; then
    if [ "$HAS_GUI" = true ]; then
        kdialog --geometry 600x400 --title "Résultat de la conversion" --msgbox "$FINAL_MESSAGE" 2>/dev/null || true
    else
        echo -e "$FINAL_MESSAGE"
    fi
fi

# Nettoyage du log s'il n'y a pas eu d'erreurs
if [ -z "$ERROR_FILES" ]; then
    rm -f "$FFMPEG_LOG"
fi
