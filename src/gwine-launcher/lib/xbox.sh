#!/bin/bash

################################################################################
# xbox.sh - Gestion de ds2xbox pour émulation manette Xbox 360
################################################################################

_DS2XBOX_PID=""
_DS2XBOX_BIN=""

XBOX_CONFIG_FILE="$GWINE_DIR/options"

DS2XBOX_SONY_IDS_ALL="0x054c/0x0ce6,0x054c/0x0df2,0x054c/0x05c4,0x054c/0x09cc,0x054c/0x0ba0"
DS2XBOX_SONY_IDS_DS4="0x054c/0x05c4,0x054c/0x09cc,0x054c/0x0ba0"
DS2XBOX_SONY_IDS_DUALSENSE="0x054c/0x0ce6,0x054c/0x0df2"

get_xbox_sdl_ignore_ids() {
    case "${xbox_filter:-all}" in
        ds4)        echo "$DS2XBOX_SONY_IDS_DS4" ;;
        dualsense)  echo "$DS2XBOX_SONY_IDS_DUALSENSE" ;;
        *)          echo "$DS2XBOX_SONY_IDS_ALL" ;;
    esac
}

get_ds2xbox_args() {
    case "${xbox_filter:-all}" in
        ds4)        echo "--ds4" ;;
        dualsense)  echo "--dualsense" ;;
        *)          echo "" ;;
    esac
}

get_xbox_default() {
    if [ -f "$XBOX_CONFIG_FILE" ]; then
        local val
        val=$(grep "^xbox_default=" "$XBOX_CONFIG_FILE" 2>/dev/null | cut -d'=' -f2)
        if [ "$val" = "on" ]; then
            echo "on"
            return 0
        fi
    fi
    echo "off"
}

get_xbox_default_filter() {
    if [ -f "$XBOX_CONFIG_FILE" ]; then
        local val
        val=$(grep "^xbox_filter=" "$XBOX_CONFIG_FILE" 2>/dev/null | cut -d'=' -f2)
        if [ "$val" = "ds4" ] || [ "$val" = "dualsense" ]; then
            echo "$val"
            return 0
        fi
    fi
    echo "all"
}

set_xbox_default() {
    local state="$1"
    local filter="${2:-all}"

    if [ "$state" != "on" ] && [ "$state" != "off" ]; then
        echo "Erreur: valeur xbox_default invalide '$state'. Utilisez 'on' ou 'off'." >&2
        return 1
    fi

    ensure_dir "$GWINE_DIR"

    local config_content=""
    if [ -f "$XBOX_CONFIG_FILE" ]; then
        config_content=$(grep -v "^xbox_default=" "$XBOX_CONFIG_FILE" 2>/dev/null | grep -v "^xbox_filter=" || true)
    fi

    if [ -n "$config_content" ]; then
        echo "$config_content" > "$XBOX_CONFIG_FILE"
        echo "xbox_default=$state" >> "$XBOX_CONFIG_FILE"
        echo "xbox_filter=$filter" >> "$XBOX_CONFIG_FILE"
    else
        echo "xbox_default=$state" > "$XBOX_CONFIG_FILE"
        echo "xbox_filter=$filter" >> "$XBOX_CONFIG_FILE"
    fi

    return 0
}

apply_xbox_default() {
    local default_state
    default_state=$(get_xbox_default)

    if [ "$default_state" = "on" ] && [ "$xbox_mode" != true ]; then
        xbox_mode=true
        xbox_filter=$(get_xbox_default_filter)
    fi
}

find_ds2xbox() {
    if [ -x "/usr/bin/ds2xbox" ]; then
        echo "/usr/bin/ds2xbox"
        return 0
    fi

    if [ -x "$SCRIPT_DIR/ds2xbox" ]; then
        echo "$SCRIPT_DIR/ds2xbox"
        return 0
    fi

    return 1
}

start_ds2xbox() {
    _DS2XBOX_BIN=$(find_ds2xbox)

    if [ -z "$_DS2XBOX_BIN" ]; then
        echo "Erreur: ds2xbox introuvable" >&2
        return 1
    fi

    ensure_dir -s "$GWINE_LOCK_DIR"

    local count_file="$GWINE_LOCK_DIR/xbox-instance-count"
    local pids_file="$GWINE_LOCK_DIR/xbox-instance-pids"
    local bin_pid_file="$GWINE_LOCK_DIR/xbox-ds2xbox-pid"
    local sync_lock="$GWINE_LOCK_DIR/.xbox.lock"

    exec 201>"$sync_lock"
    flock 201

    local count=0
    if [ -f "$count_file" ]; then
        count=$(cat "$count_file" 2>/dev/null || echo "0")
    fi

    local existing_pid=""
    if [ -f "$bin_pid_file" ]; then
        existing_pid=$(cat "$bin_pid_file" 2>/dev/null || echo "")
    fi

    if [ -n "$existing_pid" ] && kill -0 "$existing_pid" 2>/dev/null; then
        count=$((count + 1))
        echo "$count" > "$count_file"
        echo "$$" >> "$pids_file"
        _DS2XBOX_PID="$existing_pid"
        echo "ds2xbox déjà en cours (PID: $_DS2XBOX_PID), instance xbox #$count"
    else
        if [ "$count" -gt 0 ]; then
            echo "ds2xbox (PID précédent mort), nettoyage et relance..."
            echo "0" > "$count_file"
            rm -f "$pids_file" 2>/dev/null
            : > "$pids_file"
            count=0
        fi

        local ds2xbox_args
        ds2xbox_args=$(get_ds2xbox_args)
        echo "Lancement de ds2xbox $ds2xbox_args..."
        "$_DS2XBOX_BIN" $ds2xbox_args &
        _DS2XBOX_PID=$!

        if ! kill -0 "$_DS2XBOX_PID" 2>/dev/null; then
            echo "Erreur: ds2xbox s'est arrêté immédiatement" >&2
            flock -u 201
            exec 201>&-
            return 1
        fi

        echo "$_DS2XBOX_PID" > "$bin_pid_file"
        count=1
        echo "$count" > "$count_file"
        : > "$pids_file"
        echo "$$" >> "$pids_file"
        echo "ds2xbox démarré (PID: $_DS2XBOX_PID)"
    fi

    flock -u 201
    exec 201>&-

    return 0
}

stop_ds2xbox() {
    if [ -n "$_DS2XBOX_PID" ] && kill -0 "$_DS2XBOX_PID" 2>/dev/null; then
        if [ ! -d "$GWINE_LOCK_DIR" ] || [ ! -f "$GWINE_LOCK_DIR/xbox-instance-count" ]; then
            echo "Locks supprimés externement, arrêt direct de ds2xbox (PID: $_DS2XBOX_PID)..."
            kill "$_DS2XBOX_PID" 2>/dev/null
            wait "$_DS2XBOX_PID" 2>/dev/null
            _DS2XBOX_PID=""
            return 0
        fi
    else
        _DS2XBOX_PID=""
        return 0
    fi

    ensure_dir -s "$GWINE_LOCK_DIR"

    local count_file="$GWINE_LOCK_DIR/xbox-instance-count"
    local pids_file="$GWINE_LOCK_DIR/xbox-instance-pids"
    local bin_pid_file="$GWINE_LOCK_DIR/xbox-ds2xbox-pid"
    local sync_lock="$GWINE_LOCK_DIR/.xbox.lock"

    if [ ! -f "$count_file" ]; then
        return 0
    fi

    exec 201>"$sync_lock"
    flock 201

    local count
    count=$(cat "$count_file" 2>/dev/null || echo "0")

    count=$((count - 1))
    if [ "$count" -lt 0 ]; then
        count=0
    fi

    echo "$count" > "$count_file"

    if [ -f "$pids_file" ]; then
        sed -i "/^$$$/d" "$pids_file" 2>/dev/null || true
        if [ ! -s "$pids_file" ]; then
            rm -f "$pids_file" 2>/dev/null || true
        fi
    fi

    local bin_pid=""
    if [ -f "$bin_pid_file" ]; then
        bin_pid=$(cat "$bin_pid_file" 2>/dev/null || echo "")
    fi

    if [ "$count" -eq 0 ]; then
        if [ -n "$bin_pid" ] && kill -0 "$bin_pid" 2>/dev/null; then
            echo "Arrêt de ds2xbox (PID: $bin_pid, dernière instance)..."
            kill "$bin_pid" 2>/dev/null
            wait "$bin_pid" 2>/dev/null
        fi
        rm -f "$count_file" "$pids_file" "$bin_pid_file" 2>/dev/null || true
    else
        echo "ds2xbox laissé actif (encore $count instance(s) xbox)"
    fi

    _DS2XBOX_PID=""

    flock -u 201
    exec 201>&-
}
