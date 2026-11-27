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
#  - `<cache du launcher>/running.txt` : écrit pendant une partie
#    (`nom` = nom de la bibliothèque, `logo` = chemin du logo), et supprimé à
#    la fin du jeu ;
#  - `$XDG_RUNTIME_DIR/gablue/running.txt` : écrit par `gwine`/`lgplaunch`
#    (nom du paquet) pour les lancements hors launcher.
# Les PID servent de garde : un fichier qui traîne après un plantage ne doit
# pas étiqueter une capture de bureau avec le dernier jeu.
RT=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
# Dossier de cache du launcher : `GLAUNCHER_CACHE`, puis `cache_dir` de
# `~/.config/glauncher/config.toml`, puis le défaut `~/.local/share/glauncher`.
# C'est LÀ qu'il publie `running.txt` pendant une partie. Ne jamais y remettre
# `~/.cache/glauncher` : ce chemin n'a jamais existé, donc `titre` restait vide,
# donc aucun sidecar n'était posé et la galerie perdait le nom du jeu ET l'option
# « copie avec le logo » (constaté le 27/11, captures du 26/11 sans `.jeu`).
cache_launcher() {
    local conf valeur=""
    if [[ -n "${GLAUNCHER_CACHE:-}" ]]; then
        printf '%s\n' "$GLAUNCHER_CACHE"
        return 0
    fi
    conf="${XDG_CONFIG_HOME:-$HOME/.config}/glauncher/config.toml"
    if [[ -f "$conf" ]]; then
        valeur=$(sed -n 's/^[[:space:]]*cache_dir[[:space:]]*=[[:space:]]*//p' "$conf" | tail -1)
        valeur="${valeur%\"}"; valeur="${valeur#\"}"
        valeur="${valeur%\'}"; valeur="${valeur#\'}"
    fi
    case "$valeur" in
        "") printf '%s\n' "$HOME/.local/share/glauncher" ;;
        "~/"*) printf '%s\n' "$HOME/${valeur#"~/"}" ;;
        *) printf '%s\n' "$valeur" ;;
    esac
}
champ() { sed -n "s/^$2=//p" "$1" 2>/dev/null | tail -1; }
pids_vivants() {
    local p q=0
    for p in ${1//,/ }; do kill -0 "$p" 2>/dev/null && q=1; done
    [[ $q == 1 ]]
}
titre=""; logo=""
run_launcher="$(cache_launcher)/running.txt"
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

# Le nom du jeu et le logo voyagent DANS l'image — iTXt `Title`/`Logo` d'un
# PNG, commentaire `Title=`/`Logo=` d'un JPEG — et jamais dans un fichier à
# côté (demandé le 27/11) : une capture seule reste étiquetée, et la galerie
# affiche le nom et propose « copie avec le logo » sans rien d'autre à
# transporter. Le repli `spectacle` n'a pas de métadonnée à écrire : sa
# capture reste sans nom, la galerie affiche alors date et poids.

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
    meta=()
    [[ -n "$titre" ]] && meta+=("--titre=$titre")
    [[ -n "$logo" ]] && meta+=("--logo=$logo")
    "$SHOT" "${meta[@]}" "$f" && capture_ok=1
fi
if [[ $capture_ok == 1 ]]; then
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
    echo "capture : $f (sans métadonnée : repli spectacle)"
else
    echo "ECHEC: fichier non créé ($f)"
    exit 1
fi

exit 0
