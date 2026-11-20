#!/usr/bin/bash
# test-qualite.sh - enregistre la même scène à plusieurs réglages, pour juger
# la qualité à l'œil (et au débit) avant de choisir un défaut.
#
#   ./test-qualite.sh                 quatre profils, 12 s chacun (défaut)
#   ./test-qualite.sh --secondes 20   clips plus longs
#   ./test-qualite.sh --profil 1080p-h264-q3 --profil source-h265-q3
#   ./test-qualite.sh --liste         les profils disponibles
#
# Ce qu'il fait, et pourquoi :
#   - la capture est celle du DÉMON (KMS) : c'est exactement ce que fera un
#     enregistrement réel, pas une capture logicielle qui mentirait sur le coût ;
#   - il écrit les réglages dans `~/.config/gablue/recorder.conf` — le VRAI
#     fichier, pour tester aussi la reprise à chaud (le démon surveille sa
#     date) — et il le REMET à l'identique à la sortie (piège : sans ça, le
#     banc laisserait la machine en qualité 5) ;
#   - chaque clip est renommé et mesuré (durée, débit, Go/h, décodage sans
#     erreur) : tu peux comparer les fichiers côte à côte, et savoir ce que
#     coûtera une heure de jeu.
#
# Pendant le compte à rebours, bascule sur le jeu : c'est l'écran qui est
# filmé. Les réglages de jeu (résolution, HDR…) ne changent rien au banc.
set -uo pipefail

HERE="$(dirname "$(readlink -f "$0")")"
# Image : /usr/bin/grec + scripts du pad dans /usr/libexec/gablue-pad.
# Banc : les scripts du dépôt (GPAD_PAD_DIR) et le démon du banc (GPAD_REC…).
GREC=${GREC:-$(command -v grec 2>/dev/null || echo "$HERE/grec")}
export GPAD_PAD_DIR=${GPAD_PAD_DIR:-$HERE/../gablue-pad}
export GPAD_SANS_BIP=1
CONF=${XDG_CONFIG_HOME:-$HOME/.config}/gablue/recorder.conf
DOSSIER=${RECORD_DIR:-$HOME/Vidéos/Captures}
SECONDES=12
ATTENTE=5
PROFILS=()

# nom | codec | qualite | resolution | fps   (défauts du démon entre crochets)
TOUS=(
    "defaut|h264|3|1920x1080|60"
    "1080p-h264-q1|h264|1|1920x1080|60"
    "1080p-h264-q5|h264|5|1920x1080|60"
    "source-h265-q3|h265|3|source|60"
)

usage() { sed -n '2,/^set /p' "$0" | sed -e 's/^# \{0,1\}//' -e '/^set /d'; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --secondes) SECONDES="$2"; shift 2 ;;
        --attente)  ATTENTE="$2"; shift 2 ;;
        --dossier)  DOSSIER="$2"; shift 2 ;;
        --profil)   PROFILS+=("$2"); shift 2 ;;
        --liste)
            echo "profils disponibles :"
            for p in "${TOUS[@]}"; do echo "  ${p%%|*}  (${p#*|})"; done
            exit 0 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "option inconnue : $1" >&2; exit 2 ;;
    esac
done

[[ -x "$GREC" ]] || { echo "ECHEC: grec introuvable ($GREC)" >&2; exit 1; }
mkdir -p "$DOSSIER" || exit 1

# --- sauvegarde / restauration de la configuration du compte ---------------
SAUVE="$(mktemp "${TMPDIR:-/tmp}/recorder.conf.XXXXXX")"
[[ -f "$CONF" ]] && cp "$CONF" "$SAUVE" || rm -f "$SAUVE"
propre() {
    if [[ -s "$SAUVE" ]]; then
        mkdir -p "$(dirname "$CONF")"
        cp "$SAUVE" "$CONF"
        echo "== réglages d'origine restaurés ($CONF)"
    else
        rm -f "$CONF"
        echo "== aucun réglage d'origine : fichier retiré"
    fi
    rm -f "$SAUVE"
}
trap propre EXIT INT TERM

# Écrit une clé en préservant le reste du fichier (commentaires, inconnues).
pose() {
    mkdir -p "$(dirname "$CONF")"
    if [[ -f "$CONF" ]] && grep -q "^$1=" "$CONF"; then
        sed -i "s|^$1=.*|$1=$2|" "$CONF"
    else
        printf '%s=%s\n' "$1" "$2" >> "$CONF"
    fi
}

# Le démon relit le fichier dès qu'il change (il en surveille la date) ; on
# attend qu'il ait appliqué, sinon le clip suivant partirait avec l'ancien
# codec (« réglage : en-attente » tant qu'un mp4 est ouvert).
attend_application() {
    for _ in $(seq 1 40); do
        local etat
        etat=$("$GREC" status 2>/dev/null | sed -n 's/^réglage : //p')
        [[ "$etat" == "en-attente" ]] || return 0
        sleep 0.25
    done
    echo "   (réglage toujours en attente : le clip suivant peut différer)"
}

# --- arrêt de ce qui tourne déjà ------------------------------------------
if "$GREC" status | grep -q '^actif'; then
    echo "un enregistrement tourne : arrêt avant le banc"
    "$GREC" stop >/dev/null
fi
if "$GREC" replay status | grep -q '^actif'; then
    echo "replay actif : arrêt (sinon l'anneau garderait l'ancien codec)"
    "$GREC" replay stop >/dev/null
fi

# --- profils ---------------------------------------------------------------
[[ ${#PROFILS[@]} -gt 0 ]] || PROFILS=("defaut" "1080p-h264-q1" "1080p-h264-q5" "source-h265-q3")
choisis=()
for nom in "${PROFILS[@]}"; do
    for p in "${TOUS[@]}"; do
        [[ "${p%%|*}" == "$nom" ]] && choisis+=("$p")
    done
    [[ ${#choisis[@]} -eq 0 ]] && { echo "profil inconnu : $nom (--liste)" >&2; exit 2; }
done

declare -a RESUME
i=0
for p in "${choisis[@]}"; do
    i=$((i + 1))
    IFS='|' read -r nom codec qualite reso fps <<< "$p"
    pose codec "$codec"
    pose qualite "$qualite"
    pose resolution "$reso"
    pose fps "$fps"
    attend_application

    echo
    echo "== [$i/${#choisis[@]}] $nom : $codec q$qualite $reso $fps fps"
    for reste in $(seq "$ATTENTE" -1 1); do
        printf '\r   bascule sur le JEU, départ dans %2d s… ' "$reste"
        sleep 1
    done
    printf '\r%-60s\n' "   enregistrement de ${SECONDES} s…"

    "$GREC" start >/dev/null || { echo "   ECHEC du démarrage" >&2; continue; }
    sleep "$SECONDES"
    sortie=$("$GREC" stop)
    fichier=$(sed -n 's/^enregistré : \(.*\) (.*/\1/p' <<< "$sortie")
    [[ -n "$fichier" && -f "$fichier" ]] || { echo "   aucun fichier ($sortie)"; continue; }

    # nom lisible + dossier demandé
    cible="$DOSSIER/qualite_${nom}_$(date +%H%M%S).mp4"
    mv -f "$fichier" "$cible"

    taille=$(stat -c %s "$cible")
    duree=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$cible" 2>/dev/null)
    mbit=$(awk -v s="$taille" -v d="${duree:-1}" 'BEGIN { printf "%.1f", s*8/d/1000000 }')
    goh=$(awk -v m="$mbit" 'BEGIN { printf "%.1f", m*3600/8/1024 }')
    erreurs=$(ffmpeg -v error -i "$cible" -map 0 -f null - 2>&1 | wc -l)
    printf '   %s\n   %5.1f Mo, %s s, %s Mbit/s -> %s Go/h, décodage : %s\n' \
        "$(basename "$cible")" \
        "$(awk -v o="$taille" 'BEGIN { printf "%.1f", o/1048576 }')" \
        "${duree:0:5}" "$mbit" "$goh" \
        "$([[ "$erreurs" -eq 0 ]] && echo 'aucune erreur' || echo "$erreurs ligne(s)")"
    RESUME+=("$nom|$mbit|$goh|$(basename "$cible")")
done

echo
echo "== résumé"
for r in "${RESUME[@]}"; do
    IFS='|' read -r nom mbit goh fichier <<< "$r"
    printf '   %-16s %6s Mbit/s  %5s Go/h   %s\n' "$nom" "$mbit" "$goh" "$fichier"
done
cat <<EOF

Comparer à l'œil (mêmes images, mêmes mouvements) :
  mpv "$DOSSIER"/qualite_*.mp4

À retenir : le 1080p est quasi invisible sur un téléphone et divise le poids
par 2,25 ; le H.265 est plus léger mais ne s'ouvre pas partout (Windows sans
extension HEVC = écran noir). Les réglages choisis ici ne sont PAS gardés :
le banc remet ton fichier d'origine.
EOF
