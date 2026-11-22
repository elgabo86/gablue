#!/usr/bin/bash
# screenshot.sh - capture d'écran du banc (remplace takescreenshot).
#
#   ./screenshot.sh
#
# Même convention que l'ancien script : ~/Images/Screenshots/screenshot_<date>.png
# et un son de déclencheur (sounds/shot.wav).
#
# Deux choix mesurés (2560x1440, Plasma 6.8.3) :
#  1. le son part AVANT la capture : le clic est perçu en ~60 ms, au lieu
#     d'attendre la fin de la capture puis ~60 ms de démarrage de ffplay
#     (~0,4 s en tout avec spectacle) ;
#  2. la capture passe par `gpad-shot` (~30 ms, appel direct
#     org.kde.KWin.ScreenShot2 + PNG, sans le pointeur) et ne retombe sur
#     spectacle (~350 ms : client Qt) que si l'appel échoue.
set -uo pipefail

HERE="$(dirname "$(readlink -f "$0")")"

# Dossier des captures : même fichier que l'enregistreur et Gablue Manager
# (`dossier_screenshot`, réglé depuis les Réglages du launcher). Clé absente =
# défaut ci-dessous ; `SCREENSHOT_DIR` reste prioritaire (bancs et grec).
conf="${XDG_CONFIG_HOME:-$HOME/.config}/gablue/recorder.conf"
dir_conf=""
[[ -f "$conf" ]] && dir_conf=$(sed -n 's/^dossier_screenshot=//p' "$conf" | tail -1)
# le fichier est écrit par plusieurs outils : un `~` littéral est développé ici
[[ "$dir_conf" == "~"* ]] && dir_conf="$HOME${dir_conf#\~}"
DIR=${SCREENSHOT_DIR:-${dir_conf:-$HOME/Images/Screenshots}}
f="$DIR/screenshot_$(date +%Y%m%d_%H%M%S).png"

mkdir -p "$DIR"

# --- jeu en cours : nom affiché et logo ------------------------------------
# Deux sources, dans cet ordre :
#  - `<cache>/glauncher/running.txt` : écrit par le launcher pendant une partie
#    (`nom` = nom de la bibliothèque, `logo` = chemin du logo), et supprimé à
#    la fin du jeu ;
#  - `$XDG_RUNTIME_DIR/gablue/running.txt` : écrit par `gwine`/`lgplaunch`
#    (nom du paquet) pour les lancements hors launcher.
# Les PID servent de garde : un fichier qui traîne après un plantage ne doit
# pas étiqueter une capture de bureau avec le dernier jeu.
RT=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
champ() { sed -n "s/^$2=//p" "$1" 2>/dev/null | tail -1; }
pids_vivants() {
    local p q=0
    for p in ${1//,/ }; do kill -0 "$p" 2>/dev/null && q=1; done
    [[ $q == 1 ]]
}
titre=""; logo=""
run_launcher="${XDG_CACHE_HOME:-$HOME/.cache}/glauncher/running.txt"
if [[ -f "$run_launcher" ]]; then
    if pids_vivants "$(champ "$run_launcher" pids)"; then
        titre=$(champ "$run_launcher" nom)
        logo=$(champ "$run_launcher" logo)
    fi
fi
run_session="$RT/gablue/running.txt"
if [[ -z "$titre" && -f "$run_session" ]]; then
    if pids_vivants "$(champ "$run_session" pids)"; then
        titre=$(champ "$run_session" titre)
        [[ -n "$titre" ]] || titre=$(champ "$run_session" nom)
    fi
fi

# sidecar `<fichier>.jeu` : la galerie y lit le nom du jeu (et le logo pour la
# copie avec logo). Écrit même quand gpad-shot échoue (repli spectacle).
ecrire_sidecar() {
    local f=$1
    [[ -n "$titre" ]] || return 0
    { printf 'nom=%s\n' "$titre"
      [[ -n "$logo" ]] && printf 'logo=%s\n' "$logo"
    } > "$f.jeu" 2>/dev/null || true
}

# son de déclencheur d'abord : sans lui on ne sait pas si la capture est partie
# (le son ne peut pas se retrouver dans un PNG, rien ne justifie de l'attendre)
w="$HERE/sounds/shot.wav"
[[ -f "$w" ]] && command -v ffplay > /dev/null && \
    setsid ffplay -nodisp -autoexit -loglevel quiet "$w" > /dev/null 2>&1 < /dev/null &

# outil maison : binaire à côté du script au banc, /usr/bin dans l'image
# (GPAD_SHOT exporté par l'unité, en priorité)
SHOT=${GPAD_SHOT:-}
if [[ -z "$SHOT" && -x "$HERE/gpad-shot" ]]; then
    SHOT="$HERE/gpad-shot"
fi
if [[ -z "$SHOT" ]]; then
    SHOT=$(command -v gpad-shot)
fi

capture_ok=0
if [[ -n "$SHOT" ]]; then
    if [[ -n "$titre" ]]; then
        "$SHOT" --titre="$titre" "$f" && capture_ok=1
    else
        "$SHOT" "$f" && capture_ok=1
    fi
fi
if [[ $capture_ok == 1 ]]; then
    ecrire_sidecar "$f"
    echo "capture : $f"
    exit 0
fi

# repli : spectacle (interface KWin absente, compositeur non-KWin…)
if ! command -v spectacle > /dev/null 2>&1; then
    echo "ECHEC: ni gpad-shot ni spectacle"
    exit 1
fi

if [[ -n "$SHOT" ]]; then
    echo "gpad-shot a échoué, repli sur spectacle"
else
    echo "gpad-shot absent, repli sur spectacle"
fi
# spectacle -b : pas de fenêtre ; l'option -b rend la main une fois la capture faite
spectacle -f -b -n -o "$f"

if [[ -f "$f" ]]; then
    ecrire_sidecar "$f"
    echo "capture : $f"
else
    echo "ECHEC: fichier non créé ($f)"
    exit 1
fi

exit 0
