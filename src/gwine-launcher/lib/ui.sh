#!/bin/bash

################################################################################
# ui.sh - Interface utilisateur : progression via Gablue Manager, GameMode
#
# gwine n'utilise plus kdialog. Ses fenêtres (progression, confirmation, choix
# de l'exécutable d'un WGP) passent par `gablue-manager`, qui ouvrira une seule
# boîte sans sa fenêtre principale. Sans session graphique ou sans le manager,
# tout retombe sur le terminal (et jamais sur une boîte muette).
################################################################################

# Préfixe du protocole lu par `gablue-manager --progress` (local.parse_gablue_progress).
GWINE_PROGRESS_PREFIX="::gablue:"

# Gablue Manager est-il utilisable ici ? (session graphique + binaire présent)
gui_available() {
    [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && command -v gablue-manager >/dev/null 2>&1
}

# Un parent (Gablue Manager) attend-il notre progression sur la sortie ?
progress_enabled() {
    [ "${GABLUE_PROGRESS:-0}" = "1" ]
}

# Étape n/total : ligne lisible pour le journal + événement pour la barre.
# Usage: progress_step <n> <total> <libellé>
progress_step() {
    local n="$1" total="$2" label="$3"
    printf '[%s/%s] %s\n' "$n" "$total" "$label" >&2
    if progress_enabled; then
        printf '%sstep %s %s %s\n' \
            "$GWINE_PROGRESS_PREFIX" "$n" "$total" "$label" >&2
    fi
}

# Travail en cours dont le total est inconnu (téléchargement) : la barre du
# manager balaie de gauche à droite. Usage: progress_busy <libellé>
progress_busy() {
    printf '%s\n' "$1" >&2
    if progress_enabled; then
        printf '%sbusy %s\n' "$GWINE_PROGRESS_PREFIX" "$1" >&2
    fi
}

# Fin de l'opération : la barre passe à 100 %. Usage: progress_done [libellé]
progress_done() {
    local label="${1:-Terminé.}"
    if progress_enabled; then
        printf '%sdone %s\n' "$GWINE_PROGRESS_PREFIX" "$label" >&2
    fi
}

# Pose une question oui/non. Gablue Manager si possible, sinon terminal.
# Usage: ui_confirm <titre> <texte> [libellé_oui] [libellé_non]
# Retourne 0 pour oui, 1 pour non (ou pas de moyen de demander).
ui_confirm() {
    local title="$1"
    local text="${2//\\n/$'\n'}"
    local yes_label="${3:-Oui}" no_label="${4:-Non}"

    if gui_available; then
        if gablue-manager --confirm --title "$title" --text "$text" \
            --yes "$yes_label" --no "$no_label"; then
            return 0
        fi
        return 1
    fi

    if [ -t 0 ]; then
        printf '%s\n' "$text"
        local reply=""
        read -r -p "Confirmer ? (o/N) : " reply
        [[ "$reply" =~ ^[oOyY]$ ]] && return 0
    fi
    return 1
}

# Fait choisir une valeur parmi plusieurs. Gablue Manager si possible, sinon
# menu numéroté sur le terminal. Imprime le choix sur la sortie standard.
# Usage: ui_choose <titre> <texte> <valeur> [valeur…]
# Retourne 1 si annulé ou sans moyen de choisir.
ui_choose() {
    local title="$1" text="$2"
    shift 2
    local -a values=("$@")
    [ "${#values[@]}" -eq 0 ] && return 1

    if gui_available; then
        local -a args=()
        local value
        for value in "${values[@]}"; do
            args+=(--item "$value")
        done
        local choice
        choice=$(gablue-manager --choose --title "$title" --text "$text" \
            "${args[@]}") || return 1
        printf '%s\n' "$choice"
        return 0
    fi

    if [ -t 0 ]; then
        printf '%s\n\n' "$text"
        local index
        for index in "${!values[@]}"; do
            printf '  %s. %s\n' "$((index + 1))" "${values[$index]}"
        done
        echo ""
        local reply=""
        read -r -p "Numéro : " reply
        if [[ "$reply" =~ ^[0-9]+$ ]] && [ "$reply" -ge 1 ] && \
           [ "$reply" -le "${#values[@]}" ]; then
            printf '%s\n' "${values[$((reply - 1))]}"
            return 0
        fi
    fi
    return 1
}

# Lance une commande dans la fenêtre de progression de Gablue Manager.
# Usage: run_with_progress <titre> <sous-titre> -- <commande…>
# Sans manager (ou déjà piloté par lui), la commande tourne directement.
run_with_progress() {
    local title="$1" subtitle="$2"
    shift 2
    [ "${1:-}" = "--" ] && shift

    if gui_available && ! progress_enabled; then
        GABLUE_PROGRESS=1 gablue-manager --progress \
            --title "$title" --subtitle "$subtitle" \
            --auto-close-success -- "$@"
        return $?
    fi
    "$@"
}

# Vérifie si gamemode est actuellement actif
is_gamemode_active() {
    if command -v gamemoded &>/dev/null; then
        local status
        status=$(gamemoded -s 2>/dev/null)
        if echo "$status" | grep -q "is active"; then
            return 0
        fi
    fi
    return 1
}

# Active gamemode et garde le processus en vie
gamemode_start() {
    if [ "${DISABLE_GAMEMODE:-0}" = "1" ]; then
        return 0
    fi

    if ! command -v gamemoded &>/dev/null; then
        return 0
    fi

    # Lancer gamemoded -r qui bloque et maintient gamemode actif
    # Ce processus doit rester en vie pendant tout le jeu
    # Rediriger stdin pour éviter qu'il ne bloque sur l'entrée du terminal
    gamemoded -r </dev/null &>/dev/null &
    _GWINE_GAMEMODE_PID=$!
    sleep 0.1

    if is_gamemode_active; then
        echo "GameMode activé (PID: $_GWINE_GAMEMODE_PID)"
    fi
}

# Arrête gamemode en tuant le processus
gamemode_stop() {
    if [ -n "$_GWINE_GAMEMODE_PID" ] && kill -0 "$_GWINE_GAMEMODE_PID" 2>/dev/null; then
        echo "Désactivation de GameMode..."
        kill "$_GWINE_GAMEMODE_PID" 2>/dev/null
        wait "$_GWINE_GAMEMODE_PID" 2>/dev/null
        _GWINE_GAMEMODE_PID=""
        echo "GameMode désactivé"
    fi
}
