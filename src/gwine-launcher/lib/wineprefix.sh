#!/bin/bash

################################################################################
# wineprefix.sh - Gestion du préfixe Wine (création, init, DLLs, configuration)
################################################################################

# =============================================================================
# Gestion du préfixe Wine
# =============================================================================

# Sauvegarde le préfixe Wine existant
# Usage: backup_wineprefix [suffix]
# Paramètres:
#   suffix : Suffixe optionnel pour le nom du backup (défaut: timestamp)
# Définit la variable globale PREFIX_BACKUP_PATH avec le chemin du backup
# Retourne 0 si succès ou si pas de préfixe à sauvegarder, 1 si échec
backup_wineprefix() {
    local suffix="${1:-$(date +%Y%m%d_%H%M%S)}"
    local backup_dir="$HOME_REAL/Windows/Prefix.backup.$suffix"
    
    if [ -d "$WINEPREFIX" ]; then
        if mv "$WINEPREFIX" "$backup_dir" 2>/dev/null; then
            export PREFIX_BACKUP_PATH="$backup_dir"
            return 0
        else
            echo "Erreur: Impossible de sauvegarder le préfixe"
            return 1
        fi
    fi
    
    return 0
}

# Restaure le préfixe Wine depuis le backup
# Usage: restore_wineprefix [backup_path]
# Paramètres:
#   backup_path : Chemin du backup (défaut: utilise PREFIX_BACKUP_PATH)
# Retourne 0 si succès ou si pas de backup, 1 si échec
restore_wineprefix() {
    local backup_path="${1:-${PREFIX_BACKUP_PATH:-}}"
    
    if [ -z "$backup_path" ] || [ ! -d "$backup_path" ]; then
        return 0
    fi
    
    echo ""
    echo "⚠️  Restauration de l'ancien préfixe..."
    
    # Supprimer le préfixe incomplet s'il existe
    if [ -d "$WINEPREFIX" ]; then
        rm -rf "$WINEPREFIX" 2>/dev/null || true
    fi
    
    # Restaurer depuis le backup
    if mv -T "$backup_path" "$WINEPREFIX" 2>/dev/null; then
        echo "Ancien préfixe restauré: $WINEPREFIX"
        return 0
    else
        # Essayer la méthode classique si mv -T échoue
        rm -rf "$WINEPREFIX" 2>/dev/null || true
        if mv "$backup_path" "$WINEPREFIX" 2>/dev/null; then
            echo "Ancien préfixe restauré: $WINEPREFIX"
            return 0
        else
            echo "ERREUR: Impossible de restaurer le backup !"
            echo "Backup disponible dans: $backup_path"
            return 1
        fi
    fi
}

# Initialise un préfixe Wine avec wineboot --init
# Désactive mscoree et mshtml pour éviter les fenêtres de dialogue Mono/Gecko
# Crée le symlink drive_c/users -> ~/Windows/UserData AVANT wineboot pour que
# la structure steamuser (Desktop, Documents, Saved Games...) soit créée directement
# dans ~/Windows/UserData au lieu d'être perdue lors du remplacement ultérieur
wineboot_init_prefix() {
    mkdir -p "$WINEPREFIX"
    
    # Créer le symlink users -> ~/Windows/UserData avant wineboot
    # pour que wineboot peuple UserData directement
    local users_link="$WINEPREFIX/drive_c/users"
    local users_target="$HOME_REAL/Windows/UserData"
    mkdir -p "$users_target" "$WINEPREFIX/drive_c"
    if [ ! -L "$users_link" ] || [ "$(readlink "$users_link")" != "$users_target" ]; then
        rm -rf "$users_link" 2>/dev/null || true
        ln -sf "$users_target" "$users_link"
    fi
    
    if ! WINEDLLOVERRIDES="mscoree,mshtml=" WINEPREFIX="$WINEPREFIX" "$WINE_BIN" wineboot --init >/dev/null 2>&1; then
        echo "Erreur: échec de wineboot --init pour $WINEPREFIX"
        return 1
    fi
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "mscoree" /d native,builtin /f >/dev/null 2>&1 || true
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "winemenubuilder.exe" /d "" /f >/dev/null 2>&1 || true
    install_icu68_dlls
}

# Vérifie que Wine est disponible et exécutable
# Usage: require_wine [custom_message]
require_wine() {
    local custom_msg="${1:-}"
    
    if [ ! -x "$WINE_BIN" ]; then
        if [ -n "$custom_msg" ]; then
            error_exit "$custom_msg"
        else
            error_exit "Wine introuvable: $WINE_BIN"
        fi
    fi
}

# Configure la structure des dossiers Windows avec liens symboliques
# Si USE_BIND_MOUNTS est true, ne crée pas de symlinks (bind mounts gérés par sandbox)
setup_windows_directories() {
    local target_prefix="${1:-$WINEPREFIX}"
    
    # En mode joiner, ne pas modifier la structure - le master a déjà configuré les bind mounts
    # et ils sont valides dans le namespace partagé
    if [ "${INSTANCE_MODE:-master}" = "joiner" ]; then
        echo "Mode joiner: utilisation de la configuration existante du master"
        return 0
    fi
    
    echo "Configuration de la structure des dossiers Windows..."
    
    # Créer les dossiers utilisateur dans ~/Windows/
    for dir in UserData SteamData Games Applications; do
        if [ ! -e "$HOME_REAL/Windows/$dir" ] && [ ! -L "$HOME_REAL/Windows/$dir" ]; then
            if ! mkdir -p "$HOME_REAL/Windows/$dir"; then
                echo "Erreur: Impossible de créer le dossier $HOME_REAL/Windows/$dir"
                return 1
            fi
        fi
    done
    
    # Si on utilise les bind mounts (par défaut) ET le sandbox est activé, 
    # on crée quand même les symlinks comme fallback
    # Les bind mounts dans bwrap masqueront les symlinks, mais si on lance
    # sans bwrap (2ème instance avec wineserver existant), les symlinks fonctionneront
    if [ "${USE_BIND_MOUNTS:-true}" = "true" ] && [ "${nosandbox_mode:-false}" != "true" ]; then
        # Toujours créer les symlinks, même avec bind mounts
        # Les bind mounts sont prioritaires dans bwrap, les symlinks servent de fallback
        echo "Configuration des dossiers Windows..."
    fi
    
    # Créer les liens symboliques depuis le préfixe vers ~/Windows/
    
    # users -> ~/Windows/UserData
    local users_link="$target_prefix/drive_c/users"
    local users_target="$HOME_REAL/Windows/UserData"
    if [ -L "$users_link" ] && [ "$(readlink "$users_link")" = "$users_target" ]; then
        : # déjà correct, ne rien faire
    else
        rm -rf "$users_link" 2>/dev/null || true
        if ! ln -sf "$users_target" "$users_link"; then
            echo "Erreur: Échec de création du lien symbolique users"
            return 1
        fi
    fi
    
    # ProgramData/Steam -> ~/Windows/SteamData
    if ! mkdir -p "$target_prefix/drive_c/ProgramData/"; then
        echo "Erreur: Impossible de créer ProgramData"
        return 1
    fi
    if [ -e "$target_prefix/drive_c/ProgramData/Steam" ] || [ -L "$target_prefix/drive_c/ProgramData/Steam" ]; then
        rm -rf "$target_prefix/drive_c/ProgramData/Steam"
    fi
    if ! ln -sf "$HOME_REAL/Windows/SteamData" "$target_prefix/drive_c/ProgramData/Steam"; then
        echo "Erreur: Échec de création du lien symbolique Steam"
        return 1
    fi
    
    # drive_c/Applications -> ~/Windows/Applications
    if [ -e "$target_prefix/drive_c/Applications" ] || [ -L "$target_prefix/drive_c/Applications" ]; then
        rm -rf "$target_prefix/drive_c/Applications"
    fi
    if ! ln -sf "$HOME_REAL/Windows/Applications" "$target_prefix/drive_c/Applications"; then
        echo "Erreur: Échec de création du lien symbolique Applications"
        return 1
    fi
    
    # drive_c/Games -> ~/Windows/Games
    if [ -e "$target_prefix/drive_c/Games" ] || [ -L "$target_prefix/drive_c/Games" ]; then
        rm -rf "$target_prefix/drive_c/Games"
    fi
    if ! ln -sf "$HOME_REAL/Windows/Games" "$target_prefix/drive_c/Games"; then
        echo "Erreur: Échec de création du lien symbolique Games"
        return 1
    fi
    
    echo "Structure des dossiers configurée avec succès"
    return 0
}

# Copie la configuration MangoHud
copy_mangohud_config() {
    local MANGOHUD_CONFIG_DIR="$HOME_REAL/.config/MangoHud"
    local MANGOHUD_CONF_USER="$MANGOHUD_CONFIG_DIR/MangoHud.conf"
    local MANGOHUD_CONF_SOURCE="/usr/share/ublue-os/gablue/MangoHud.conf"
    
    # Créer le dossier de config
    ensure_dir "$MANGOHUD_CONFIG_DIR" "Impossible de créer le dossier $MANGOHUD_CONFIG_DIR"
    
    # Copier la config depuis ublue-os/gablue si elle existe
    if [ -f "$MANGOHUD_CONF_SOURCE" ]; then
        echo "Copie de la configuration MangoHud depuis $MANGOHUD_CONF_SOURCE..."
        if cp -f "$MANGOHUD_CONF_SOURCE" "$MANGOHUD_CONF_USER"; then
            echo "Configuration MangoHud copiée dans $MANGOHUD_CONF_USER"
        else
            echo "Erreur: Échec de la copie de la configuration MangoHud"
            return 1
        fi
    else
        # Créer un config MangoHud par défaut si inexistant
        if [ ! -f "$MANGOHUD_CONF_USER" ]; then
            cat > "$MANGOHUD_CONF_USER" << 'EOF'
# MangoHud configuration
fps_limit=0
vsync=0
gl_vsync=0
legacy_layout=false
gpu_stats
cpu_stats
ram
vram
fps
frame_timing
EOF
            echo "Configuration MangoHud créée dans ~/.config/MangoHud/"
        else
            echo "Configuration MangoHud déjà existante"
        fi
    fi
    
    return 0
}

# Configure le registre Wine pour rediriger TEMP/TMP vers un chemin Windows standard
# Les vieux installateurs ne gèrent pas bien les chemins Z:\ (comme 3DMark2000)
setup_wine_temp_symlinks() {
    local target_prefix="${1:-$WINEPREFIX}"
    local username
    username=$(whoami)
    
    mkdir -p "$CACHE_DIR/temp"
    
    local temp_paths=(
        "$target_prefix/drive_c/users/$username/AppData/Local/Temp"
        "$target_prefix/drive_c/users/$username/Temp"
    )
    
    for temp_path in "${temp_paths[@]}"; do
        if [ -d "$temp_path" ] && [ ! -L "$temp_path" ]; then
            rm -rf "$temp_path"
        fi
        
        if [ ! -L "$temp_path" ]; then
            ln -sf "$CACHE_DIR/temp" "$temp_path"
            echo "Symlink Temp créé: $temp_path -> $CACHE_DIR/temp"
        fi
    done
}

# Fonctions de fix manette (DisableHidraw)
apply_padfix_setting() {
    local SYSTEM_REG="$WINEPREFIX/system.reg"
    
    if [ ! -f "$SYSTEM_REG" ]; then
        echo "Warning: system.reg introuvable, skip du fix manette"
        return 0
    fi
    
    if ! grep -q '\[System\\\\CurrentControlSet\\\\Services\\\\WineBus\]' "$SYSTEM_REG" 2>/dev/null; then
        echo '' >> "$SYSTEM_REG"
        echo '[System\\CurrentControlSet\\Services\\WineBus]' >> "$SYSTEM_REG"
    fi
    
    if [ "$fix_mode" = true ]; then
        if grep -q '"DisableHidraw"' "$SYSTEM_REG" 2>/dev/null; then
            sed -i 's/"DisableHidraw"=dword:00000001/"DisableHidraw"=dword:00000000/' "$SYSTEM_REG"
            echo "Mode fix manette: DisableHidraw=0 (hidraw activé)"
        else
            echo '"DisableHidraw"=dword:00000000' >> "$SYSTEM_REG"
            echo "Mode fix manette: DisableHidraw=0 (hidraw activé)"
        fi
    else
        if grep -q '"DisableHidraw"' "$SYSTEM_REG" 2>/dev/null; then
            sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' "$SYSTEM_REG"
            echo "Mode normal: DisableHidraw=1 (hidraw désactivé par défaut)"
        else
            echo '"DisableHidraw"=dword:00000001' >> "$SYSTEM_REG"
            echo "Mode normal: DisableHidraw=1 (hidraw désactivé par défaut)"
        fi
    fi
}

# Restaurer DisableHidraw après le lancement
restore_padfix_setting() {
    [ "$fix_mode" != true ] && return 0
    
    local SYSTEM_REG="$WINEPREFIX/system.reg"
    
    if [ -f "$SYSTEM_REG" ]; then
        sed -i 's/"DisableHidraw"=dword:00000000/"DisableHidraw"=dword:00000001/' "$SYSTEM_REG"
        echo "Mode fix restauré: DisableHidraw=1"
    fi
}

# Configure les clés de registre pour SDL Input (DInput/XInput mapping)
# Usage: configure_sdl_input_registry
configure_sdl_input_registry() {
    local SYSTEM_REG="$WINEPREFIX/system.reg"
    
    if [ ! -f "$SYSTEM_REG" ]; then
        echo "Warning: system.reg introuvable, skip de la configuration SDL"
        return 0
    fi
    
    # Utiliser wine reg add pour une modification propre (comme Bottles)
    # EnableSDLInput - Active le backend SDL pour les manettes
    if WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\Drivers' /v EnableSDLInput /t REG_DWORD /d 1 /f >/dev/null 2>&1; then
        echo "Configuration SDL: EnableSDLInput=1"
    else
        echo "Warning: Échec de la configuration EnableSDLInput"
    fi
    
    # EnableSDLInputMapping - Active le mapping SDL pour les manettes
    if WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\Drivers' /v EnableSDLInputMapping /t REG_DWORD /d 1 /f >/dev/null 2>&1; then
        echo "Configuration SDL: EnableSDLInputMapping=1"
    else
        echo "Warning: Échec de la configuration EnableSDLInputMapping"
    fi
}

restore_windows_symlinks() {
    local target_prefix="${1:-$WINEPREFIX}"
    
    for target in "$target_prefix/drive_c/users" "$target_prefix/drive_c/ProgramData/Steam" "$target_prefix/drive_c/Applications" "$target_prefix/drive_c/Games"; do
        local link_target=""
        case "$target" in
            "$target_prefix/drive_c/users") link_target="$HOME_REAL/Windows/UserData" ;;
            "$target_prefix/drive_c/ProgramData/Steam") link_target="$HOME_REAL/Windows/SteamData" ;;
            "$target_prefix/drive_c/Applications") link_target="$HOME_REAL/Windows/Applications" ;;
            "$target_prefix/drive_c/Games") link_target="$HOME_REAL/Windows/Games" ;;
        esac
        
        if [ -d "$target" ] && [ ! -L "$target" ]; then
            rm -rf "$target"
            ln -sf "$link_target" "$target"
        fi
    done
}

install_icu68_dlls() {
    local icu_src_64=""
    local icu_src_32=""
    for d in "$WINE_DIR/lib/wine/x86_64-windows" "$WINE_DIR/lib64/wine/x86_64-windows"; do
        [ -d "$d" ] && { icu_src_64="$d"; break; }
    done
    for d in "$WINE_DIR/lib/wine/i386-windows" "$WINE_DIR/lib/i386-windows" "$WINE_DIR/lib64/wine/i386-windows"; do
        [ -d "$d" ] && { icu_src_32="$d"; break; }
    done
    local prefix_sys32="$WINEPREFIX/drive_c/windows/system32"
    local prefix_syswow64="$WINEPREFIX/drive_c/windows/syswow64"

    [ -f "$icu_src_64/icuuc68.dll" ] || return 0

    for dll in icuuc68.dll icuin68.dll icudt68.dll; do
        cp -a "$icu_src_64/$dll" "$prefix_sys32/" 2>/dev/null || true
        cp -a "$icu_src_32/$dll" "$prefix_syswow64/" 2>/dev/null || true
    done
}
