#!/bin/bash

################################################################################
# conflict.sh - Vérification des conflits d'instance
################################################################################

check_exe_instance_conflict() {
    local exe_path="$1"
    
    if [ "$wgp_mode" = "true" ] || [ "$nosandbox_mode" = true ]; then
        return 0
    fi
    
    local current_exe_dir
    current_exe_dir="$(dirname "$exe_path")"
    local real_current_exe_dir
    real_current_exe_dir=$(realpath "$current_exe_dir")

    local exe_instance_file="$GWINE_LOCK_DIR/active-exe-instance"

    if [ ! -f "$exe_instance_file" ]; then
        if pgrep wineserver >/dev/null 2>&1; then
            local other_gwine
            other_gwine=$(pgrep "^gwine$" | grep -v "^$$$" | head -1)
            if [ -n "$other_gwine" ]; then
                local other_cwd
                other_cwd=$(readlink -f "/proc/$other_gwine/cwd" 2>/dev/null || echo "")
                if [ -n "$other_cwd" ]; then
                    echo "$other_gwine|$other_cwd" > "$exe_instance_file"
                fi
            fi
        fi
    fi

    if [ ! -f "$exe_instance_file" ]; then
        return 0
    fi

    local existing_entry
    existing_entry=$(cat "$exe_instance_file" 2>/dev/null)

    if [ -z "$existing_entry" ]; then
        return 0
    fi

    local existing_pid=""
    local existing_exe_dir="$existing_entry"
    if [[ "$existing_entry" == *"|"* ]]; then
        existing_pid=$(echo "$existing_entry" | cut -d'|' -f1)
        existing_exe_dir=$(echo "$existing_entry" | cut -d'|' -f2-)
    fi

    local exe_still_running=false

    if [ -n "$existing_pid" ] && kill -0 "$existing_pid" 2>/dev/null; then
        exe_still_running=true
    elif pgrep -f "bwrap.*$existing_exe_dir" >/dev/null 2>&1; then
        exe_still_running=true
    fi

    if [ "$exe_still_running" = false ]; then
        rm -f "$exe_instance_file"
        return 0
    fi

    local download_dir
    download_dir=$(xdg-user-dir DOWNLOAD 2>/dev/null)
    if [ -z "$download_dir" ] || [ ! -d "$download_dir" ]; then
        download_dir="$HOME_REAL/Téléchargements"
        [ ! -d "$download_dir" ] && download_dir="$HOME_REAL/Downloads"
    fi

    local real_download_dir
    real_download_dir=$(realpath "$download_dir")
    local real_existing_dir
    real_existing_dir=$(realpath "$existing_exe_dir")
    
    local in_run_media=false
    if [[ "$real_current_exe_dir" == "/run/media"* ]]; then
        in_run_media=true
    fi

    if [[ "$real_current_exe_dir" != "$real_download_dir"* ]] && [ "$in_run_media" = false ]; then
        local existing_in_run_media=false
        if [[ "$real_existing_dir" == "/run/media"* ]]; then
            existing_in_run_media=true
        fi
        
        if [[ "$real_existing_dir" != "$real_download_dir"* ]] && [ "$existing_in_run_media" = false ]; then
            if [ "$real_current_exe_dir" != "$real_existing_dir" ]; then
                local should_stop=false
                local game_name
                game_name=$(basename "$existing_exe_dir")
                
                local kdialog_msg="Un programme Windows (.exe) est deja en cours.\n\nAttention : Les programmes Windows hors des dossiers autorises ne peuvent pas tourner simultanement. Si vous arretez ce programme, tous les autres programmes Windows en cours seront aussi fermes (ils partagent le meme wineserver).\n\nVoulez-vous arreter TOUS les programmes Windows et lancer celui-ci ?"

                if command -v kdialog &>/dev/null; then
                    if kdialog --title "Conflit d'instance" --yes-label "Oui, tout arreter" --no-label "Non, annuler" --warningyesno "$kdialog_msg"; then
                        should_stop=true
                    fi
                else
                    echo ""
                    echo "Un programme Windows (.exe) est en cours : $game_name"
                    echo "Attention: Les programmes Windows hors dossiers autorises ne peuvent pas tourner simultanement."
                    echo "Arreter ce programme fermera aussi les autres programmes Windows (wineserver partage)."
                    read -p "Voulez-vous arreter tous les programmes Windows et lancer celui-ci ? (o/N): " -r
                    [[ "$REPLY" =~ ^[oOyY]$ ]] && should_stop=true
                fi

                if [ "$should_stop" = true ]; then
                    echo "Arrêt de toutes les instances en cours..."
                    
                    pgrep -f -i "\\.exe|\\.bat" | grep -v "^$$$" | xargs -r kill -9 2>/dev/null || true
                    
                    sleep 2
                    
                    rm -f "$exe_instance_file"
                    rm -f "$GWINE_LOCK_DIR"/master-* 2>/dev/null || true
                    rm -f "$GWINE_LOCK_DIR"/prefix-*-count 2>/dev/null || true
                    rm -f "$GWINE_LOCK_DIR"/prefix-*-pids 2>/dev/null || true
                    
                    if [ -f "$GWINE_LOCK_DIR/xbox-ds2xbox-pid" ]; then
                        local ds2xbox_pid
                        ds2xbox_pid=$(cat "$GWINE_LOCK_DIR/xbox-ds2xbox-pid" 2>/dev/null || echo "")
                        if [ -n "$ds2xbox_pid" ] && kill -0 "$ds2xbox_pid" 2>/dev/null; then
                            echo "0" > "$GWINE_LOCK_DIR/xbox-instance-count"
                            : > "$GWINE_LOCK_DIR/xbox-instance-pids"
                        else
                            rm -f "$GWINE_LOCK_DIR/xbox-instance-count" "$GWINE_LOCK_DIR/xbox-instance-pids" "$GWINE_LOCK_DIR/xbox-ds2xbox-pid" 2>/dev/null || true
                        fi
                    fi
                    
                    echo "Toutes les instances arrêtées"
                else
                    error_exit "Lancement annulé"
                fi
            fi
        fi
    fi
    
    return 0
}
