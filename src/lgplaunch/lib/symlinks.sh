# Vérifie si le LGP contient des fichiers de sauvegarde
has_saves() {
    [ -f "$MOUNT_DIR/.savepath" ]
}

# Crée le symlink /tmp/lgp-saves/$GAME_INTERNAL_NAME vers ~/.local/share/lgp-saves
# Un symlink par jeu permet de lancer plusieurs LGP en parallèle
setup_saves_symlink() {
    # Ne rien faire si le LGP n'a pas de saves
    has_saves || return 0

    local GAME_SAVES_DIR="$SAVES_REAL/$GAME_INTERNAL_NAME"
    local GAME_SAVES_SYMLINK="$SAVES_SYMLINK/$GAME_INTERNAL_NAME"

    # Créer le dossier de sauvegardes réel pour ce jeu
    mkdir -p "$GAME_SAVES_DIR"

    # Créer le dossier /tmp/lgp-saves si nécessaire
    mkdir -p "$SAVES_SYMLINK"

    # Supprimer l'ancien symlink du jeu s'il existe
    if [ -L "$GAME_SAVES_SYMLINK" ]; then
        rm -f "$GAME_SAVES_SYMLINK"
    fi

    # Créer le symlink pour ce jeu
    ln -s "$GAME_SAVES_DIR" "$GAME_SAVES_SYMLINK"
    echo "Symlink créé: $GAME_SAVES_SYMLINK -> $GAME_SAVES_DIR"
}

# Supprime le symlink /tmp/lgp-saves/$GAME_INTERNAL_NAME
cleanup_saves_symlink() {
    local GAME_SAVES_SYMLINK="$SAVES_SYMLINK/$GAME_INTERNAL_NAME"
    [ -L "$GAME_SAVES_SYMLINK" ] && rm -f "$GAME_SAVES_SYMLINK"
}

# Vérifie si le LGP contient des fichiers d'extra
has_extras() {
    [ -f "$MOUNT_DIR/.extrapath" ]
}

# Vérifie si le LGP contient des fichiers temporaires
has_temps() {
    [ -f "$MOUNT_DIR/.temppath" ]
}

# Crée le symlink /tmp/lgp-extra/$GAME_INTERNAL_NAME vers ~/.cache/lgp-extra
# Un symlink par jeu permet de lancer plusieurs LGP en parallèle
setup_extras_symlink() {
    # Ne rien faire si le LGP n'a pas d'extras
    has_extras || return 0

    local GAME_EXTRAS_DIR="$EXTRA_REAL/$GAME_INTERNAL_NAME"
    local GAME_EXTRAS_SYMLINK="$EXTRA_SYMLINK/$GAME_INTERNAL_NAME"

    # Créer le dossier d'extras réel pour ce jeu
    mkdir -p "$GAME_EXTRAS_DIR"

    # Créer le dossier /tmp/lgp-extra si nécessaire
    mkdir -p "$EXTRA_SYMLINK"

    # Supprimer l'ancien symlink du jeu s'il existe
    if [ -L "$GAME_EXTRAS_SYMLINK" ]; then
        rm -f "$GAME_EXTRAS_SYMLINK"
    fi

    # Créer le symlink pour ce jeu
    ln -s "$GAME_EXTRAS_DIR" "$GAME_EXTRAS_SYMLINK"
    echo "Symlink créé: $GAME_EXTRAS_SYMLINK -> $GAME_EXTRAS_DIR"
}

# Supprime le symlink /tmp/lgp-extra/$GAME_INTERNAL_NAME
cleanup_extras_symlink() {
    local GAME_EXTRAS_SYMLINK="$EXTRA_SYMLINK/$GAME_INTERNAL_NAME"
    [ -L "$GAME_EXTRAS_SYMLINK" ] && rm -f "$GAME_EXTRAS_SYMLINK"
}

# Prépare les dossiers pour le montage overlay des fichiers temporaires
# Crée upperdir et workdir pour l'overlayfs
setup_temp_symlink() {
    # Ne rien faire si le LGP n'a pas de temps
    has_temps || return 0
    
    # Vérifier si c'est le mode "full overlay" - dans ce cas, on ne prépare rien ici
    # prepare_full_overlay() gérera tout
    local TEMPPATH_FILE="$MOUNT_DIR/.temppath"
    if [ -f "$TEMPPATH_FILE" ] && grep -q "^\*$" "$TEMPPATH_FILE" 2>/dev/null; then
        echo "Mode overlay complet détecté - skip setup_temp_symlink"
        return 0
    fi

    # Utiliser un ID unique sans espaces pour les chemins de travail
    local GAME_ID="lgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')"
    local GAME_TEMP_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"
    local GAME_TEMP_UPPER="/tmp/lgp-temp-upper-$UID/$GAME_ID"
    local GAME_TEMP_WORK="/tmp/lgp-temp-work-$UID/$GAME_ID"
    
    # Stocker les chemins réels pour le nettoyage
    export _GAME_TEMP_ID="$GAME_ID"

    # Nettoyer les anciens dossiers s'ils existent
    if [ -d "$GAME_TEMP_UPPER" ]; then
        rm -rf "$GAME_TEMP_UPPER"
    fi
    if [ -d "$GAME_TEMP_WORK" ]; then
        rm -rf "$GAME_TEMP_WORK"
    fi

    # Créer les dossiers pour l'overlay avec permissions utilisateur
    mkdir -p "$GAME_TEMP_DIR"
    mkdir -p "$GAME_TEMP_UPPER"
    mkdir -p "$GAME_TEMP_WORK"
    
    # Définir les permissions pour l'overlay (l'utilisateur doit pouvoir écrire)
    chmod 777 "$GAME_TEMP_UPPER"
    chmod 777 "$GAME_TEMP_WORK"
    
    echo "Dossiers overlay préparés pour: $GAME_TEMP_DIR"
    echo "  upperdir: $GAME_TEMP_UPPER"
    echo "  workdir: $GAME_TEMP_WORK"
}

# Démonte l'overlay kernel et nettoie les dossiers temporaires
cleanup_temp_symlink() {
    local GAME_TEMP_DIR="$TEMP_REAL/$GAME_INTERNAL_NAME"
    local GAME_ID="${_GAME_TEMP_ID:-lgp-$(echo "$GAME_INTERNAL_NAME" | tr -cd '[:alnum:]-')}" 
    local GAME_TEMP_UPPER="/tmp/lgp-temp-upper-$UID/$GAME_ID"
    local GAME_TEMP_WORK="/tmp/lgp-temp-work-$UID/$GAME_ID"
    
    if [ -n "$_FULL_OVERLAY_MOUNT" ]; then
        echo "Nettoyage de l'overlay complet..."
        unmount_overlay "$_FULL_OVERLAY_MOUNT"
        
        rm -rf "$_FULL_OVERLAY_MOUNT" 2>/dev/null || true
        rm -rf "$_FULL_OVERLAY_UPPER" 2>/dev/null || true
        rm -rf "$_FULL_OVERLAY_WORK" 2>/dev/null || true
        
        unset _FULL_OVERLAY_MOUNT _FULL_OVERLAY_UPPER _FULL_OVERLAY_WORK
        return 0
    fi
    
    unmount_overlay "$GAME_TEMP_DIR"
    
    rm -rf "$GAME_TEMP_DIR" 2>/dev/null || true
    rm -rf "$GAME_TEMP_UPPER" 2>/dev/null || true
    rm -rf "$GAME_TEMP_WORK" 2>/dev/null || true
}

