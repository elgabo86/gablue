#!/bin/bash

################################################################################
# setup.sh - Configuration de l'environnement Wine
################################################################################

setup_wine_environment() {
    local exe_path="$1"
    local env_vars="${2:-}"
    
    export WINEPREFIX="$HOME_REAL/Windows/Prefix"
    mkdir -p "$WINEPREFIX"
    
    local user_winedlloverrides=""
    if [ -n "$env_vars" ]; then
        user_winedlloverrides=$(echo "$env_vars" | grep -o 'WINEDLLOVERRIDES="[^"]*"' | sed 's/WINEDLLOVERRIDES="\(.*\)"/\1/' || true)
    fi
    
    if [ -n "$env_vars" ]; then
        echo "Variables d'environnement: $env_vars"
        eval "export $env_vars"
        
        user_winedlloverrides=$(echo "$env_vars" | grep -o 'WINEDLLOVERRIDES="[^"]*"' | sed 's/WINEDLLOVERRIDES="\(.*\)"/\1/' || true)
    fi
    
    if [ "${_NEEDS_WINETRICKS_INIT:-0}" = "1" ]; then
        install_winetricks_components
        install_dxvk_vkd3d
        unset _NEEDS_WINETRICKS_INIT
    fi
    
    if [ "$INSTANCE_MODE" != "joiner" ] || [ "$wgp_mode" = "true" ]; then
        setup_windows_directories "$WINEPREFIX"
    else
        echo "Mode joiner: utilisation de la configuration existante du master"
    fi
    
    echo "Exécutable: $exe_path"
    echo "WINEPREFIX: $WINEPREFIX"
    
    export WINE_LARGE_ADDRESS_AWARE="${WINE_LARGE_ADDRESS_AWARE:-1}"
    export STAGING_SHARED_MEMORY="${STAGING_SHARED_MEMORY:-1}"
    export WINEFSYNC="${WINEFSYNC:-1}"
    export WINENTSYNC="${WINENTSYNC:-1}"
    export LC_ALL="${LC_ALL:-fr_FR}"

    if is_wayland_mode; then
        echo "Mode Wayland activé (désactivation de XWayland)"
        unset DISPLAY
    fi
    
    local winedll_overrides=""
    
    if [ -n "${WINEDLLOVERRIDES:-}" ]; then
        winedll_overrides="${WINEDLLOVERRIDES}"
    fi
    
    if [ -n "$user_winedlloverrides" ]; then
        if [ -n "$winedll_overrides" ]; then
            winedll_overrides="${winedll_overrides};${user_winedlloverrides}"
        else
            winedll_overrides="${user_winedlloverrides}"
        fi
    fi
    
    local default_overrides="winemenubuilder='';winesni='';winmm=n,b;version=n,b"
    if [ -n "$winedll_overrides" ]; then
        winedll_overrides="${winedll_overrides};${default_overrides}"
    else
        winedll_overrides="${default_overrides}"
    fi
    
    export WINEDLLOVERRIDES="$winedll_overrides"
    
    export WINEARCH="win64"
    
    setup_gpu_vulkan
    
    if is_nvidia_gpu; then
        export DXVK_NVAPIHACK=0
        export DXVK_ENABLE_NVAPI=1
        
        export __NV_PRIME_RENDER_OFFLOAD=1
        export __VK_LAYER_NV_optimus=NVIDIA_only
        export __GLX_VENDOR_LIBRARY_NAME=nvidia
        
        export SDL_GAMECONTROLLER_ALLOW_STEAM_VIRTUAL_GAMEPAD=1
        
        local current_runner
        current_runner=$(get_current_runner)
        if [ "$current_runner" = "proton" ]; then
            export GST_PLUGIN_FEATURE_RANK="nvh264dec:0,nvh265dec:0"
        fi
    fi
    
    export GST_PLUGIN_SYSTEM_PATH_1_0="$WINE_DIR/lib64/gstreamer-1.0:$WINE_DIR/lib32/gstreamer-1.0"
    export WINE_GST_REGISTRY_DIR="$CACHE_DIR"
    if [ -f "$WINE_DIR/lib64/libexec/gstreamer-1.0/gst-plugin-scanner" ]; then
        export GST_PLUGIN_SCANNER="$WINE_DIR/lib64/libexec/gstreamer-1.0/gst-plugin-scanner"
        export GST_PLUGIN_SCANNER_1_0="$WINE_DIR/lib64/libexec/gstreamer-1.0/gst-plugin-scanner"
    fi
    if [ -f "$WINE_DIR/lib32/libexec/gstreamer-1.0/gst-plugin-scanner" ]; then
        export GST_PLUGIN_SCANNER_32="$WINE_DIR/lib32/libexec/gstreamer-1.0/gst-plugin-scanner"
    fi
    
    if [ "$xbox_mode" = true ]; then
        export SDL_GAMECONTROLLER_IGNORE_DEVICES="$(get_xbox_sdl_ignore_ids)"
    fi
    
    local WINE_LIBS="$WINE_DIR/lib:$WINE_DIR/lib32:$WINE_DIR/lib64"
    if [ -d "$WINE_DIR/lib/wine/x86_64-unix" ]; then
        WINE_LIBS="$WINE_LIBS:$WINE_DIR/lib/wine/x86_64-unix"
    fi
    if [ -d "$WINE_DIR/lib64/wine/x86_64-unix" ]; then
        WINE_LIBS="$WINE_LIBS:$WINE_DIR/lib64/wine/x86_64-unix"
    fi
    if [ -d "$WINE_DIR/lib/wine/i386-unix" ]; then
        WINE_LIBS="$WINE_LIBS:$WINE_DIR/lib/wine/i386-unix"
    fi
    if [ -d "$WINE_DIR/lib32/wine/i386-unix" ]; then
        WINE_LIBS="$WINE_LIBS:$WINE_DIR/lib32/wine/i386-unix"
    fi
    if [ -d "$WINE_DIR/lib64/gst-libs" ]; then
        WINE_LIBS="$WINE_LIBS:$WINE_DIR/lib64/gst-libs"
    fi
    if [ -d "$WINE_DIR/lib32/gst-libs" ]; then
        WINE_LIBS="$WINE_LIBS:$WINE_DIR/lib32/gst-libs"
    fi
    if [ -d "$WINE_DIR/lib/gst-libs" ]; then
        WINE_LIBS="$WINE_LIBS:$WINE_DIR/lib/gst-libs"
    fi
    if [ -d "$WINE_DIR/libexec/gstreamer-1.0" ]; then
        WINE_LIBS="$WINE_LIBS:$WINE_DIR/libexec/gstreamer-1.0"
    fi
    export LD_LIBRARY_PATH="$WINE_LIBS:$LD_LIBRARY_PATH"
    
    # Overlayfs statfs shim: sur Kinoite, le rootfs (/) est overlayfs read-only
    # avec f_bavail=0. GetDiskFreeSpaceEx(NULL) retourne 0 MB et fait crasher
    # les jeux AGS et d'autres apps qui check le disk space du CWD.
    # Le shim intercepte fstatfs/fstatfs64 et remplace f_bavail=0 par des valeurs factices.
    if [ -z "${GWINE_NO_STATFS_SHIM:-}" ]; then
        local root_bavail
        root_bavail=$(stat -f / --printf="%a" 2>/dev/null || echo "1")
        if [ "$root_bavail" = "0" ]; then
            command -v _gwine_extract_shims &>/dev/null && _gwine_extract_shims
            local shim64="$GWINE_LIB_DIR/lib64/composefs_statfs_shim.so"
            local shim32="$GWINE_LIB_DIR/lib/composefs_statfs_shim.so"
            if [ ! -f "$shim64" ] || [ ! -f "$shim32" ]; then
                local script_lib_dir="${SCRIPT_DIR:-$HOME_REAL/.local/share/gwine}/lib"
                if [ -f "$script_lib_dir/composefs_statfs_shim.so" ]; then
                    mkdir -p "$GWINE_LIB_DIR/lib64" "$GWINE_LIB_DIR/lib"
                    cp "$script_lib_dir/composefs_statfs_shim.so" "$shim64"
                    cp "$script_lib_dir/composefs_statfs_shim32.so" "$shim32"
                fi
            fi
            if [ -f "$shim64" ] && [ -f "$shim32" ]; then
                export LD_PRELOAD="$GWINE_LIB_DIR/\$LIB/composefs_statfs_shim.so${LD_PRELOAD:+:$LD_PRELOAD}"
                echo "Overlayfs statfs shim active (rootfs f_bavail=0)"
            fi
        fi
    fi
    
    # Configuration du cache DXVK - utiliser le chemin standard avec symlink
    # Le dossier AppData/Local/dxvk est un symlink vers ~/.cache/gwine/shader-cache/dxvk
    export DXVK_STATE_CACHE_PATH="$SHADER_CACHE_DIR/dxvk"
    export DXVK_SHADER_CACHE_PATH="$SHADER_CACHE_DIR/dxvk"
    export VKD3D_SHADER_CACHE_PATH="$SHADER_CACHE_DIR/vkd3d"
    export __GL_SHADER_DISK_CACHE=1
    export __GL_SHADER_DISK_CACHE_SKIP_CLEANUP=1
    export __GL_SHADER_DISK_CACHE_PATH="$SHADER_CACHE_DIR/nvidia"
    export MESA_SHADER_CACHE_DIR="$SHADER_CACHE_DIR/mesa"
    
    ensure_dirs -s "$CACHE_DIR" "$COMPONENTS_DIR" "$SHADER_CACHE_DIR"
    ensure_dirs -s "$DXVK_STATE_CACHE_PATH" "$VKD3D_SHADER_CACHE_PATH" "$__GL_SHADER_DISK_CACHE_PATH" "$MESA_SHADER_CACHE_DIR"
    
    if [ "${MANGOHUD:-1}" != "0" ]; then
        export MANGOHUD_BIN=$(command -v mangohud 2>/dev/null)
    else
        export MANGOHUD_BIN=""
    fi
}
