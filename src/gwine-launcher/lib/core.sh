#!/bin/bash

################################################################################
# core.sh - Variables globales et fonctions utilitaires de base
################################################################################

# Variables globales
GWINE_DIR="$HOME/.local/share/gwine"
GWINE_LIB_DIR="$GWINE_DIR"
HOME_REAL="$(realpath "$HOME")"
WINDOWS_HOME="$HOME_REAL/Windows/UserData"
# Répertoire /tmp partagé pour le multi-instance (doit être défini avant les symlinks)
GWINE_LOCK_DIR="/tmp/gwine-locks-$USER"
SHARED_TMP_DIR="$GWINE_LOCK_DIR/tmp"

# Chemins des symlinks dans le /tmp partagé (visibles par toutes les instances sandbox/non-sandbox)
SAVES_SYMLINK="$SHARED_TMP_DIR/wgp-saves"
EXTRA_SYMLINK="$SHARED_TMP_DIR/wgp-extra"
TEMP_SYMLINK="$SHARED_TMP_DIR/wgp-temp"

SAVES_REAL="$WINDOWS_HOME/$USER/LocalSavesWGP"
EXTRA_REAL="$HOME/.cache/wgp-extra"
TEMP_REAL="/tmp/wgp-temp"

CACHE_DIR="$HOME/.cache/gwine"
COMPONENTS_DIR="$CACHE_DIR/components"
SHADER_CACHE_DIR="$CACHE_DIR/shader-cache"
DXVK_CACHE_DIR="$COMPONENTS_DIR/dxvk"
DXVK_ASYNC_CACHE_DIR="$COMPONENTS_DIR/dxvk-gplasync"
VKD3D_CACHE_DIR="$COMPONENTS_DIR/vkd3d"
DXVK_NVAPI_CACHE_DIR="$COMPONENTS_DIR/dxvk-nvapi"

# Les liens symboliques dans /tmp/ sont créés par init_wineserver_manager()
# car ensure_dir() n'est pas encore défini à ce point

WINE_DIR="$GWINE_DIR/wine"
WINE_BIN="$WINE_DIR/bin/wine"
WINE32_BIN="$WINE_DIR/bin/wine"
WINESERVER_BIN="$WINE_DIR/bin/wineserver"

# Variables de mode
fix_mode=false
xbox_mode=false
xbox_filter=""
xbox_on_mode=false
xbox_off_mode=false
nofix_mode=false
reset_mode=false
exewgp_mode=false
init_mode=false
update_mode=false
cmd_mode=false
cmd_command=""
regedit_mode=false
reg_mode=false
reg_args=()
winecfg_mode=false
winetricks_mode=false
download_wincomponents_mode=false
cachepack_mode=false
nosandbox_mode=false
joytest_mode=false
winetricks_args=""
args=""
fullpath=""
OFFLINE_MODE=false

# Variables WGP
GAME_INTERNAL_NAME=""
MOUNT_DIR=""

# Variable globale pour stocker la référence dbus du progress dialog
_PROGRESS_DBUS_REF=""

# Variable globale pour stocker le PID du processus gamemode
_GWINE_GAMEMODE_PID=""

# Variable globale pour contrôler l'utilisation de kdialog
_USE_KDIALOG=false

# Par défaut, utiliser des bind mounts (meilleure compatibilité)
# Pour utiliser les liens symboliques classiques, utiliser --use-ln-mounts
USE_BIND_MOUNTS=true

# Fonctions utilitaires de base

error_exit() {
    echo "Erreur: $1" >&2
    exit 1
}

get_system_tool() {
    local tool="$1"
    local tool_path
    tool_path=$(command -v "$tool" 2>/dev/null)
    if [ -z "$tool_path" ]; then
        error_exit "$tool n'est pas installé sur le système"
    fi
    echo "$tool_path"
}

# Crée un répertoire s'il n'existe pas
# Usage: ensure_dir [-s] <directory> [error_message]
# Options:
#   -s : Mode silencieux (ne retourne pas d'erreur si échec)
# Retourne 0 si succès ou si le répertoire existe déjà, 1 si échec (sauf mode silencieux)
ensure_dir() {
    local silent=false
    local dir=""
    local error_msg=""
    
    # Parser les options en premier
    while [ $# -gt 0 ]; do
        case "$1" in
            -s) 
                silent=true
                shift
                ;;
            *)
                break
                ;;
        esac
    done
    
    # Récupérer les arguments positionnels
    dir="${1:-}"
    error_msg="${2:-}"
    
    if [ -z "$dir" ]; then
        [ "$silent" = false ] && echo "Erreur: Aucun répertoire spécifié" >&2
        return 1
    fi
    
    if [ -d "$dir" ]; then
        return 0
    fi
    
    if ! mkdir -p "$dir" 2>/dev/null; then
        if [ "$silent" = false ]; then
            if [ -n "$error_msg" ]; then
                error_exit "$error_msg"
            else
                error_exit "Impossible de créer le répertoire: $dir"
            fi
        fi
        return 1
    fi
    
    return 0
}

# Crée plusieurs répertoires en une seule fois
# Usage: ensure_dirs [-s] <dir1> [dir2] [dir3] ...
# Option -s : Mode silencieux
ensure_dirs() {
    local silent=""
    
    if [ "$1" = "-s" ]; then
        silent="-s"
        shift
    fi
    
    for dir in "$@"; do
        ensure_dir $silent "$dir"
    done
}

# Vérifie si une version est plus récente qu'une autre
# Compare simplement les versions - retourne 0 si v1 > v2
compare_versions() {
    local v1="$1"  # version GitHub
    local v2="$2"  # version locale
    
    # Si pas de version locale, v1 est plus récente
    [ -z "$v2" ] && return 0
    
    # Si versions identiques, pas de mise à jour
    [ "$v1" = "$v2" ] && return 1
    
    # Utiliser sort -V pour comparer
    local newer
    newer=$(printf '%s\n%s\n' "$v1" "$v2" | sort -V | tail -n1)
    [ "$newer" = "$v1" ] && return 0
    return 1
}

# =============================================================================
# Gestion des overlays — kernel overlayfs natif via unshare (user namespaces)
# =============================================================================
# Usage:
#   mount_overlay <lowerdir> <upperdir> <workdir> <mountpoint>
#   unmount_overlay <mount_point> [-f] [-l]
#
# Les overlays sont montés dans le même unshare que bwrap (montage différé).
# Les paramètres sont stockés dans _GWINE_KERNEL_OVERLAY_MOUNTS et le montage
# effectif est fait par _gwine_kernel_overlay_mount_script() au moment du lancement.

# Variable globale : liste des overlays kernel à monter (une ligne par overlay)
# Format: lowerdir|upperdir|workdir|mountpoint
_GWINE_KERNEL_OVERLAY_MOUNTS=""

# Variable globale : indique si on utilise le kernel overlay (positionné par mount_overlay)
_GWINE_USING_KERNEL_OVERLAY=false

# Vérifie que le kernel overlayfs via user namespaces est disponible, sinon erreur
_require_kernel_overlay() {
    if ! unshare -U -m --map-root-user true 2>/dev/null; then
        error_exit "Kernel overlayfs indisponible : user namespaces non supportés"
    fi
    if [ ! -f /proc/filesystems ] || ! grep -qw overlay /proc/filesystems 2>/dev/null; then
        error_exit "Kernel overlayfs indisponible : module overlay non chargé"
    fi
}

# Génère un identifiant court pour un mountpoint (pour le fichier de suivi)
_kernel_overlay_id() {
    local mp="$1"
    echo "$mp" | md5sum 2>/dev/null | cut -c1-8 || echo "$mp" | cksum 2>/dev/null | cut -d' ' -f1
}

# Enregistre un overlay kernel (montage différé dans le unshare de bwrap)
# Usage: _register_kernel_overlay <lowerdir> <upperdir> <workdir> <mountpoint>
_register_kernel_overlay() {
    local lower="$1"
    local upper="$2"
    local work="$3"
    local mountpoint="$4"
    
    # Enregistrer les paramètres
    _GWINE_KERNEL_OVERLAY_MOUNTS+="${lower}|${upper}|${work}|${mountpoint}"$'\n'
    _GWINE_USING_KERNEL_OVERLAY=true
    
    # Marqueur pour le suivi
    local pid_file="/tmp/gwine-ko-$( _kernel_overlay_id "$mountpoint" ).pid"
    echo "deferred" > "$pid_file"
    
    echo "Overlay kernel enregistré (montage différé): $mountpoint"
    return 0
}

# Génère les commandes bash pour monter tous les overlays kernel enregistrés
# Usage: _gwine_kernel_overlay_mount_script
# Retourne un script bash à exécuter dans le unshare
_gwine_kernel_overlay_mount_script() {
    local script=""
    while IFS='|' read -r lower upper work mountpoint; do
        [ -z "$lower" ] && continue
        script+="mkdir -p \"$(dirname "$mountpoint")\" 2>/dev/null; "
        script+="mount -t overlay overlay -o lowerdir=\"$lower\",upperdir=\"$upper\",workdir=\"$work\" \"$mountpoint\" 2>/dev/null || true; "
    done <<< "$_GWINE_KERNEL_OVERLAY_MOUNTS"
    echo "$script"
}

# Nettoie les marqueurs kernel overlay après usage
_cleanup_kernel_overlay_markers() {
    while IFS='|' read -r lower upper work mountpoint; do
        [ -z "$mountpoint" ] && continue
        rm -f "/tmp/gwine-ko-$( _kernel_overlay_id "$mountpoint" ).pid"
    done <<< "$_GWINE_KERNEL_OVERLAY_MOUNTS"
    _GWINE_KERNEL_OVERLAY_MOUNTS=""
    _GWINE_USING_KERNEL_OVERLAY=false
}

# Monte un overlay (kernel overlayfs différé)
# Usage: mount_overlay <lowerdir> <upperdir> <workdir> <mountpoint>
mount_overlay() {
    _require_kernel_overlay
    _register_kernel_overlay "$@"
}

# Démonte un overlay kernel
# Usage: unmount_overlay <mount_point> [-f] [-l]
unmount_overlay() {
    local mount_point="$1"
    local force=false
    local lazy=false
    
    shift
    while [ $# -gt 0 ]; do
        case "$1" in
            -f) force=true ;;
            -l) lazy=true ;;
        esac
        shift
    done
    
    if [ -z "$mount_point" ]; then
        return 0
    fi
    
    local pid_file="/tmp/gwine-ko-$( _kernel_overlay_id "$mount_point" ).pid"
    if [ -f "$pid_file" ]; then
        rm -f "$pid_file"
        return 0
    fi
    
    # Fallback: si le mount existe encore (stale), le démonter
    if mountpoint -q "$mount_point" 2>/dev/null; then
        umount -l "$mount_point" 2>/dev/null || true
    fi
    
    return 0
}

# Compatibilité — ancien nom, redirige vers unmount_overlay
unmount_fuse_overlay() {
    unmount_overlay "$@"
}

# Nettoie tous les overlays kernel orphelins (appelé au démarrage)
cleanup_kernel_overlay_pidfiles() {
    for pid_file in /tmp/gwine-ko-*.pid; do
        [ -f "$pid_file" ] || continue
        local marker
        marker=$(cat "$pid_file" 2>/dev/null)
        if [ "$marker" = "deferred" ]; then
            rm -f "$pid_file"
        elif [ -n "$marker" ] && kill -0 "$marker" 2>/dev/null; then
            kill "$marker" 2>/dev/null || true
            rm -f "$pid_file"
        else
            rm -f "$pid_file"
        fi
    done
}

# Appeler le nettoyage des overlays kernel orphelins au chargement
cleanup_kernel_overlay_pidfiles

# Gère le backup/restauration lors des mises à jour
# Usage: backup_component <component_dir>
# Retourne le chemin du backup ou vide
backup_component() {
    local component_dir="$1"
    local backup_dir="${component_dir}.backup"
    
    if [ -d "$component_dir" ]; then
        rm -rf "$backup_dir"
        mv "$component_dir" "$backup_dir"
        echo "$backup_dir"
    fi
}

# Restaure un backup si l'installation échoue
# Usage: restore_backup_component <backup_dir> <component_dir>
restore_backup_component() {
    local backup_dir="$1"
    local component_dir="$2"
    
    if [ -d "$backup_dir" ]; then
        rm -rf "$component_dir" 2>/dev/null
        mv "$backup_dir" "$component_dir"
    fi
}

# Nettoie un backup après installation réussie
# Usage: cleanup_backup <backup_dir>
cleanup_backup_component() {
    local backup_dir="$1"
    rm -rf "$backup_dir" 2>/dev/null || true
}

# Fonction générique pour les mises à jour avec backup
# Usage: update_component_with_backup <cache_dir> <pattern> <download_func>
update_component_with_backup() {
    local cache_dir="$1"
    local pattern="$2"
    local download_func="$3"
    local current_dir
    current_dir=$(find "$cache_dir" -mindepth 1 -maxdepth 1 -type d -name "$pattern" | sort -V | tail -1)
    local backup_dir=""
    
    # Créer le backup si existe
    if [ -n "$current_dir" ]; then
        backup_dir=$(backup_component "$current_dir")
    fi
    
    # Télécharger la nouvelle version
    if ! $download_func; then
        # En cas d'échec, restaurer le backup
        if [ -n "$backup_dir" ]; then
            restore_backup_component "$backup_dir" "$current_dir"
            echo "Version précédente conservée"
        fi
        return 1
    fi
    
    # Nettoyer le backup si succès
    cleanup_backup_component "$backup_dir"
    return 0
}

# Trouve le premier dossier correspondant à un pattern dans un répertoire
# Usage: find_component_dir <cache_dir> <pattern>
# Retourne le chemin du dossier trouvé ou vide
find_component_dir() {
    local cache_dir="$1"
    local pattern="$2"
    
    if [ ! -d "$cache_dir" ]; then
        return 1
    fi
    
    find "$cache_dir" -mindepth 1 -maxdepth 1 -type d -name "$pattern" | sort -V | tail -1
}

# =============================================================================
# Création des liens symboliques pour compatibilité WGP
# Ces liens permettent aux WGP d'accéder aux répertoires via /tmp/
# en mode sandbox et non-sandbox
# =============================================================================

# Créer les liens symboliques dans /tmp/
# Les WGP utilisent les chemins /tmp/wgp-* et /tmp/wgpackmount/
# Ces liens pointent vers le répertoire partagé
ensure_dir -s "$SHARED_TMP_DIR"
for subdir in wgpackmount wgp-extra wgp-saves wgp-temp; do
    if [ ! -L "/tmp/$subdir" ] || [ ! -e "/tmp/$subdir" ]; then
        rm -f "/tmp/$subdir" 2>/dev/null || true
        ln -sf "$SHARED_TMP_DIR/$subdir" "/tmp/$subdir" 2>/dev/null || true
    fi
done

# =============================================================================
# Gestion des processus
# =============================================================================

# Tue un processus et tous ses descendants récursivement
# Usage: kill_process_tree <pid> [signal]
kill_process_tree() {
    local pid=$1
    local sig=${2:-TERM}
    
    if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
        return 0
    fi
    
    # Trouver et tuer récursivement tous les enfants
    for child in $(pgrep -P "$pid" 2>/dev/null); do
        kill_process_tree "$child" "$sig"
    done
    
    # Tuer le processus lui-même
    kill "-$sig" "$pid" 2>/dev/null || true
}

# Tue les processus orphelins liés à un WGP spécifique
# Usage: kill_wgp_orphans <wgp_file> <wineprefix>
kill_wgp_orphans() {
    local wgp_file="$1"
    local wineprefix="$2"
    
    [ -z "$wgp_file" ] && return 0
    
    local wgp_name
    wgp_name=$(basename "$wgp_file" .wgp 2>/dev/null || basename "$wgp_file")
    
    echo "Recherche processus orphelins pour: $wgp_name"
    
    for pid in $(pgrep -f "\.exe" 2>/dev/null); do
        if [ -r "/proc/$pid/environ" ]; then
            if grep -q "WINEPREFIX=$wineprefix" "/proc/$pid/environ" 2>/dev/null; then
                local cmdline
                cmdline=$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null)
                if [[ "$cmdline" == *"$wgp_name"* ]]; then
                    if ! [[ "$cmdline" =~ (winedevice|plugplay|explorer|services|svchost|rpcss|wineboot|steam|msiexec|regsvr32|spoolsv|conhost|wmplayer|taskkill|wineconsole)\.exe ]]; then
                        echo "  Tue PID $pid: $(echo "$cmdline" | cut -c1-80)"
                        kill "$pid" 2>/dev/null || true
                    fi
                fi
            fi
        fi
    done
}

reset_gwine_options() {
    local options_file="$GWINE_DIR/options"
    local bind_dirs_file="${HOME}/.config/gwine/bind-dirs.conf"
    local changed=false

    echo "Réinitialisation des options gwine..."

    ensure_dir "$GWINE_DIR"

    if [ -f "$options_file" ]; then
        local old_runner old_dxvk old_display old_xbox old_xbox_filter
        old_runner=$(grep "^runner=" "$options_file" 2>/dev/null | cut -d'=' -f2)
        old_dxvk=$(grep "^dxvk_mode=" "$options_file" 2>/dev/null | cut -d'=' -f2)
        old_display=$(grep "^display_mode=" "$options_file" 2>/dev/null | cut -d'=' -f2)
        old_xbox=$(grep "^xbox_default=" "$options_file" 2>/dev/null | cut -d'=' -f2)
        old_xbox_filter=$(grep "^xbox_filter=" "$options_file" 2>/dev/null | cut -d'=' -f2)

        echo "  runner: ${old_runner:-non défini} -> proton"
        echo "  dxvk_mode: ${old_dxvk:-non défini} -> dxvk"
        echo "  display_mode: ${old_display:-x11} (inchangé)"
        echo "  xbox_default: ${old_xbox:-non défini} -> off"
        changed=true
    else
        echo "  Aucune option existante, création avec les valeurs par défaut"
        changed=true
    fi

    cat > "$options_file" << 'EOF'
runner=proton
dxvk_mode=dxvk
display_mode=x11
xbox_default=off
xbox_filter=all
EOF

    if [ -f "$bind_dirs_file" ] && [ -s "$bind_dirs_file" ]; then
        echo "  bind-dirs: vidé"
        : > "$bind_dirs_file"
        changed=true
    fi

    if [ "$changed" = true ]; then
        echo "Options réinitialisées aux valeurs par défaut."
    else
        echo "Les options étaient déjà aux valeurs par défaut."
    fi
}
