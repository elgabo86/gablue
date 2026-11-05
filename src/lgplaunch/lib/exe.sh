
# Détermine si un fichier est un exécutable lancable (binaire ELF, .sh, .py, .AppImage)
# Mêmes critères que makelgp.py pour la détection des exécutables
_lgp_is_executable() {
    local path="$1"
    local name_lower
    name_lower="$(basename "$path")"
    name_lower="${name_lower,,}"

    case "$name_lower" in
        *.sh|*.py|*.appimage)
            return 0
            ;;
    esac

    # Binaire ELF exécutable (bit +x + magic ELF)
    if [ -x "$path" ] && [ "$(head -c 4 "$path" 2>/dev/null)" = $'\x7fELF' ]; then
        return 0
    fi

    return 1
}

# Mode interactif : sélection d'un exécutable dans le LGP
# Surcharge le contenu de .launch (équivalent de gwine --exewgp pour les WGP)
# Lit quand même .args du pack. Deux entrées :
#   --exe=<rel>  → lance ce chemin du pack sans menu (ex. --exe=launch.sh)
#   --exe        → menu des exécutables détectés dans le mount (binaires ELF,
#                  .sh, .py, .AppImage, symlinks internes au pack)
select_exe_from_lgp() {
    if [ -n "${SELECT_EXE_PATH:-}" ]; then
        local rel="${SELECT_EXE_PATH#./}"
        case "$rel" in
            /*) echo "Erreur: --exe= attend un chemin RELATIF à la racine du pack: $SELECT_EXE_PATH" >&2
                cleanup_lgp
                exit 1 ;;
            *..*) echo "Erreur: --exe= refuse les '..': $SELECT_EXE_PATH" >&2
                cleanup_lgp
                exit 1 ;;
        esac
        FULL_EXE_PATH="$MOUNT_DIR/$rel"
        if [ ! -f "$FULL_EXE_PATH" ]; then
            echo "Erreur: exécutable introuvable dans le pack: $rel" >&2
            cleanup_lgp
            exit 1
        fi
        if ! _lgp_is_executable "$FULL_EXE_PATH"; then
            echo "Erreur: $rel n'est pas un exécutable lancable (ELF, .sh, .py, .AppImage)" >&2
            cleanup_lgp
            exit 1
        fi
        REAL_EXE_PATH="$(realpath "$FULL_EXE_PATH" 2>/dev/null)"
        [ -z "$REAL_EXE_PATH" ] && REAL_EXE_PATH="$FULL_EXE_PATH"
        echo "Exécutable sélectionné: $rel"
        return 0
    fi

    echo "Recherche des exécutables dans le pack..."

    local found=0
    local exe_array=()
    local exe_path exe_rel target resolved

    # Balayer le mount SANS suivre les symlinks (find -L suivrait les symlinks
    # /tmp/lgp-saves/... hors du pack) : les symlinks sont traités manuellement.
    # Deux passes ordonnées : la RACINE du pack d'abord (.launch, game.sh,
    # launch.sh, binaires — c'est ce qu'on veut proposer), puis le reste de
    # l'arbre sans les bibliothèques partagées (des ELF +x qui noyaient le menu :
    # 20 entrées de app/usr/lib avant d'atteindre la racine) ni la plomberie du
    # pack (.rebuild = outils de maintenance, .save = données amorcées).
    while IFS= read -r -d '' exe_path; do
        exe_rel="${exe_path#$MOUNT_DIR/}"
        # Exclure le script de pré-lancement (hook, pas un jeu)
        [ "$exe_rel" = ".script.sh" ] && continue

        if [ -L "$exe_path" ]; then
            # Symlink : ne considérer que les cibles relatives internes au pack
            target=$(readlink "$exe_path")
            [[ "$target" = /* ]] && continue
            resolved=$(realpath -m "$(dirname "$exe_path")/$target" 2>/dev/null) || continue
            [[ "$resolved" != "$MOUNT_DIR"/* ]] && continue
            [ -f "$resolved" ] || continue
            if _lgp_is_executable "$resolved"; then
                exe_array+=("$exe_path")
                found=$((found + 1))
            fi
        elif [ -f "$exe_path" ]; then
            if _lgp_is_executable "$exe_path"; then
                exe_array+=("$exe_path")
                found=$((found + 1))
            fi
        fi

        # Limiter la taille du menu (comme gwine --exewgp)
        [ $found -ge 40 ] && break
    done < <(
        # chaque passe est triée SÉPARÉMENT : un tri global mélangerait la
        # racine et l'arbre (app/... passe avant game.sh à l'alphabet)
        find "$MOUNT_DIR" -maxdepth 1 \( -type f -o -type l \) -print0 2>/dev/null | sort -z
        find "$MOUNT_DIR" -mindepth 2 \( -type f -o -type l \) \
            ! -path "$MOUNT_DIR/.rebuild/*" ! -path "$MOUNT_DIR/.save/*" \
            ! -path '*/lib/*' ! -name '*.so' ! -name '*.so.*' \
            ! -name '*.a' ! -name '*.o' ! -name '*.dll' -print0 2>/dev/null | sort -z
    )

    if [ $found -eq 0 ]; then
        echo "Aucun exécutable trouvé dans le pack"
        cleanup_lgp
        exit 1
    fi

    local menu_args=("Choisissez un exécutable à lancer :")

    for exe_path in "${exe_array[@]}"; do
        local rel_path="${exe_path#$MOUNT_DIR/}"
        menu_args+=("$rel_path" "$rel_path")
    done

    local EXE_REL_PATH
    local exit_status=0
    if command -v kdialog &> /dev/null; then
        EXE_REL_PATH=$(kdialog --menu "${menu_args[@]}") || exit_status=$?
    else
        echo ""
        echo "Exécutables disponibles:"
        for i in "${!exe_array[@]}"; do
            local rel_path="${exe_array[$i]#$MOUNT_DIR/}"
            echo "  $((i+1)). $rel_path"
        done
        echo ""
        # stdin du heredoc du namespace est le script lui-même : lire depuis /dev/tty
        read -rp "Entrez le numéro de l'exécutable: " choice < /dev/tty 2>/dev/null || exit_status=1
        if [ $exit_status -eq 0 ] && [[ "$choice" =~ ^[0-9]+$ ]] && [ "$choice" -ge 1 ] && [ "$choice" -le ${#exe_array[@]} ]; then
            EXE_REL_PATH="${exe_array[$((choice-1))]#$MOUNT_DIR/}"
        else
            exit_status=1
        fi
    fi

    if [ $exit_status -ne 0 ] || [ -z "$EXE_REL_PATH" ]; then
        echo "Annulé par l'utilisateur"
        cleanup_lgp
        exit 0
    fi

    FULL_EXE_PATH="$MOUNT_DIR/$EXE_REL_PATH"

    if [ ! -f "$FULL_EXE_PATH" ]; then
        echo "Erreur: exécutable introuvable: $FULL_EXE_PATH"
        cleanup_lgp
        exit 1
    fi

    # Résoudre les symlinks (même logique que read_lgp_config)
    if [ -L "$FULL_EXE_PATH" ]; then
        REAL_EXE_PATH="$(realpath "$FULL_EXE_PATH" 2>/dev/null)"
        echo "Symlink détecté: $FULL_EXE_PATH -> $REAL_EXE_PATH"
    else
        REAL_EXE_PATH="$(realpath "$FULL_EXE_PATH" 2>/dev/null)"
        if [ -z "$REAL_EXE_PATH" ]; then
            REAL_EXE_PATH="$FULL_EXE_PATH"
        fi
    fi

    echo "Exécutable sélectionné: $EXE_REL_PATH"
}

