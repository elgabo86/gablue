#!/bin/bash

################################################################################
# server.sh - Gestion du wineserver
################################################################################

wait_for_wineserver() {
    local prefix_path="${1:-$WINEPREFIX}"
    
    local wine_procs
    wine_procs=$(pgrep "^wine" 2>/dev/null | wc -l)
    
    if [ "$wine_procs" -eq 0 ]; then
        echo "Aucun processus wine actif, wineserver se terminera automatiquement"
        return 0
    fi
    
    local system_procs=0
    for pid in $(pgrep "^wine" 2>/dev/null); do
        if [ -r "/proc/$pid/cmdline" ]; then
            local proc_cmdline
            proc_cmdline=$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null || echo "")
            case "$proc_cmdline" in
                *winedevice.exe*|*plugplay.exe*|*explorer.exe*|*services.exe*|*svchost.exe*|*rpcss.exe*)
                    system_procs=$((system_procs + 1))
                    ;;
            esac
        fi
    done
    
    if [ "$system_procs" -eq "$wine_procs" ]; then
        echo "Seuls des processus système Wine actifs, arrêt du wineserver"
        if command -v wineserver &>/dev/null; then
            WINEPREFIX="$prefix_path" wineserver -k 2>/dev/null || true
        fi
        return 0
    fi
    
    echo "Attente de wineserver pour: $prefix_path ($wine_procs processus wine actifs)"
    
    if command -v wineserver &>/dev/null; then
        if timeout 30 sh -c "WINEPREFIX=\"$prefix_path\" wineserver -w" 2>/dev/null; then
            echo "Wineserver terminé"
        else
            echo "Wineserver: timeout, arrêt forcé"
            WINEPREFIX="$prefix_path" wineserver -k 2>/dev/null || true
        fi
    fi
}

is_wineserver_running() {
    local prefix_path="${1:-$WINEPREFIX}"
    
    if pgrep -f "wineserver.*$prefix_path" >/dev/null 2>&1; then
        return 0
    fi
    
    local socket="$prefix_path/.wine-server"
    if [ -e "$socket" ]; then
        if pgrep wineserver >/dev/null 2>&1; then
            return 0
        fi
    fi
    
    return 1
}

kill_wineserver() {
    local prefix_path="${1:-$WINEPREFIX}"
    
    echo "Arrêt de wineserver pour: $prefix_path"
    
    if command -v wineserver &>/dev/null; then
        WINEPREFIX="$prefix_path" wineserver -k 2>/dev/null || true
    fi
    
    stop_ds2xbox
    
    rm -f "$GWINE_LOCK_DIR"/prefix-* 2>/dev/null || true
    rm -f "$GWINE_LOCK_DIR"/master-* 2>/dev/null || true
    rm -f "$GWINE_LOCK_DIR"/wineprefix-* 2>/dev/null || true
    rm -f "$GWINE_LOCK_DIR"/active-exe-instance 2>/dev/null || true
}

wine_instance_start() {
    local prefix_path="${1:-$WINEPREFIX}"
    init_wineserver_manager
    register_wine_instance "$prefix_path"
}

wine_instance_end() {
    local prefix_path="${1:-$WINEPREFIX}"
    
    local count
    count=$(unregister_wine_instance "$prefix_path")
    
    if [ "$count" -eq 0 ]; then
        wait_for_wineserver "$prefix_path"
        return 0
    else
        echo "Il reste $count instance(s) active(s), wineserver continue"
        return 1
    fi
}
