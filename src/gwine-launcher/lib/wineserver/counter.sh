#!/bin/bash

################################################################################
# counter.sh - Gestion du compteur d'instances
################################################################################

register_wine_instance() {
    local prefix_path="${1:-$WINEPREFIX}"
    local prefix_hash
    prefix_hash=$(echo "$prefix_path" | md5sum | cut -d' ' -f1)
    local lock_file="$GWINE_LOCK_DIR/prefix-$prefix_hash-count"
    local pid_file="$GWINE_LOCK_DIR/prefix-$prefix_hash-pids"
    
    ensure_dir -s "$GWINE_LOCK_DIR"
    
    local sync_lock="$GWINE_LOCK_DIR/.sync.lock"
    
    exec 200>"$sync_lock"
    flock 200
    
    local count=0
    if [ -f "$lock_file" ]; then
        count=$(cat "$lock_file" 2>/dev/null || echo "0")
    fi
    
    count=$((count + 1))
    echo "$count" > "$lock_file"
    
    echo "$$" >> "$pid_file"
    
    flock -u 200
    exec 200>&-
    
    echo "$count"
}

unregister_wine_instance() {
    local prefix_path="${1:-$WINEPREFIX}"
    local prefix_hash
    prefix_hash=$(echo "$prefix_path" | md5sum | cut -d' ' -f1)
    local lock_file="$GWINE_LOCK_DIR/prefix-$prefix_hash-count"
    local pid_file="$GWINE_LOCK_DIR/prefix-$prefix_hash-pids"
    
    if [ ! -f "$lock_file" ]; then
        echo "0"
        return 0
    fi
    
    local sync_lock="$GWINE_LOCK_DIR/.sync.lock"
    
    exec 200>"$sync_lock"
    flock 200
    
    local count
    count=$(cat "$lock_file" 2>/dev/null || echo "0")
    
    count=$((count - 1))
    if [ $count -lt 0 ]; then
        count=0
    fi
    
    echo "$count" > "$lock_file"
    
    if [ -f "$pid_file" ]; then
        sed -i "/^$$$/d" "$pid_file" 2>/dev/null || true
        if [ ! -s "$pid_file" ]; then
            rm -f "$pid_file" 2>/dev/null || true
        fi
    fi
    
    flock -u 200
    exec 200>&-
    
    if [ $count -eq 0 ]; then
        rm -f "$lock_file" 2>/dev/null || true
        rm -f "$pid_file" 2>/dev/null || true
    fi
    
    echo "$count"
}

is_last_wine_instance() {
    local prefix_path="${1:-$WINEPREFIX}"
    local prefix_hash
    prefix_hash=$(echo "$prefix_path" | md5sum | cut -d' ' -f1)
    local lock_file="$GWINE_LOCK_DIR/prefix-$prefix_hash-count"
    
    if [ ! -f "$lock_file" ]; then
        return 0
    fi
    
    local count
    count=$(cat "$lock_file" 2>/dev/null || echo "0")
    
    if [ "$count" -eq 0 ]; then
        return 0
    else
        return 1
    fi
}
