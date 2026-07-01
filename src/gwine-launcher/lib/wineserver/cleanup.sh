#!/bin/bash

################################################################################
# cleanup.sh - Nettoyage des locks orphelins
################################################################################

cleanup_stale_wineserver_locks() {
    [ ! -d "$GWINE_LOCK_DIR" ] && return 0
    
    for lock_file in "$GWINE_LOCK_DIR"/prefix-*-count; do
        [ -f "$lock_file" ] || continue
        
        local prefix_hash
        prefix_hash=$(basename "$lock_file" | sed 's/prefix-\(.*\)-count/\1/')
        local pid_file="$GWINE_LOCK_DIR/prefix-$prefix_hash-pids"
        
        local has_active_process=false
        
        if [ -f "$pid_file" ]; then
            while IFS= read -r pid; do
                if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
                    has_active_process=true
                    break
                fi
            done < "$pid_file"
        fi
        
        if [ "$has_active_process" = false ]; then
            rm -f "$lock_file" 2>/dev/null || true
            rm -f "$pid_file" 2>/dev/null || true
            echo "Nettoyage des locks orphelins pour: $prefix_hash"
        fi
    done
}
