#!/bin/bash

################################################################################
# launcher-main.sh - Fonction principale de lancement
################################################################################

launch_wine_game() {
    local exe_path="$1"
    local game_args="${2:-}"
    local display_name="${3:-$(basename "$exe_path")}"
    local env_vars="${4:-}"
    local wgp_mode="${5:-false}"
    local wgp_file_dir="${6:-}"
    local wgp_file="${7:-}"
    
    echo "Lancement de $display_name avec gwine..."
    
    if [ "${USE_BIND_MOUNTS:-true}" = "false" ]; then
        echo "Mode liens symboliques activé (fallback)"
    fi
    
    apply_padfix_setting
    
    require_wine
    
    check_exe_instance_conflict "$exe_path"
    
    INSTANCE_MODE=$(get_instance_mode "$WINEPREFIX")
    
    if [ "$INSTANCE_MODE" = "joiner" ]; then
        echo "Mode joiner détecté - Connexion à l'instance maître existante"
    else
        echo "Mode master - Création d'une nouvelle instance wineserver"
    fi
    
    if [ "$wgp_mode" != "true" ] && [ "$nosandbox_mode" != true ]; then
        local current_exe_dir
        current_exe_dir="$(dirname "$exe_path")"
        local real_current_exe_dir
        real_current_exe_dir=$(realpath "$current_exe_dir")

        local exe_instance_file="$GWINE_LOCK_DIR/active-exe-instance"

        local should_update_file=false
        
        if [ "$INSTANCE_MODE" = "master" ]; then
            should_update_file=true
        else
            should_update_file=false
        fi
        
        if [ "$should_update_file" = true ]; then
            echo "$$|$real_current_exe_dir" > "$exe_instance_file"
        fi
    fi
    
    wine_instance_start "$WINEPREFIX"
    echo "Instance enregistrée dans le pool de wineserver"
    
    if [ "$nosandbox_mode" != true ]; then
        if command -v get_wine_socket_dir &>/dev/null; then
            local socket_dir=$(get_wine_socket_dir)
            export WINE_TMPDIR="$socket_dir"
            echo "Socket wineserver partagé: $WINE_TMPDIR"
        fi
    fi
    
    setup_wine_environment "$exe_path" "$env_vars"
    
    if [ -n "$wgp_file" ]; then
        GWINE_WGP_FILE="$wgp_file"
        export GWINE_WGP_FILE
    fi
    
    echo "Arguments: $game_args"
    
    gamemode_start
    
    local exe_dir
    exe_dir="$(dirname "$exe_path")"
    echo "Working directory: $exe_dir"
    
    local wineserver_already_running=false
    if [ -n "${WINEPREFIX:-}" ]; then
        if pgrep wineserver >/dev/null 2>&1; then
            wineserver_already_running=true
        fi
    fi
    
    local bwrap_exe_dir="$exe_dir"
    
    if [ "$nosandbox_mode" != true ]; then
        if command -v bwrap &>/dev/null; then
            echo "Sandbox activé (bwrap)"
            local bwrap_access_dir=""
            local bwrap_access_mode="rw"
            
            if [ "$wgp_mode" = "true" ] && [ -n "$wgp_file_dir" ]; then
                bwrap_access_dir="$wgp_file_dir"
                bwrap_access_mode="ro"
            else
                bwrap_access_dir="$(dirname "$exe_path")"
                bwrap_access_mode="rw"
            fi
            
            if [[ "$exe_dir" == "$SHARED_TMP_DIR"* ]]; then
                exe_dir="/tmp${exe_dir#$SHARED_TMP_DIR}"
            fi
            
            register_master_instance "$WINEPREFIX"
            
            if [ "$wineserver_already_running" = true ]; then
                echo "Note: wineserver existant - utilisation des symlinks (pas de bind mounts)"
            fi
        else
            echo "Warning: bwrap (bubblewrap) non disponible, sandbox désactivé"
            cd "$exe_dir" || error_exit "Impossible de changer le répertoire vers $exe_dir"
        fi
    else
        echo "Sandbox désactivé (--nosandbox)"
        cd "$exe_dir" || error_exit "Impossible de changer le répertoire vers $exe_dir"
    fi
    
    if [ -n "${GWINE_CUSTOM_VARS:-}" ]; then
        while IFS= read -r env_line; do
            if [ -n "$env_line" ]; then
                local var_name="${env_line%%=*}"
                local var_value="${env_line#*=}"
                if [ -n "$var_name" ]; then
                    export "$var_name"="$var_value"
                    echo "Variable exportée: $var_name=$var_value"
                fi
            fi
        done <<< "$GWINE_CUSTOM_VARS"
    fi
    
    export WINEPREFIX
    
    local use_mangohud=false
    if [ "${MANGOHUD:-}" != "0" ] && [ -n "${MANGOHUD_BIN:-}" ]; then
        use_mangohud=true
        echo "MangoHud activé"
    elif [ "${MANGOHUD:-}" = "0" ]; then
        echo "MangoHud désactivé par MANGOHUD=0"
        unset MANGOHUD
        unset MANGOHUD_DLSYM
        unset MANGOHUD_BIN
    fi
    
    local bwrap_pid=""
    local wine_pid=""
    
    if [ "$nosandbox_mode" = true ]; then
        if [ "${_GWINE_USING_KERNEL_OVERLAY:-false}" = "true" ]; then
            # Kernel overlay sans sandbox : wrapper wine dans unshare
            local kernel_cmd
            kernel_cmd="$(_gwine_kernel_overlay_mount_script)"
            if [ "$use_mangohud" = true ]; then
                export MANGOHUD=1
                kernel_cmd+="exec env MANGOHUD=1 \"$MANGOHUD_BIN\" \"$WINE_BIN\" \"$exe_path\" $game_args"
            else
                kernel_cmd+="exec \"$WINE_BIN\" \"$exe_path\" $game_args"
            fi
            echo "Lancement avec kernel overlayfs (unshare, sans sandbox)..."
            unshare -U -m --map-root-user bash -c "$kernel_cmd" </dev/null &
            wine_pid=$!
            GWINE_GAME_PID=$wine_pid
            export GWINE_GAME_PID
        elif [ "$use_mangohud" = true ]; then
            export MANGOHUD=1
            "$MANGOHUD_BIN" "$WINE_BIN" "$exe_path" $game_args </dev/null &
            wine_pid=$!
            GWINE_GAME_PID=$wine_pid
            export GWINE_GAME_PID
        else
            "$WINE_BIN" "$exe_path" $game_args </dev/null &
            wine_pid=$!
            GWINE_GAME_PID=$wine_pid
            export GWINE_GAME_PID
        fi
    else
        build_bwrap_args "$bwrap_access_dir" "$bwrap_access_mode"
        
        bwrap_add --chdir "$bwrap_exe_dir"
        bwrap_add --
        
        if [ "$use_mangohud" = true ]; then
            bwrap_add "$MANGOHUD_BIN"
        fi
        bwrap_add "$WINE_BIN"
        bwrap_add "$exe_path"
        if [ -n "$game_args" ]; then
            bwrap_add $game_args
        fi
        
        if [ "${_GWINE_USING_KERNEL_OVERLAY:-false}" = "true" ]; then
            # En mode kernel, les overlays sont montés dans le même unshare que bwrap
            local kernel_cmd
            kernel_cmd="$(_gwine_kernel_overlay_mount_script)"
            kernel_cmd+="exec bwrap"
            local arg
            for arg in "${BWRAP_ARGS[@]}"; do
                kernel_cmd+=" $(printf '%q' "$arg")"
            done
            echo "Lancement avec kernel overlayfs (unshare + bwrap)..."
            unshare -U -m --map-root-user bash -c "$kernel_cmd" </dev/null &
            bwrap_pid=$!
            GWINE_GAME_PID=$bwrap_pid
            export GWINE_GAME_PID
        else
            bwrap "${BWRAP_ARGS[@]}" </dev/null &
            bwrap_pid=$!
            GWINE_GAME_PID=$bwrap_pid
            export GWINE_GAME_PID
        fi
        
        # Attendre que le jeu démarre
        sleep 2
        
        # Chercher récursivement tous les processus descendants de bwrap
        find_descendants() {
            local parent=$1
            local descendants=""
            for child in $(pgrep -P "$parent" 2>/dev/null); do
                descendants="$descendants $child"
                descendants="$descendants $(find_descendants "$child")"
            done
            echo "$descendants"
        }
        
        # Afficher l'arbre des processus pour debug
        echo "Arbre des processus:"
        for pid in $(pgrep -P "$bwrap_pid" 2>/dev/null); do
            echo "  bwrap child: $pid"
            for child in $(pgrep -P "$pid" 2>/dev/null); do
                if [ -r "/proc/$child/cmdline" ]; then
                    local cmd
                    cmd=$(tr '\0' ' ' < "/proc/$child/cmdline" 2>/dev/null | head -c 100)
                    echo "    -> $child: $cmd"
                fi
            done
        done
        
        # Essayer de trouver le processus exe
        for pid in $(pgrep -P "$bwrap_pid" 2>/dev/null); do
            for child in $(pgrep -P "$pid" 2>/dev/null); do
                if [ -r "/proc/$child/cmdline" ]; then
                    local cmdline
                    cmdline=$(tr '\0' ' ' < "/proc/$child/cmdline" 2>/dev/null)
                    if [[ "$cmdline" == *.exe* ]]; then
                        wine_pid="$child"
                        break 2
                    fi
                fi
                # Chercher un niveau plus profond
                for grandchild in $(pgrep -P "$child" 2>/dev/null); do
                    if [ -r "/proc/$grandchild/cmdline" ]; then
                        local gcmdline
                        gcmdline=$(tr '\0' ' ' < "/proc/$grandchild/cmdline" 2>/dev/null)
                        if [[ "$gcmdline" == *.exe* ]]; then
                            wine_pid="$grandchild"
                            break 3
                        fi
                    fi
                done
            done
        done
    fi
    
    if [ -n "$wine_pid" ]; then
        echo "PID processus jeu: $wine_pid"
    else
        echo "PID processus jeu: détection automatique (plusieurs processus .exe possibles)"
    fi
    
    echo "Attente de la fin du jeu..."
    local wait_count=0
    local still_running=true
    local game_exe_name
    game_exe_name=$(basename "$exe_path" .exe)
    
    while [ "$still_running" = true ]; do
        local has_wine_process=false
        local has_game_process=false
        
        # Chercher tous les processus avec ce WINEPREFIX
        for pid in $(ls /proc 2>/dev/null | grep -E '^[0-9]+$'); do
            if [ -r "/proc/$pid/environ" ]; then
                if grep -q "WINEPREFIX=$WINEPREFIX" "/proc/$pid/environ" 2>/dev/null; then
                    if [ -r "/proc/$pid/cmdline" ]; then
                        local cmdline
                        cmdline=$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null)
                        # Ignorer les processus système wine
                        if [[ "$cmdline" == *.exe* ]]; then
                            has_wine_process=true
                            if ! [[ "$cmdline" =~ (winedevice|plugplay|explorer|services|svchost|rpcss|wineboot|steam|msiexec|regsvr32|spoolsv)\.exe ]] && \
                               ! [[ "$cmdline" =~ (conhost|wmplayer|taskkill|wineconsole|iexplore|msedge|chrome)\.exe ]] && \
                               ! [[ "$cmdline" == *\.exe.* ]]; then
                                has_game_process=true
                            fi
                        fi
                    fi
                fi
            fi
        done
        
        if [ "$has_game_process" = true ]; then
            wait_count=$((wait_count + 1))
            if [ $((wait_count % 30)) -eq 0 ]; then
                echo "Jeu toujours en cours..."
            fi
            sleep 1
        else
            still_running=false
            echo "Jeu terminé"
        fi
    done
    
    # Arrêter bwrap si encore actif
    if [ "$nosandbox_mode" != true ] && [ -n "$bwrap_pid" ]; then
        if kill -0 "$bwrap_pid" 2>/dev/null; then
            echo "Arrêt du sandbox..."
            kill -TERM "$bwrap_pid" 2>/dev/null || true
            sleep 1
            kill -KILL "$bwrap_pid" 2>/dev/null || true
            wait "$bwrap_pid" 2>/dev/null || true
        fi
    fi
    
    local exit_code=0
    
    restore_padfix_setting
    
    gamemode_stop
    
    local count
    count=$(unregister_wine_instance "$WINEPREFIX")
    
    if [ "$count" -eq 0 ]; then
        wait_for_wineserver "$WINEPREFIX"
        echo "Dernière instance terminée, wineserver arrêté"
    else
        echo "Instance terminée, il reste $count instance(s), wineserver continue"
    fi
    
    if [ "$count" -eq 0 ]; then
        local exe_instance_file="$GWINE_LOCK_DIR/active-exe-instance"
        rm -f "$exe_instance_file"
        restore_windows_symlinks "$WINEPREFIX"
    fi
    
    _cleanup_kernel_overlay_markers 2>/dev/null || true
    
    return $exit_code
}
