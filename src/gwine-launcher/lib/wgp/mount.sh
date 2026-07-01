#!/bin/bash

################################################################################
# wgp-mount.sh - Montage et démontage des packs WGP (squashfuse)
################################################################################

# =============================================================================
# Nettoyage des overlays orphelins (appelé au début du montage)
# =============================================================================

cleanup_orphan_overlays() {
    # Vérifie et démonte les overlays qui pourraient être restés montés
    # C'est très rapide car mountpoint -q est instantané
    
    local mount_point
    
    # Nettoyer les overlays full (/tmp/wgp-full-overlay/*)
    if [ -d "/tmp/wgp-full-overlay" ]; then
        for mount_point in /tmp/wgp-full-overlay/*; do
            [ -d "$mount_point" ] || continue
            if mountpoint -q "$mount_point" 2>/dev/null; then
                # Vérifier si un processus gwine utilise encore cet overlay
                local game_id
                game_id=$(basename "$mount_point")
                local lock_file="$GWINE_LOCK_DIR/wgp-lock-${game_id#wgp-}"
                # Rétrocompatibilité: vérifier aussi l'ancien chemin
                [ ! -f "$lock_file" ] && lock_file="/tmp/wgp-lock-${game_id#wgp-}"
                local should_unmount=true
                
                if [ -f "$lock_file" ]; then
                    local lock_pid
                    lock_pid=$(cat "$lock_file" 2>/dev/null | cut -d: -f1)
                    if [ -n "$lock_pid" ] && kill -0 "$lock_pid" 2>/dev/null; then
                        should_unmount=false
                    fi
                fi
                
                if [ "$should_unmount" = true ]; then
                    echo "Démontage de l'overlay orphelin: $mount_point"
                    unmount_overlay "$mount_point" -f 2>/dev/null || true
                    rm -rf "$mount_point" 2>/dev/null || true
                    rm -rf "/tmp/wgp-full-overlay-upper/${game_id}" 2>/dev/null || true
                    rm -rf "/tmp/wgp-full-overlay-work/${game_id}" 2>/dev/null || true
                fi
            fi
        done
    fi
    
    # Nettoyer les overlays temp folders (/tmp/wgp-temp/*)
    if [ -d "/tmp/wgp-temp" ]; then
        for mount_point in /tmp/wgp-temp/*; do
            [ -d "$mount_point" ] || continue
            if mountpoint -q "$mount_point" 2>/dev/null; then
                local game_id
                game_id=$(basename "$mount_point")
                local lock_file="$GWINE_LOCK_DIR/wgp-lock-${game_id#wgp-}"
                # Rétrocompatibilité: vérifier aussi l'ancien chemin
                [ ! -f "$lock_file" ] && lock_file="/tmp/wgp-lock-${game_id#wgp-}"
                local should_unmount=true
                
                if [ -f "$lock_file" ]; then
                    local lock_pid
                    lock_pid=$(cat "$lock_file" 2>/dev/null | cut -d: -f1)
                    if [ -n "$lock_pid" ] && kill -0 "$lock_pid" 2>/dev/null; then
                        should_unmount=false
                    fi
                fi
                
                if [ "$should_unmount" = true ]; then
                    echo "Démontage de l'overlay temp orphelin: $mount_point"
                    unmount_overlay "$mount_point" -f 2>/dev/null || true
                    rm -rf "$mount_point" 2>/dev/null || true
                    rm -rf "/tmp/wgp-temp-upper/${game_id}" 2>/dev/null || true
                    rm -rf "/tmp/wgp-temp-work/${game_id}" 2>/dev/null || true
                fi
            fi
        done
    fi
}

# =============================================================================
# Montage du pack WGP
# =============================================================================

mount_wgp() {
    ensure_dir -s "$MOUNT_BASE"
    
    # Nettoyer les overlays orphelins au cas où un précédent crash les aurait laissés
    cleanup_orphan_overlays
    
    # Vérifier si le MOUNT_DIR existe et est stale (Transport endpoint is not connected)
    if [ -d "$MOUNT_DIR" ]; then
        if ! ls "$MOUNT_DIR" >/dev/null 2>&1; then
            echo "Point de montage stale détecté: $MOUNT_DIR"
            echo "Nettoyage forcé..."
            # D'abord démonter (lazy), PUIS tuer les processus
            fusermount -uz "$MOUNT_DIR" 2>/dev/null || umount -l "$MOUNT_DIR" 2>/dev/null || true
            sleep 0.3
            # Maintenant on peut tuer les processus FUSE
            pkill -9 -f "squashfuse.*$(basename "$WGPACK_NAME")" 2>/dev/null || true
            pkill -9 -f "squashfuse.*$MOUNT_DIR" 2>/dev/null || true
            sleep 0.3
            # Supprimer le répertoire
            rmdir "$MOUNT_DIR" 2>/dev/null || rm -rf "$MOUNT_DIR" 2>/dev/null || true
            # Vérifier que c'est bien nettoyé
            if [ -d "$MOUNT_DIR" ]; then
                echo "Erreur: impossible de nettoyer $MOUNT_DIR"
                exit 1
            fi
        fi
    fi
    
    local LOCK_FILE="$GWINE_LOCK_DIR/wgp-lock-$WGPACK_NAME"
    
    if [ -f "$LOCK_FILE" ]; then
        local LOCK_PID
        LOCK_PID=$(cat "$LOCK_FILE" 2>/dev/null | cut -d: -f1)
        if [ -n "$LOCK_PID" ] && kill -0 "$LOCK_PID" 2>/dev/null; then
            local RELAUNCH=false
            local QUESTION="$WGPACK_NAME est déjà lancé.\n\nVoulez-vous arrêter l'instance en cours et relancer le jeu ?"
            
            if command -v kdialog &> /dev/null; then
                kdialog --warningyesno "$QUESTION" --yes-label "Oui, relancer" --no-label "Non, annuler" && RELAUNCH=true
            else
                read -p "$QUESTION (o/N): " -r
                [[ "$REPLY" =~ ^[oOyY]$ ]] && RELAUNCH=true
            fi
            
            if [ "$RELAUNCH" = true ]; then
                echo "Arrêt de l'instance en cours..."
                # Créer un marqueur de relancement pour empêcher le démontage par l'ancienne instance
                local RELAUNCH_MARKER="$GWINE_LOCK_DIR/wgp-relaunch-$WGPACK_NAME"
                touch "$RELAUNCH_MARKER"
                # Tuer le processus principal
                kill -9 "$LOCK_PID" 2>/dev/null
                # Tuer tous les processus enfants du groupe de processus
                kill -9 -- -"$LOCK_PID" 2>/dev/null || true
                # Tuer spécifiquement tous les processus bwrap associés à ce WGP
                local mount_escape
                mount_escape=$(printf '%s' "$MOUNT_DIR" | sed 's/[[\.*^$()+?{|\\]/\\&/g')
                pkill -9 -f "bwrap.*$mount_escape" 2>/dev/null || true
                # Tuer tous les processus wine associés à ce préfixe
                pkill -9 -f "wine.*$WINEPREFIX" 2>/dev/null || true
                # Attendre que tous les processus se terminent
                sleep 1
                # Nettoyer nous-mêmes les montages si nécessaire (l'ancien cleanup ne le fera pas à cause du marqueur)
                unmount_overlay "$MOUNT_DIR" -f
                local OVERLAY_DIR="/tmp/wgp-full-overlay/wgp-$(echo "$WGPACK_NAME" | tr -cd '[:alnum:]-')"
                unmount_overlay "$OVERLAY_DIR" -f
                rm -f "$LOCK_FILE"
                # Le marqueur sera supprimé après le montage réussi
            else
                error_exit "$WGPACK_NAME est déjà en cours d'exécution"
            fi
        else
            rm -f "$LOCK_FILE"
        fi
    fi

    if mountpoint -q "$MOUNT_DIR"; then
        echo "Montage orphelin détecté pour $WGPACK_NAME, nettoyage..."
        # Tester si le montage est accessible (pas de "Transport endpoint not connected")
        if ! ls "$MOUNT_DIR" >/dev/null 2>&1; then
            echo "Montage stale détecté (endpoint déconnecté)..."
            # Vérifier qu'aucun processus n'utilise ce point de montage
            local HAS_ACCESS=false
            if command -v fuser >/dev/null 2>&1 && fuser "$MOUNT_DIR" 2>/dev/null | grep -q .; then
                HAS_ACCESS=true
            elif command -v lsof >/dev/null 2>&1 && lsof +D "$MOUNT_DIR" 2>/dev/null | grep -q .; then
                HAS_ACCESS=true
            fi
            if [ "$HAS_ACCESS" = "false" ]; then
                echo "Aucun processus n'utilise le montage, forçage du démontage..."
                # Lazy unmount pour les montages stale
                umount -l "$MOUNT_DIR" 2>/dev/null || true
            else
                echo "Des processus utilisent encore ce montage, attente..."
                sleep 2
            fi
        fi
        unmount_overlay "$MOUNT_DIR" -f -l
    fi

    local SQUASHFUSE_BIN
    SQUASHFUSE_BIN=$(get_system_tool squashfuse)

    ensure_dir -s "$MOUNT_DIR"
    echo "Montage de $WGPACK_FILE sur $MOUNT_DIR..."
    "$SQUASHFUSE_BIN" -r "$WGPACK_FILE" "$MOUNT_DIR"

    if [ $? -ne 0 ]; then
        echo "Échec du montage, nettoyage forcé du point de montage..."
        # D'abord démonter (lazy), PUIS tuer les processus
        fusermount -uz "$MOUNT_DIR" 2>/dev/null || umount -l "$MOUNT_DIR" 2>/dev/null || true
        sleep 0.3
        # Maintenant on peut tuer les processus FUSE
        pkill -9 -f "squashfuse.*$(basename "$WGPACK_NAME")" 2>/dev/null || true
        pkill -9 -f "squashfuse.*$MOUNT_DIR" 2>/dev/null || true
        sleep 0.3
        # Supprimer le répertoire
        rmdir "$MOUNT_DIR" 2>/dev/null || rm -rf "$MOUNT_DIR" 2>/dev/null || true
        
        if [ -d "$MOUNT_DIR" ]; then
            error_exit "Impossible de nettoyer le point de montage $MOUNT_DIR"
        fi
        
        # Réessayer le montage une fois
        ensure_dir -s "$MOUNT_DIR"
        echo "Nouvelle tentative de montage sur $MOUNT_DIR..."
        "$SQUASHFUSE_BIN" -r "$WGPACK_FILE" "$MOUNT_DIR"
        
        if [ $? -ne 0 ]; then
            error_exit "Erreur lors du montage du squashfs"
        fi
    fi
    
    echo "$$:$(date +%s)" > "$LOCK_FILE"
    
    # Supprimer le marqueur de relancement après montage réussi
    rm -f "$GWINE_LOCK_DIR/wgp-relaunch-$WGPACK_NAME" 2>/dev/null || true
}

# =============================================================================
# Démontage et nettoyage du pack WGP
# =============================================================================

cleanup_wgp() {
    # Vérifier si un relancement est en cours - si oui, ne pas démonter
    local RELAUNCH_MARKER="$GWINE_LOCK_DIR/wgp-relaunch-$WGPACK_NAME"
    if [ -f "$RELAUNCH_MARKER" ]; then
        return 0
    fi
    
    # Nettoyer les overlays temporaires d'abord (ils utilisent le montage principal comme lowerdir)
    cleanup_saves_symlink
    cleanup_extras_symlink
    cleanup_temp_symlink
    
    echo "Démontage de $WGPACK_NAME..."
    
    # D'abord démonter (lazy), PUIS tuer les processus
    fusermount -uz "$MOUNT_DIR" 2>/dev/null || umount -l "$MOUNT_DIR" 2>/dev/null || true
    sleep 0.3
    
    # Vérifier si le montage est toujours présent
    if [ -d "$MOUNT_DIR" ] && ! ls "$MOUNT_DIR" >/dev/null 2>&1; then
        echo "Montage stale, nettoyage forcé..."
        # Tuer les processus FUSE
        pkill -9 -f "squashfuse.*$WGPACK_NAME" 2>/dev/null || true
        pkill -9 -f "squashfuse.*$MOUNT_DIR" 2>/dev/null || true
        sleep 0.3
        # Forcer le démontage
        umount -l "$MOUNT_DIR" 2>/dev/null || true
    fi
    
    # Nettoyer les répertoires
    if [ -d "$MOUNT_DIR" ]; then
        rmdir "$MOUNT_DIR" 2>/dev/null || rm -rf "$MOUNT_DIR" 2>/dev/null || true
    fi
    
    rm -f "$GWINE_LOCK_DIR/wgp-lock-$WGPACK_NAME" 2>/dev/null || true
}
