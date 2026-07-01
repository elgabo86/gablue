#!/bin/bash

################################################################################
# ui.sh - Interface utilisateur (kdialog, barres de progression, GameMode)
################################################################################

# Détecte la commande qdbus disponible
_get_qdbus_cmd() {
    if command -v qdbus6 &>/dev/null; then
        echo "qdbus6"
    elif command -v qdbus-qt5 &>/dev/null; then
        echo "qdbus-qt5"
    elif command -v qdbus &>/dev/null; then
        echo "qdbus"
    else
        echo ""
    fi
}

# Crée une barre de progression kdialog
# Usage: progress_create "Titre" nombre_étapes [show_cancel] [force]
# Paramètre show_cancel: "true" pour afficher le bouton Annuler (défaut: true)
# Paramètre force: "true" pour forcer kdialog même sans --kdialog (défaut: false)
# Retourne la référence dbus via stdout
progress_create() {
    local title="$1"
    local steps="${2:-10}"
    local show_cancel="${3:-true}"
    local force="${4:-false}"
    
    # Ne pas créer de barre de progression si kdialog n'est pas explicitement activé
    # sauf si force=true (pour création auto du préfixe)
    if [ "$_USE_KDIALOG" != "true" ] && [ "$force" != "true" ]; then
        return
    fi
    
    local qdbus_cmd
    qdbus_cmd=$(_get_qdbus_cmd)
    
    if command -v kdialog &>/dev/null && [ -n "$qdbus_cmd" ]; then
        local dbusRef
        dbusRef=$(kdialog --title "$title" --progressbar "Initialisation..." "$steps" 2>/dev/null)
        # Nettoyer la référence (enlever les retours à la ligne et espaces)
        dbusRef=$(printf '%s' "$dbusRef" | tr -d '\n\r' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
        if [ -n "$dbusRef" ]; then
            # Stocker dans la variable globale pour pouvoir fermer lors d'interruption
            _PROGRESS_DBUS_REF="$dbusRef"
            # Split pour configurer le dialogue
            local service path
            read service path <<< "$(_split_dbus_ref "$dbusRef")"
            # Activer/désactiver le bouton Annuler selon le paramètre
            if [ "$show_cancel" = "true" ]; then
                $qdbus_cmd "$service" "$path" showCancelButton true >/dev/null 2>&1 || true
            fi
            printf '%s' "$dbusRef"
        fi
    fi
}

# Met à jour la barre de progression
# Usage: progress_update dbus_ref valeur "Message"
progress_update() {
    local dbusRef="$1"
    local value="$2"
    local message="$3"
    
    local qdbus_cmd
    qdbus_cmd=$(_get_qdbus_cmd)
    
    if [ -n "$dbusRef" ] && [ -n "$qdbus_cmd" ]; then
        local service path
        read service path <<< "$(_split_dbus_ref "$dbusRef")"
        $qdbus_cmd "$service" "$path" Set "" value "$value" >/dev/null 2>&1 || true
        if [ -n "$message" ]; then
            $qdbus_cmd "$service" "$path" setLabelText "$message" >/dev/null 2>&1 || true
        fi
    fi
}

# Ferme la barre de progression
# Usage: progress_close dbus_ref
progress_close() {
    local dbusRef="$1"
    
    local qdbus_cmd
    qdbus_cmd=$(_get_qdbus_cmd)
    
    if [ -n "$dbusRef" ] && [ -n "$qdbus_cmd" ]; then
        local service path
        read service path <<< "$(_split_dbus_ref "$dbusRef")"
        $qdbus_cmd "$service" "$path" close >/dev/null 2>&1 || true
    fi
    # Réinitialiser la variable globale
    _PROGRESS_DBUS_REF=""
}

# Split dbusRef into service and path
_split_dbus_ref() {
    local dbusRef="$1"
    # Split on space: "org.kde.kdialog-xxx /ProgressDialog" -> service path
    local service=$(echo "$dbusRef" | awk '{print $1}')
    local path=$(echo "$dbusRef" | awk '{print $2}')
    echo "$service $path"
}

# Vérifie si l'utilisateur a cliqué sur Annuler
# Usage: progress_is_cancelled dbus_ref
# Retourne 0 si annulé, 1 sinon
# Note: quand kdialog est fermé (Annuler), le service DBus disparaît
progress_is_cancelled() {
    local dbusRef="$1"
    
    local qdbus_cmd
    qdbus_cmd=$(_get_qdbus_cmd)
    
    if [ -n "$dbusRef" ] && [ -n "$qdbus_cmd" ]; then
        # Split the reference
        local service path
        read service path <<< "$(_split_dbus_ref "$dbusRef")"
        
        # Vérifier wasCancelled - si le service n'existe pas, ça retourne vide
        local cancelled
        cancelled=$($qdbus_cmd "$service" "$path" wasCancelled 2>/dev/null)
        if [ "$cancelled" = "true" ] || [ -z "$cancelled" ]; then
            # true = bouton annuler cliqué
            # vide = dialogue fermé = annulé
            return 0
        fi
    fi
    return 1
}

# Gère l'annulation par l'utilisateur avec fermeture de la barre de progression
# Usage: handle_cancellation dbus_ref [--with-backup] [message_personnalisé]
handle_cancellation() {
    local dbus_ref="$1"
    local with_backup=false
    local custom_message="Initialisation annulée par l'utilisateur"
    
    shift
    # Vérifier les options
    while [ $# -gt 0 ]; do
        case "$1" in
            --with-backup)
                with_backup=true
                ;;
            *)
                custom_message="$1"
                ;;
        esac
        shift
    done
    
    # Restaurer le backup si demandé
    if [ "$with_backup" = true ]; then
        restore_backup 2>/dev/null || true
    fi
    
    progress_close "$dbus_ref"
    echo "$custom_message"
    if [ "$_USE_KDIALOG" = "true" ] && command -v kdialog &>/dev/null; then
        kdialog --title "Initialisation annulée" --msgbox "L'initialisation du préfixe a été annulée.\n\nL'ancien préfixe a été restauré."
    fi
    exit 0
}

# Vérifie si l'utilisateur a annulé et gère l'annulation si nécessaire
# Usage: check_progress_cancelled <dbus_ref> [--with-backup] [message_personnalisé]
# Retourne 0 si annulé (ne revient pas car exit), 1 sinon
check_progress_cancelled() {
    local dbus_ref="$1"
    shift
    
    if [ -z "$dbus_ref" ]; then
        return 1
    fi
    
    if ! progress_is_cancelled "$dbus_ref"; then
        return 1
    fi
    
    handle_cancellation "$dbus_ref" "$@"
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
