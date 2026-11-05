# Fonction principale pour le mode LGP
run_lgp_mode() {
    _LGP_MODE=true
    init_lgp_variables
    mount_lgp

    # Nettoyage en cas d'interruption
    trap cleanup_lgp EXIT
    
    # Nettoyer /tmp/edenln-$UID s'il existe (contient des vieux symlinks de edenln.sh)
    if [ -d "/tmp/edenln-$UID" ]; then
        echo "Nettoyage de /tmp/edenln-$UID..."
        rm -rf /tmp/edenln-$UID/* /tmp/edenln-$UID/.* 2>/dev/null || true
    fi

    # Créer les dossiers de base UID-spécifiques sur l'hôte
    for base in lgp-saves lgp-extra lgp-temp lgpackmount edenln \
                lgp-temp-upper lgp-temp-work \
                lgp-full-overlay lgp-full-overlay-upper lgp-full-overlay-work; do
        mkdir -p "/tmp/$base-$UID" 2>/dev/null
    done

    # Exporter les fonctions nécessaires dans le namespace
    export -f _lgp_setup_bind_mounts _lgp_kernel_overlay_mount_script \
        _lgp_cleanup_kernel_overlay_markers _lgp_register_kernel_overlay \
        _lgp_kernel_overlay_id _lgp_require_kernel_overlay mount_overlay unmount_overlay \
        _lgp_cleanup_kernel_overlay_pidfiles \
        setup_saves_symlink setup_extras_symlink setup_temp_symlink \
        cleanup_saves_symlink cleanup_extras_symlink cleanup_temp_symlink \
        prepare_saves prepare_extras prepare_temps prepare_full_overlay \
        _copy_dir_with_symlinks _copy_symlink_as_abs \
        _copy_dir_contents _copy_dir_recursive \
        execute_prelaunch_script read_lgp_config read_lgp_args \
        _lgp_is_executable select_exe_from_lgp load_env_files load_env_file \
        has_saves has_extras has_temps \
        error_exit cleanup_lgp launch_game \
        2>/dev/null

    # Exporter les variables globales utilisées par les fonctions
    export _LGP_MODE MOUNT_DIR MOUNT_BASE LGPACK_NAME GAME_INTERNAL_NAME \
           SAVES_SYMLINK SAVES_REAL EXTRA_BASE EXTRA_DIR EXTRA_SYMLINK EXTRA_REAL \
           TEMP_SYMLINK TEMP_REAL \
           args FULL_EXE_PATH REAL_EXE_PATH HOME_REAL SELECT_EXE_MODE \
           SELECT_EXE_PATH \
           _LGP_KERNEL_OVERLAY_MOUNTS _LGP_USING_KERNEL_OVERLAY \
           _GAME_TEMP_ID _FULL_OVERLAY_MOUNT _FULL_OVERLAY_UPPER _FULL_OVERLAY_WORK \
           UID

    # Exécuter tout le setup et le jeu dans un namespace de montage privé
    # Les symlinks traduisent /tmp/lgp-* → /tmp/lgp-*-$UID
    unshare -U -m --map-root-user bash <<'LGP_NAMESPACE_EOF'
        _lgp_setup_bind_mounts

        setup_saves_symlink
        setup_extras_symlink
        setup_temp_symlink
        prepare_saves
        prepare_extras
        prepare_temps
        execute_prelaunch_script
        if [ "${SELECT_EXE_MODE:-false}" = "true" ]; then
            # Mode --exe : sélection interactive d'un exécutable dans le pack
            # (surcharge le contenu de .launch, .args du pack reste chargé)
            select_exe_from_lgp
        else
            read_lgp_config
        fi

        exe_dir="$(dirname "$REAL_EXE_PATH")"
        load_env_files "$MOUNT_DIR" "$exe_dir"

        if [ "$FULL_EXE_PATH" != "$REAL_EXE_PATH" ]; then
            work_dir="$(dirname "$FULL_EXE_PATH")"
        else
            work_dir="$exe_dir"
        fi

        launch_game "$REAL_EXE_PATH" "$args" "$LGPACK_NAME" "$work_dir"

        cleanup_saves_symlink
        cleanup_extras_symlink
        cleanup_temp_symlink
        _lgp_cleanup_kernel_overlay_markers 2>/dev/null || true
LGP_NAMESPACE_EOF
    local game_exit=$?

    echo "Jeu terminé avec le code: $game_exit"
    return $game_exit
}

# Fonction principale pour le mode classique (exécutable direct)
run_classic_mode() {
    local dirpath
    local filename

    dirpath=$(dirname "$fullpath")
    filename=$(basename "$fullpath")

    # Résoudre le symlink si nécessaire
    local real_path="$fullpath"
    if [ -L "$fullpath" ]; then
        real_path="$(realpath "$fullpath")"
        echo "Symlink détecté: $fullpath -> $real_path"
    fi

    # Exécuter le script de pré-lancement si existe
    local script_file="$dirpath/.script.sh"
    if [ -f "$script_file" ]; then
        echo "Exécution du script de pré-lancement..."
        chmod +x "$script_file" 2>/dev/null
        (cd "$dirpath" && "$script_file")
    fi

    # Charger les variables d'environnement du fichier .env
    local dir_env="$dirpath/.env"
    if [ -f "$dir_env" ]; then
        echo "Chargement des variables d'environnement depuis $dir_env..."
        load_env_file "$dir_env"
    fi

    # Lancer le jeu
    launch_game "$real_path" "$args" "$filename"
}

