#!/bin/bash

################################################################################
# running.sh - jeu en cours publié pour l'overlay manette (gpad)
#
# `gpad` affiche le nom et l'icône du **paquet** dans son en-tête, et son logo
# au-dessus, quand la fenêtre active appartient au jeu lancé. Le lien entre une
# fenêtre et un jeu ne peut pas être la classe X11 (une fenêtre de jeu natif ou
# Wine ne dit rien du `.lgp` qui l'a lancé) : lgplaunch publie donc les PID du
# jeu et gpad les compare à celui de la fenêtre active — directement ou par
# descendance (le jeu tourne en avant-plan, donc sous ce processus).
#
# Le fichier vit dans le dossier d'exécution de la session, il est retiré à la
# fin du jeu s'il est encore le nôtre. Même module dans gwine : les deux
# lanceurs sont autonomes, aucun helper de l'image n'est requis.
################################################################################

# Dossier où publier : le dossier d'exécution de la session (disparaît à la
# déconnexion), avec un repli par utilisateur quand il n'y en a pas (ssh, cron).
running_dir() {
    if [ -n "${XDG_RUNTIME_DIR:-}" ] && [ -d "${XDG_RUNTIME_DIR:-}" ]; then
        printf '%s/gablue\n' "$XDG_RUNTIME_DIR"
    else
        printf '/tmp/gablue-%s\n' "$(id -u)"
    fi
}

running_file() { printf '%s/running.txt\n' "$(running_dir)"; }

# running_publish <nom> <paquet|""> <pid> [pid…]
#
# Écriture atomique (`.part` + renommage) : gpad peut lire à tout instant.
running_publish() {
    local nom="$1"
    local paquet="${2:-}"
    shift 2 || return 0
    [ $# -gt 0 ] || return 0

    local dir
    dir="$(running_dir)"
    mkdir -p "$dir" 2>/dev/null || return 0

    local pids
    pids="$(printf '%s' "$*" | tr ' ' ',')"
    local fichier="$dir/running.txt"
    local tmp="$fichier.part"

    {
        printf 'nom=%s\n' "$nom"
        [ -n "$paquet" ] && printf 'paquet=%s\n' "$paquet"
        printf 'pids=%s\n' "$pids"
    } > "$tmp" 2>/dev/null || return 0

    if mv -f "$tmp" "$fichier" 2>/dev/null; then
        RUNNING_PIDS="$pids"
        RUNNING_FILE="$fichier"
        echo "Overlay: jeu publié ($nom, pid $pids)"
    else
        rm -f "$tmp" 2>/dev/null
    fi
}

# Retire le fichier, mais seulement s'il porte encore NOTRE liste de PID : deux
# jeux peuvent tourner en parallèle, on ne vole pas la publication de l'autre.
running_clear() {
    local fichier="${RUNNING_FILE:-$(running_file)}"
    [ -n "${RUNNING_PIDS:-}" ] || return 0
    [ -f "$fichier" ] || return 0
    if grep -qxF "pids=$RUNNING_PIDS" "$fichier" 2>/dev/null; then
        rm -f "$fichier" 2>/dev/null
    fi
}

# running_fetch_art <paquet>
#
# Demande à glauncher de télécharger le logo et l'icône **manquants** du paquet
# (dans SON cache, en PNG) : gpad les relit à chaque passe et les fait
# apparaître dès qu'ils sont écrits. Rien n'est téléchargé par l'overlay, et
# rien n'est fait si le paquet n'a pas de visuel disponible. Détaché : le jeu
# ne doit pas attendre le réseau, et le téléchargement survit au lanceur.
running_fetch_art() {
    local paquet="${1:-}"
    [ -n "$paquet" ] && [ -f "$paquet" ] || return 0
    local bin="${GLAUNCHER_BIN:-}"
    if [ -z "$bin" ]; then
        bin="$(command -v glauncher 2>/dev/null)" || true
    fi
    [ -n "$bin" ] && [ -x "$bin" ] || return 0
    setsid "$bin" --art "$paquet" </dev/null >/dev/null 2>&1 &
    echo "Overlay: visuels manquants demandés à glauncher"
}
