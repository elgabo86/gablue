#!/bin/bash

################################################################################
# init.sh - Initialisation du système de locks
################################################################################

WINEPREFIX_LOCK_FILE="$GWINE_LOCK_DIR/wineprefix-count"
WINE_SOCKET_DIR="$GWINE_LOCK_DIR/wine-socket"

MASTER_INSTANCE_FILE="$GWINE_LOCK_DIR/master-instance"
MASTER_PID_FILE="$GWINE_LOCK_DIR/master-pid"
MASTER_USERNS_FILE="$GWINE_LOCK_DIR/master-userns"
MASTER_PIDNS_FILE="$GWINE_LOCK_DIR/master-pidns"

init_wineserver_manager() {
    ensure_dir -s "$GWINE_LOCK_DIR"
    chmod 700 "$GWINE_LOCK_DIR"
    ensure_dir -s "$SHARED_TMP_DIR"
    chmod 711 "$SHARED_TMP_DIR"
    local wine_socket_subdir="$SHARED_TMP_DIR/.wine-$UID"
    ensure_dir -s "$wine_socket_subdir"
    chmod 700 "$wine_socket_subdir"

    # Les symlinks globaux /tmp/wgp-* ne sont nécessaires qu'en mode --nosandbox.
    # En mode sandbox (défaut), /tmp est bind-monté depuis $SHARED_TMP_DIR,
    # donc les chemins sont déjà par utilisateur.
    if [ "${nosandbox_mode:-false}" = true ]; then
        for shared in saves extra temp; do
            local global_link="/tmp/wgp-$shared"
            local user_dir="$SHARED_TMP_DIR/wgp-$shared"
            ensure_dir -s "$user_dir"
            chmod 777 "$user_dir"
            rm -rf "$global_link" 2>/dev/null || true
            ln -sf "$user_dir" "$global_link" 2>/dev/null || true
        done
    fi
}

get_shared_tmp_dir() {
    echo "$SHARED_TMP_DIR"
}

get_wine_socket_dir() {
    echo "$SHARED_TMP_DIR/.wine-$UID"
}
