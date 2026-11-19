#!/usr/bin/bash
# test-logo.sh — banc du logo de jeu dans l'overlay manette (gpad).
#
# Le logo vient du cache de glauncher
# (`~/.local/share/glauncher/assets/logos/<slug>.png`) et n'est affiché QUE si
# `running.txt` — écrit par glauncher pendant une partie et retiré à la fin —
# contient le PID de la fenêtre active. Ce banc n'a donc pas besoin d'un jeu :
# il publie ce fichier pour une fenêtre connue, puis ouvre l'overlay (sans grab,
# il se referme seul).
#
#   ./test-logo.sh                fenêtre du terminal courant + logo réel du cache
#   ./test-logo.sh --jeu          AUCUNE publication : overlay sur le running.txt
#                                 du vrai jeu en cours (glauncher, ou gwine /
#                                 lgplaunch qui publient le paquet) ; bascule
#                                 sur sa fenêtre, ouverture après 6 s
#   ./test-logo.sh --pid 1234     publie pour une fenêtre déjà ouverte (un vrai jeu)
#   ./test-logo.sh --reel         ne publie rien : compile les deux binaires et
#                                 affiche la marche à suivre avec un vrai jeu
#
# Options : --logo <png> (défaut : le premier logo du cache), --slug <nom>,
#           --nom <libellé> (défaut : nom du fichier de logo), --icone <png>,
#           --duree <s> (panneau ouvert N s, défaut 12), --attendre <s>
#           (compte à rebours de --jeu, défaut 6), --no-shot (pas de capture
#           spectacle), --force (remplace un `running.txt` d'un vrai jeu).
#
# Aucun grab : la manette reste au jeu, le panneau s'ouvre et se ferme seul.
set -euo pipefail

HERE="$(dirname "$(readlink -f "$0")")"
CACHE="${GLAUNCHER_CACHE:-$HOME/.local/share/glauncher}"
RUNNING="$CACHE/running.txt"
# publication de gwine/lgplaunch (paquet lancé sans glauncher) : même nom de
# fichier, dans le dossier d'exécution de la session
LIVE="${XDG_RUNTIME_DIR:-/tmp/gablue-$UID}/gablue/running.txt"
SLUG=test-logo
NOM=""
PID=""
LOGO=""
ICONE=""
REEL=0
JEU=0
FAKE=0
FORCE=0
DUREE=12
ATTENTE=6
SHOT=1

while [[ $# -gt 0 ]]; do
    case "$1" in
        --pid) PID="$2"; FAKE=1; shift 2 ;;
        --logo) LOGO="$2"; FAKE=1; shift 2 ;;
        --icone) ICONE="$2"; FAKE=1; shift 2 ;;
        --slug) SLUG="$2"; FAKE=1; shift 2 ;;
        --nom) NOM="$2"; FAKE=1; shift 2 ;;
        --duree) DUREE="$2"; shift 2 ;;
        --attendre) ATTENTE="$2"; shift 2 ;;
        --jeu) JEU=1; shift ;;
        --reel) REEL=1; shift ;;
        --no-shot) SHOT=0; shift ;;
        --force) FORCE=1; shift ;;
        -h|--help)
            sed -n '2,/^set /p' "$0" | sed -e 's/^# \{0,1\}//' -e '/^set /d'
            exit 0 ;;
        *) echo "option inconnue : $1" >&2; exit 2 ;;
    esac
done

# Le dépôt cargo global écrit HORS du dépôt : le chemin des binaires se demande
# à cargo, jamais supposé.
cible_cargo() {
    ( cd "$1" && cargo metadata --format-version 1 --no-deps 2>/dev/null |
        python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])' )
}

# PID du terminal qui porte ce script : c'est lui la fenêtre active quand on
# appuie sur HOME depuis la même fenêtre, donc le cas le plus simple à tester.
pid_terminal() {
    local pid=$$ stat rest state ppid comm
    while [[ "$pid" -gt 1 ]]; do
        comm=$(cat "/proc/$pid/comm" 2>/dev/null || true)
        case "$comm" in
            konsole|yakuake|alacritty|kitty|foot|xterm|tilix|wezterm*)
                echo "$pid"; return 0 ;;
        esac
        stat=$(cat "/proc/$pid/stat" 2>/dev/null) || return 1
        rest=${stat#*') '}
        read -r state ppid _ <<< "$rest"
        [[ "$ppid" =~ ^[0-9]+$ ]] || return 1
        pid=$ppid
    done
    return 1
}

premier_logo() {
    find "$CACHE/assets/logos" -maxdepth 1 -name '*.png' -print -quit 2>/dev/null
}

compiler_overlay() {
    echo "== compilation de l'overlay (release)…"
    ( cd "$HERE/gpad" && cargo build --release )
    OVERLAY="$(cible_cargo "$HERE/gpad")/release/gpad"
    [[ -x "$OVERLAY" ]] || { echo "overlay non compilé : $OVERLAY" >&2; exit 1; }
    echo "   $OVERLAY"
}

compiler_launcher() {
    echo "== compilation de glauncher (release)…"
    ( cd "$HERE/../glauncher" && cargo build --release )
    LAUNCHER="$(cible_cargo "$HERE/../glauncher")/release/glauncher"
    [[ -x "$LAUNCHER" ]] || { echo "launcher non compilé : $LAUNCHER" >&2; exit 1; }
    echo "   $LAUNCHER"
}

# ------------------------------------------------------------------ mode réel
if [[ "$REEL" == 1 ]]; then
    compiler_overlay
    compiler_launcher
    cat <<EOF

== à toi de jouer (vrai jeu, rien de factice)
   1. lance :  $LAUNCHER --windowed
   2. ouvre un jeu et attends sa fenêtre (plein écran forcé par KWin).
   3. manette : HOME pendant que le jeu est au premier plan.
   4. le logo du jeu doit s'afficher AU-DESSUS de « FENETRE ACTIVE » ;
      dans le journal de l'overlay : LOGO …/assets/logos/<slug>.png (<slug>)
   5. le jeu quitté (ou une autre fenêtre active), plus aucun bandeau.

   Vérifier ce que glauncher publie, pendant la partie :
      cat $RUNNING

EOF
    exit 0
fi

# ------------------------------------------------------------- mode factice
compiler_overlay

# Un vrai jeu tourne déjà et rien de factice n'a été demandé : le banc bascule
# tout seul en mode --jeu (aucune écriture, on se sert du `running.txt` réel).
# Refuser sec obligeait à comprendre l'option avant de pouvoir tester.
if [[ "$JEU" == 0 && "$REEL" == 0 && "$FAKE" == 0 ]] &&
    { [[ -f "$LIVE" ]] || { [[ -f "$RUNNING" ]] && ! grep -q "^slug=$SLUG\$" "$RUNNING"; }; }; then
    echo "un jeu tourne déjà — mode --jeu automatique (aucune écriture)"
    JEU=1
fi

# --jeu : on ne publie RIEN, on se sert du `running.txt` du vrai jeu en cours.
# C'est le seul moyen de voir le logo aujourd'hui : l'overlay du service est
# encore celui de l'image (ancien binaire, sans logo).
if [[ "$JEU" == 1 ]]; then
    # glauncher (slug + visuels résolus) ou gwine/lgplaunch (nom + paquet)
    JEU_FICHIER="$LIVE"
    [[ -f "$JEU_FICHIER" ]] || JEU_FICHIER="$RUNNING"
    [[ -f "$JEU_FICHIER" ]] || {
        echo "aucun jeu publié ($LIVE ni $RUNNING)" >&2
        echo "lance un jeu depuis glauncher, ou un .wgp/.lgp avec gwine/lgplaunch" >&2
        exit 1
    }
    echo "== jeu publié ($JEU_FICHIER) :"
    sed 's/^/   /' "$JEU_FICHIER"
    echo
    echo "Bascule sur la FENÊTRE DU JEU (Alt-Tab) : l'overlay s'ouvre dans"
    echo "${ATTENTE} s et se referme après ${DUREE} s. Aucune touche à presser."
    for reste in $(seq "$ATTENTE" -1 1); do
        printf '\r   ouverture dans %d s… ' "$reste"
        sleep 1
    done
    echo
    OUT=/tmp/opencode/logo-pad.png
    if [[ "$SHOT" == 1 ]] && command -v spectacle > /dev/null; then
        ( sleep 2; spectacle -b -n -o "$OUT" > /dev/null 2>&1 ) &
    fi
    KWIN_SCRIPTS="$HERE/kwin" SCRIPTS="$HERE/scripts" \
        "$OVERLAY" --no-grab --seconds "$DUREE"
    if [[ -f "$OUT" ]]; then
        echo "capture : $OUT"
    fi
    exit 0
fi

if [[ -z "$LOGO" ]]; then
    LOGO="$(premier_logo || true)"
fi
[[ -n "$LOGO" && -f "$LOGO" ]] || {
    echo "aucun logo dans $CACHE/assets/logos — choisis-en un dans le" >&2
    echo "launcher (page du jeu → Images ▸ Logo) ou passe --logo <png>." >&2
    exit 1
}

# Nom affiché et icône : le launcher les publie en vrai (nom du paquet, icône
# extraite du .wgp/.lgp). Au banc, on prend un nom lisible et un PNG du cache.
if [[ -z "$NOM" ]]; then
    NOM="$(basename "$LOGO" .png)"
fi
if [[ -z "$ICONE" ]]; then
    ICONE="$(find "$CACHE/pack-icons" "$CACHE/assets/icons" -maxdepth 1 -name '*.png' \
        -print -quit 2>/dev/null || true)"
fi

if [[ -z "$PID" ]]; then
    PID="$(pid_terminal || true)"
fi
if [[ -z "$PID" ]]; then
    echo "fenêtre introuvable : relance depuis Konsole, ou passe --pid <pid>" >&2
    exit 1
fi
[[ -d "/proc/$PID" ]] || { echo "PID $PID inexistant" >&2; exit 1; }

# Un vrai jeu tourne déjà : on ne lui vole pas son `running.txt`.
if [[ -f "$RUNNING" && "$FORCE" != 1 ]]; then
    if ! grep -q "^slug=$SLUG\$" "$RUNNING" 2>/dev/null; then
        echo "un jeu publie déjà $RUNNING :" >&2
        sed 's/^/   /' "$RUNNING" >&2
        echo "   → teste avec ce jeu (--pid <pid de sa fenêtre>), ou --force" >&2
        exit 1
    fi
fi

mkdir -p /tmp/opencode
SAUVE="$(mktemp "/tmp/opencode/running-sauve.XXXXXX")"
if [[ -f "$RUNNING" ]]; then cp "$RUNNING" "$SAUVE"; else rm -f "$SAUVE"; fi
propre() {
    # On ne retire que NOTRE fichier : jamais celui d'un vrai jeu.
    if [[ -f "$RUNNING" ]] && head -n 1 "$RUNNING" | grep -q "^slug=$SLUG\$"; then
        if [[ -s "$SAUVE" ]]; then cp "$SAUVE" "$RUNNING"; else rm -f "$RUNNING"; fi
    fi
    rm -f "$SAUVE"
}
trap propre EXIT INT TERM

mkdir -p "$CACHE"
{
    echo "slug=$SLUG"
    echo "nom=$NOM"
    echo "pids=$PID"
    echo "logo=$LOGO"
    if [[ -n "$ICONE" ]]; then
        echo "icone=$ICONE"
    fi
} > "$RUNNING"

echo "== état publié (ce que lit l'overlay) :"
sed 's/^/   /' "$RUNNING"
echo "== fenêtre visée : PID $PID ($(cat "/proc/$PID/comm" 2>/dev/null))"
echo
echo "L'overlay s'ouvre tout seul (aucun grab : la manette reste au jeu)."
echo "Garde CETTE fenêtre au premier plan — le logo doit apparaître au-dessus"
echo "de « FENETRE ACTIVE ». Il se referme après ${DUREE} s (--duree pour plus)."
echo

# Capture pendant que le panneau est là : c'est la preuve visuelle (et elle
# permet de comparer deux réglages). `sleep` court dans le sous-shell, pas ici.
OUT=/tmp/opencode/logo-pad.png
if [[ "$SHOT" == 1 ]] && command -v spectacle > /dev/null; then
    ( sleep 2; spectacle -b -n -o "$OUT" > /dev/null 2>&1 ) &
    echo "capture en cours → $OUT"
fi

# KWIN_SCRIPTS est indispensable au banc : sans lui, `chemin_kwin` ne trouve
# pas `wininfo.js` à côté d'un binaire resté dans le cache cargo, et l'overlay
# n'a alors AUCUNE fenêtre active (donc jamais de logo).
KWIN_SCRIPTS="$HERE/kwin" \
    SCRIPTS="$HERE/scripts" \
    "$OVERLAY" --no-grab --seconds "$DUREE"

if [[ -f "$OUT" ]]; then
    echo "capture : $OUT"
fi
