#!/bin/bash

################################################################################
# master.sh - Gestion de l'instance maître (pour namespaces partagés)
################################################################################

is_master_instance_alive() {
    if pgrep wineserver >/dev/null 2>&1; then
        return 0
    fi
    
    rm -f "$MASTER_INSTANCE_FILE" "$MASTER_PID_FILE" "$MASTER_USERNS_FILE" "$MASTER_PIDNS_FILE" 2>/dev/null
    return 1
}

register_master_instance() {
    local prefix_path="${1:-$WINEPREFIX}"
    
    ensure_dir -s "$GWINE_LOCK_DIR"
    
    if [ -n "$MOUNT_BASE" ]; then
        mkdir -p "$MOUNT_BASE"
    fi
    
    echo "$$" > "$MASTER_PID_FILE"
    echo "$prefix_path" > "$MASTER_INSTANCE_FILE"
    
    if [ -d "/proc/$$/ns" ]; then
        echo "/proc/$$/ns/user" > "$MASTER_USERNS_FILE" 2>/dev/null || true
        echo "/proc/$$/ns/pid" > "$MASTER_PIDNS_FILE" 2>/dev/null || true
    fi
    
    echo "Instance maître enregistrée (PID: $$)"
}

get_master_pid() {
    pgrep wineserver 2>/dev/null | head -1
}

get_master_namespaces() {
    local userns=""
    local pidns=""
    
    if [ -f "$MASTER_USERNS_FILE" ]; then
        userns=$(cat "$MASTER_USERNS_FILE" 2>/dev/null)
    fi
    
    if [ -f "$MASTER_PIDNS_FILE" ]; then
        pidns=$(cat "$MASTER_PIDNS_FILE" 2>/dev/null)
    fi
    
    echo "$userns:$pidns"
}

promote_to_master() {
    local prefix_path="${1:-$WINEPREFIX}"
    
    local pid_file="$GWINE_LOCK_DIR/prefix-$(echo "$prefix_path" | md5sum | cut -d' ' -f1)-pids"
    
    if [ -f "$pid_file" ]; then
        while IFS= read -r pid; do
            if [ -n "$pid" ] && [ "$pid" != "$$" ] && kill -0 "$pid" 2>/dev/null; then
                echo "$pid" > "$MASTER_PID_FILE"
                echo "$prefix_path" > "$MASTER_INSTANCE_FILE"
                
                if [ -d "/proc/$pid/ns" ]; then
                    readlink -f "/proc/$pid/ns/user" > "$MASTER_USERNS_FILE" 2>/dev/null || true
                    readlink -f "/proc/$pid/ns/pid" > "$MASTER_PIDNS_FILE" 2>/dev/null || true
                fi
                
                echo "Nouvelle instance maître promue (PID: $pid)"
                return 0
            fi
        done < "$pid_file"
    fi
    
    return 1
}

get_instance_mode() {
    local prefix_path="${1:-$WINEPREFIX}"
    
    cleanup_stale_wineserver_locks
    
    if is_master_instance_alive; then
        echo "joiner"
    else
        echo "master"
    fi
}
