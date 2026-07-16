#!/bin/bash

################################################################################
# sandbox.sh - Sandboxing avec bubblewrap
################################################################################

BWRAP_ARGS=()

bwrap_add() {
    BWRAP_ARGS+=("$@")
}

bwrap_reset() {
    BWRAP_ARGS=()
}

build_bwrap_devices() {
    bwrap_add --dev-bind /dev /dev
}

build_bwrap_configs() {
    if [ -n "${XDG_RUNTIME_DIR:-}" ] && [ -d "$XDG_RUNTIME_DIR" ]; then
        bwrap_add --bind-try "$XDG_RUNTIME_DIR" "$XDG_RUNTIME_DIR"
    else
        local user_id
        user_id=$(id -u)
        if [ -d "/run/user/$user_id" ]; then
            bwrap_add --bind-try /run/user/$user_id /run/user/$user_id
        fi
    fi
    
    if [ -d "/run/dbus" ]; then
        bwrap_add --bind /run/dbus /run/dbus
    fi
    
    if [ -n "${MANGOHUD_BIN:-}" ] && [ -f "$MANGOHUD_BIN" ]; then
        bwrap_add --ro-bind-try "$MANGOHUD_BIN" "$MANGOHUD_BIN"
    fi
    
    if [ -d "/usr/lib/mangohud" ]; then
        bwrap_add --ro-bind-try /usr/lib/mangohud /usr/lib/mangohud
    fi
    if [ -d "/usr/lib64/mangohud" ]; then
        bwrap_add --ro-bind-try /usr/lib64/mangohud /usr/lib64/mangohud
    fi
    if [ -d "/usr/share/mangohud" ]; then
        bwrap_add --ro-bind-try /usr/share/mangohud /usr/share/mangohud
    fi
    
    if [ -d "/usr/share/glvnd/egl_vendor.d" ]; then
        bwrap_add --ro-bind-try /usr/share/glvnd/egl_vendor.d /usr/share/glvnd/egl_vendor.d
    fi
    if [ -d "/etc/glvnd/egl_vendor.d" ]; then
        bwrap_add --ro-bind-try /etc/glvnd/egl_vendor.d /etc/glvnd/egl_vendor.d
    fi
    if [ -d "/usr/share/egl/egl_external_platform.d" ]; then
        bwrap_add --ro-bind-try /usr/share/egl/egl_external_platform.d /usr/share/egl/egl_external_platform.d
    fi
    if [ -d "/etc/egl/egl_external_platform.d" ]; then
        bwrap_add --ro-bind-try /etc/egl/egl_external_platform.d /etc/egl/egl_external_platform.d
    fi
    
    if [ -d "/usr/share/nvidia" ]; then
        bwrap_add --ro-bind-try /usr/share/nvidia /usr/share/nvidia
    fi
    
    if [ -d "/usr/share/drirc.d" ]; then
        bwrap_add --ro-bind-try /usr/share/drirc.d /usr/share/drirc.d
    fi
    if [ -f "/etc/drirc" ]; then
        bwrap_add --ro-bind-try /etc/drirc /etc/drirc
    fi
    
    local mangohud_config_dir="$HOME_REAL/.config/MangoHud"
    if [ -d "$mangohud_config_dir" ]; then
        bwrap_add --ro-bind-try "$mangohud_config_dir" "$mangohud_config_dir"
    fi
}

build_bwrap_directories() {
    local exe_dir="${1:-}"
    local access_mode="${2:-rw}"
    
    if [ -d "$SAVES_REAL" ]; then
        bwrap_add --bind "$SAVES_REAL" "$SAVES_REAL"
    fi
    
    if [ -d "$HOME_REAL/Windows" ]; then
        bwrap_add --bind "$HOME_REAL/Windows" "$HOME_REAL/Windows"
        for subdir in SteamData UserData; do
            local subdir_path="$HOME_REAL/Windows/$subdir"
            if [ -L "$subdir_path" ]; then
                local real_path
                real_path=$(readlink -f "$subdir_path")
                if [ -n "$real_path" ] && [ "$real_path" != "$subdir_path" ]; then
                    bwrap_add --bind "$real_path" "$real_path"
                fi
            fi
        done
    fi
    
    if [ "${USE_BIND_MOUNTS:-true}" = "true" ] && [ "${INSTANCE_MODE:-master}" != "joiner" ]; then
        mkdir -p "$WINEPREFIX/drive_c/ProgramData"
        
        for target in "$WINEPREFIX/drive_c/users" "$WINEPREFIX/drive_c/ProgramData/Steam" "$WINEPREFIX/drive_c/Applications" "$WINEPREFIX/drive_c/Games"; do
            if [ -e "$target" ] || [ -L "$target" ]; then
                rm -rf "$target"
            fi
        done
        
        mkdir -p "$WINEPREFIX/drive_c/users" "$WINEPREFIX/drive_c/ProgramData/Steam" "$WINEPREFIX/drive_c/Applications" "$WINEPREFIX/drive_c/Games"
        
        bwrap_add --bind "$HOME_REAL/Windows/UserData" "$WINEPREFIX/drive_c/users"
        bwrap_add --bind "$HOME_REAL/Windows/SteamData" "$WINEPREFIX/drive_c/ProgramData/Steam"
        bwrap_add --bind "$HOME_REAL/Windows/Applications" "$WINEPREFIX/drive_c/Applications"
        bwrap_add --bind "$HOME_REAL/Windows/Games" "$WINEPREFIX/drive_c/Games"
    fi
    
    if [ -d "$CACHE_DIR" ]; then
        bwrap_add --bind "$CACHE_DIR" "$CACHE_DIR"
    fi
    
    if [ -d "$EXTRA_REAL" ]; then
        bwrap_add --bind "$EXTRA_REAL" "$EXTRA_REAL"
    fi
    
    if [ -d "$GWINE_DIR" ]; then
        bwrap_add --bind "$GWINE_DIR" "$GWINE_DIR"
    fi
}

build_bwrap_tmp() {
    local shared_tmp_dir
    if command -v get_shared_tmp_dir &>/dev/null; then
        shared_tmp_dir=$(get_shared_tmp_dir)
    fi
    
    if [ -n "$shared_tmp_dir" ] && [ -d "$shared_tmp_dir" ]; then
        bwrap_add --bind "$shared_tmp_dir" /tmp
        echo "Sandbox: /tmp partagé entre instances" >&2
    else
        bwrap_add --tmpfs /tmp
        echo "Sandbox: /tmp isolé (tmpfs)" >&2
    fi
    
    if [ -n "$MOUNT_BASE" ]; then
        mkdir -p "$MOUNT_BASE"
        bwrap_add --bind "$MOUNT_BASE" "$MOUNT_BASE"
        echo "Sandbox: WGP mounts bindés dans /tmp partagé" >&2
    fi
    
    if [ -d "$GWINE_LOCK_DIR" ]; then
        bwrap_add --bind "$GWINE_LOCK_DIR" "$GWINE_LOCK_DIR"
    fi
    
    if [ -d "$SAVES_SYMLINK" ]; then
        bwrap_add --bind "$SAVES_SYMLINK" "$SAVES_SYMLINK"
    fi
    
    if [ -d "$EXTRA_SYMLINK" ]; then
        bwrap_add --bind "$EXTRA_SYMLINK" "$EXTRA_SYMLINK"
    fi
    
    if [ -d "$TEMP_SYMLINK" ]; then
        bwrap_add --bind "$TEMP_SYMLINK" "$TEMP_SYMLINK"
    fi
    
    if [ -d "$TEMP_REAL" ]; then
        bwrap_add --bind "$TEMP_REAL" "$TEMP_REAL"
    fi
    
    if [ -d "/tmp/wgp-full-overlay" ]; then
        bwrap_add --bind /tmp/wgp-full-overlay /tmp/wgp-full-overlay
    fi
    
    if [ -d "/tmp/wgp-full-overlay-work" ]; then
        bwrap_add --bind /tmp/wgp-full-overlay-work /tmp/wgp-full-overlay-work
    fi
}

build_bwrap_user_dirs() {
    local exe_dir="${1:-}"
    local access_mode="${2:-rw}"
    
    if [ -d "/run/media" ]; then
        bwrap_add --bind /run/media /run/media
    fi
    
    local download_dir
    download_dir=$(xdg-user-dir DOWNLOAD 2>/dev/null)
    if [ -z "$download_dir" ] || [ ! -d "$download_dir" ]; then
        download_dir="$HOME_REAL/Téléchargements"
        [ ! -d "$download_dir" ] && download_dir="$HOME_REAL/Downloads"
    fi
    
    if [ -n "$download_dir" ] && [ -d "$download_dir" ]; then
        bwrap_add --bind "$download_dir" "$download_dir"
    fi
    
    if command -v get_custom_bind_dirs_paths &>/dev/null; then
        local custom_paths
        custom_paths="$(get_custom_bind_dirs_paths)"
        if [ -n "$custom_paths" ]; then
            while IFS= read -r dir_path; do
                if [ -n "$dir_path" ] && [ -d "$dir_path" ]; then
                    bwrap_add --bind "$dir_path" "$dir_path"
                fi
            done <<< "$custom_paths"
            echo "Sandbox: répertoires personnalisés bindés" >&2
        fi
    fi
    
    if [ -n "$exe_dir" ] && [ -d "$exe_dir" ]; then
        local exe_instance_file="$GWINE_LOCK_DIR/active-exe-instance"
        local is_first_exe=false
        if [ ! -f "$exe_instance_file" ] || [ "$(cat "$exe_instance_file" 2>/dev/null)" = "$(realpath "$exe_dir")" ]; then
            is_first_exe=true
        fi
        
        if [ "$is_first_exe" = true ] && [ "$access_mode" = "rw" ]; then
            local parent_dir
            parent_dir="$(dirname "$exe_dir")"
            if [ -d "$parent_dir" ] && [ "$parent_dir" != "/" ]; then
                local already_bound=false
                for arg in "${BWRAP_ARGS[@]}"; do
                    if [[ "$arg" == "$parent_dir" ]]; then
                        already_bound=true
                        break
                    fi
                done
                if [ "$already_bound" = false ]; then
                    bwrap_add --bind "$parent_dir" "$parent_dir"
                    echo "Sandbox: dossier parent bindé: $parent_dir" >&2
                fi
            fi
        fi
        
        if [ "$access_mode" = "ro" ]; then
            bwrap_add --ro-bind "$exe_dir" "$exe_dir"
        else
            bwrap_add --bind "$exe_dir" "$exe_dir"
        fi
    fi
}

build_bwrap_env_vars() {
    bwrap_add --setenv HOME "$HOME"
    bwrap_add --setenv WINEPREFIX "$WINEPREFIX"
    bwrap_add --setenv WINEARCH "$WINEARCH"
    bwrap_add --setenv WINE "$WINE_BIN"
    bwrap_add --setenv PATH "$PATH"
    
    if [ -n "${LD_PRELOAD:-}" ]; then
        bwrap_add --setenv LD_PRELOAD "$LD_PRELOAD"
    else
        bwrap_add --setenv LD_PRELOAD ""
    fi
    
    [ -n "${WINE_TMPDIR:-}" ] && bwrap_add --setenv WINE_TMPDIR "$WINE_TMPDIR"
    [ -n "${TMP:-}" ] && bwrap_add --setenv TMP "$TMP"
    [ -n "${TEMP:-}" ] && bwrap_add --setenv TEMP "$TEMP"
    [ -n "${WINE_LARGE_ADDRESS_AWARE:-}" ] && bwrap_add --setenv WINE_LARGE_ADDRESS_AWARE "$WINE_LARGE_ADDRESS_AWARE"
    [ -n "${STAGING_SHARED_MEMORY:-}" ] && bwrap_add --setenv STAGING_SHARED_MEMORY "$STAGING_SHARED_MEMORY"
    [ -n "${WINEFSYNC:-}" ] && bwrap_add --setenv WINEFSYNC "$WINEFSYNC"
    [ -n "${LOW_LATENCY_LAYER:-}" ] && bwrap_add --setenv LOW_LATENCY_LAYER "$LOW_LATENCY_LAYER"
    [ -n "${LC_ALL:-}" ] && bwrap_add --setenv LC_ALL "$LC_ALL"
    [ -n "${WINEDEBUG:-}" ] && bwrap_add --setenv WINEDEBUG "$WINEDEBUG"
    [ -n "${WINEDLLOVERRIDES:-}" ] && bwrap_add --setenv WINEDLLOVERRIDES "$WINEDLLOVERRIDES"
    [ -n "${LD_LIBRARY_PATH:-}" ] && bwrap_add --setenv LD_LIBRARY_PATH "$LD_LIBRARY_PATH"
    [ -n "${VK_ICD_FILENAMES:-}" ] && bwrap_add --setenv VK_ICD_FILENAMES "$VK_ICD_FILENAMES"
    [ -n "${DXVK_STATE_CACHE_PATH:-}" ] && bwrap_add --setenv DXVK_STATE_CACHE_PATH "$DXVK_STATE_CACHE_PATH"
    [ -n "${DXVK_SHADER_CACHE_PATH:-}" ] && bwrap_add --setenv DXVK_SHADER_CACHE_PATH "$DXVK_SHADER_CACHE_PATH"
    [ -n "${VKD3D_SHADER_CACHE_PATH:-}" ] && bwrap_add --setenv VKD3D_SHADER_CACHE_PATH "$VKD3D_SHADER_CACHE_PATH"
    [ -n "${__GL_SHADER_DISK_CACHE:-}" ] && bwrap_add --setenv __GL_SHADER_DISK_CACHE "$__GL_SHADER_DISK_CACHE"
    [ -n "${__GL_SHADER_DISK_CACHE_SKIP_CLEANUP:-}" ] && bwrap_add --setenv __GL_SHADER_DISK_CACHE_SKIP_CLEANUP "$__GL_SHADER_DISK_CACHE_SKIP_CLEANUP"
    [ -n "${__GL_SHADER_DISK_CACHE_PATH:-}" ] && bwrap_add --setenv __GL_SHADER_DISK_CACHE_PATH "$__GL_SHADER_DISK_CACHE_PATH"
    [ -n "${MESA_SHADER_CACHE_DIR:-}" ] && bwrap_add --setenv MESA_SHADER_CACHE_DIR "$MESA_SHADER_CACHE_DIR"
    [ -n "${MANGOHUD:-}" ] && bwrap_add --setenv MANGOHUD "$MANGOHUD"
    [ -n "${MANGOHUD_DLSYM:-}" ] && bwrap_add --setenv MANGOHUD_DLSYM "$MANGOHUD_DLSYM"
    [ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ] && bwrap_add --setenv DBUS_SESSION_BUS_ADDRESS "$DBUS_SESSION_BUS_ADDRESS"
    
    [ -n "${DXVK_NVAPIHACK:-}" ] && bwrap_add --setenv DXVK_NVAPIHACK "$DXVK_NVAPIHACK"
    [ -n "${DXVK_ENABLE_NVAPI:-}" ] && bwrap_add --setenv DXVK_ENABLE_NVAPI "$DXVK_ENABLE_NVAPI"
    
    # DXVK_ASYNC pour le mode dxvk-async (doit être défini avant via l'environnement ou la fonction)
    local dxvk_async_value
    dxvk_async_value=$(get_dxvk_async_env)
    if [ -n "$dxvk_async_value" ]; then
        bwrap_add --setenv DXVK_ASYNC "$dxvk_async_value"
    elif [ -n "${DXVK_ASYNC:-}" ]; then
        bwrap_add --setenv DXVK_ASYNC "$DXVK_ASYNC"
    fi
    [ -n "${__NV_PRIME_RENDER_OFFLOAD:-}" ] && bwrap_add --setenv __NV_PRIME_RENDER_OFFLOAD "$__NV_PRIME_RENDER_OFFLOAD"
    [ -n "${__VK_LAYER_NV_optimus:-}" ] && bwrap_add --setenv __VK_LAYER_NV_optimus "$__VK_LAYER_NV_optimus"
    [ -n "${__GLX_VENDOR_LIBRARY_NAME:-}" ] && bwrap_add --setenv __GLX_VENDOR_LIBRARY_NAME "$__GLX_VENDOR_LIBRARY_NAME"
    [ -n "${SDL_GAMECONTROLLER_ALLOW_STEAM_VIRTUAL_GAMEPAD:-}" ] && bwrap_add --setenv SDL_GAMECONTROLLER_ALLOW_STEAM_VIRTUAL_GAMEPAD "$SDL_GAMECONTROLLER_ALLOW_STEAM_VIRTUAL_GAMEPAD"
    [ -n "${SDL_GAMECONTROLLER_IGNORE_DEVICES:-}" ] && bwrap_add --setenv SDL_GAMECONTROLLER_IGNORE_DEVICES "$SDL_GAMECONTROLLER_IGNORE_DEVICES"
    [ -n "${WINE_GST_REGISTRY_DIR:-}" ] && bwrap_add --setenv WINE_GST_REGISTRY_DIR "$WINE_GST_REGISTRY_DIR"
    [ -n "${GST_PLUGIN_SYSTEM_PATH_1_0:-}" ] && bwrap_add --setenv GST_PLUGIN_SYSTEM_PATH_1_0 "$GST_PLUGIN_SYSTEM_PATH_1_0"
    [ -n "${GST_PLUGIN_FEATURE_RANK:-}" ] && bwrap_add --setenv GST_PLUGIN_FEATURE_RANK "$GST_PLUGIN_FEATURE_RANK"
    [ -n "${GST_REGISTRY:-}" ] && bwrap_add --setenv GST_REGISTRY "$GST_REGISTRY"
    [ -n "${GST_REGISTRY_1_0:-}" ] && bwrap_add --setenv GST_REGISTRY_1_0 "$GST_REGISTRY_1_0"
    [ -n "${GST_PLUGIN_SCANNER:-}" ] && bwrap_add --setenv GST_PLUGIN_SCANNER "$GST_PLUGIN_SCANNER"
    [ -n "${GST_PLUGIN_SCANNER_1_0:-}" ] && bwrap_add --setenv GST_PLUGIN_SCANNER_1_0 "$GST_PLUGIN_SCANNER_1_0"
    [ -n "${GST_REGISTRY_32:-}" ] && bwrap_add --setenv GST_REGISTRY_32 "$GST_REGISTRY_32"
    [ -n "${GST_PLUGIN_SCANNER_32:-}" ] && bwrap_add --setenv GST_PLUGIN_SCANNER_32 "$GST_PLUGIN_SCANNER_32"

    [ -n "${USER:-}" ] && bwrap_add --setenv USER "$USER"
    [ -n "${LOGNAME:-}" ] && bwrap_add --setenv LOGNAME "$LOGNAME"
    [ -n "${SHELL:-}" ] && bwrap_add --setenv SHELL "$SHELL"
    [ -n "${XDG_SESSION_TYPE:-}" ] && bwrap_add --setenv XDG_SESSION_TYPE "$XDG_SESSION_TYPE"
    [ -n "${XDG_CURRENT_DESKTOP:-}" ] && bwrap_add --setenv XDG_CURRENT_DESKTOP "$XDG_CURRENT_DESKTOP"
    [ -n "${WAYLAND_DISPLAY:-}" ] && bwrap_add --setenv WAYLAND_DISPLAY "$WAYLAND_DISPLAY"
    [ -n "${DISPLAY:-}" ] && bwrap_add --setenv DISPLAY "$DISPLAY"
    [ -n "${XDG_RUNTIME_DIR:-}" ] && bwrap_add --setenv XDG_RUNTIME_DIR "$XDG_RUNTIME_DIR"
    [ -n "${XDG_SESSION_DESKTOP:-}" ] && bwrap_add --setenv XDG_SESSION_DESKTOP "$XDG_SESSION_DESKTOP"
    [ -n "${XDG_CONFIG_DIRS:-}" ] && bwrap_add --setenv XDG_CONFIG_DIRS "$XDG_CONFIG_DIRS"
    [ -n "${XDG_DATA_DIRS:-}" ] && bwrap_add --setenv XDG_DATA_DIRS "$XDG_DATA_DIRS"
    
    if [ -n "${GWINE_CUSTOM_VARS:-}" ]; then
        while IFS= read -r env_line; do
            if [ -n "$env_line" ]; then
                local var_name="${env_line%%=*}"
                local var_value="${env_line#*=}"
                if [ -n "$var_name" ]; then
                    bwrap_add --setenv "$var_name" "$var_value"
                fi
            fi
        done <<< "$GWINE_CUSTOM_VARS"
    fi
}

build_bwrap_args() {
    local exe_dir="${1:-}"
    local access_mode="${2:-rw}"
    
    bwrap_reset
    
    bwrap_add --share-net
    
    bwrap_add --die-with-parent
    bwrap_add --new-session
    bwrap_add --cap-drop ALL
    bwrap_add --cap-add CAP_SYS_PTRACE
    
    bwrap_add --ro-bind / /
    bwrap_add --proc /proc
    bwrap_add --tmpfs /home
    
    bwrap_add --bind-try /etc/resolv.conf /etc/resolv.conf
    bwrap_add --bind-try /etc/hosts /etc/hosts
    bwrap_add --bind-try /etc/ssl /etc/ssl
    bwrap_add --bind-try /etc/ca-certificates /etc/ca-certificates
    bwrap_add --bind-try /etc/pki /etc/pki
    bwrap_add --bind-try /etc/localtime /etc/localtime
    bwrap_add --bind-try /etc/machine-id /etc/machine-id
    bwrap_add --ro-bind-try /etc/ld.so.cache /etc/ld.so.cache
    
    build_bwrap_devices
    build_bwrap_configs
    build_bwrap_directories "$exe_dir" "$access_mode"
    build_bwrap_tmp
    build_bwrap_user_dirs "$exe_dir" "$access_mode"
    build_bwrap_env_vars
}
