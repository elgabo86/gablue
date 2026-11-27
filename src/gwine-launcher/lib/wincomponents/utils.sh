#!/bin/bash

################################################################################
# utils.sh - Fonctions utilitaires pour les composants Windows
################################################################################

calculate_sha256() {
    local file="$1"
    if command -v sha256sum &>/dev/null; then
        sha256sum "$file" | cut -d' ' -f1
    elif command -v shasum &>/dev/null; then
        shasum -a 256 "$file" | cut -d' ' -f1
    else
        echo ""
    fi
}

verify_component_file() {
    local file="$1"
    local expected_sha256="$2"
    
    if [ ! -f "$file" ]; then
        return 1
    fi
    
    if [ -z "$expected_sha256" ]; then
        return 0
    fi
    
    local actual_sha256
    actual_sha256=$(calculate_sha256 "$file")
    
    if [ "$actual_sha256" = "$expected_sha256" ]; then
        return 0
    else
        return 1
    fi
}

download_component() {
    local name="$1"
    local urls="${COMPONENT_URLS[$name]}"
    local sha256="${COMPONENT_SHA256[$name]}"

    if [ -z "$urls" ]; then
        echo "Erreur: URL non définie pour le composant $name"
        return 1
    fi

    # Liste d'URLs séparées par des espaces : la première est la source
    # principale, les suivantes sont des miroirs de secours (fallback)
    # shellcheck disable=SC2206
    local url_list=($urls)
    local filename=$(basename "${url_list[0]}")
    local cache_file="$WINCOMPONENTS_CACHE/$name/$filename"

    ensure_dir "$WINCOMPONENTS_CACHE/$name"

    if verify_component_file "$cache_file" "$sha256"; then
        echo "Composant $name déjà en cache"
        return 0
    fi

    echo "  - Téléchargement de $name..."
    local url downloaded=false is_fallback=false
    for url in "${url_list[@]}"; do
        if [ "$is_fallback" = true ]; then
            echo "    → Essai du miroir de secours..."
        fi
        if wget -q --timeout=60 --tries=2 "$url" -O "$cache_file" 2>/dev/null; then
            downloaded=true
            break
        fi
        rm -f "$cache_file"
        is_fallback=true
    done

    if [ "$downloaded" != true ]; then
        echo "    ✗ Échec du téléchargement"
        return 1
    fi
    
    if [ -n "$sha256" ] && ! verify_component_file "$cache_file" "$sha256"; then
        echo "Erreur: Checksum incorrect pour $name"
        rm -f "$cache_file"
        return 1
    fi
    
    echo "✓ $name téléchargé avec succès"
    return 0
}

check_wincomponents_cache() {
    local missing=()
    
    for component in "${WINCOMPONENTS_REQUIRED[@]}"; do
        local urls="${COMPONENT_URLS[$component]}"
        if [ -z "$urls" ]; then
            continue
        fi

        # Nom de fichier issu de la première URL (source principale)
        # shellcheck disable=SC2206
        local url_list=($urls)
        local filename=$(basename "${url_list[0]}")
        local cache_file="$WINCOMPONENTS_CACHE/$component/$filename"
        local sha256="${COMPONENT_SHA256[$component]}"
        
        if ! verify_component_file "$cache_file" "$sha256"; then
            missing+=("$component")
        fi
    done
    
    if [ ${#missing[@]} -gt 0 ]; then
        echo "Composants manquants dans le cache:"
        for comp in "${missing[@]}"; do
            echo "  - $comp"
        done
        return 1
    fi
    
    return 0
}

download_all_wincomponents() {
    echo "Téléchargement des composants Windows..."
    
    local failed=()
    
    for component in "${WINCOMPONENTS_REQUIRED[@]}"; do
        if ! download_component "$component"; then
            failed+=("$component")
        fi
    done
    
    if [ ${#failed[@]} -gt 0 ]; then
        echo "Erreur: Échec du téléchargement de certains composants:"
        for comp in "${failed[@]}"; do
            echo "  - $comp"
        done
        return 1
    fi
    
    echo "✓ Tous les composants sont dans le cache"
    return 0
}

# Lance un installateur Windows silencieux dans le préfixe.
#
# Les bootstrappers WiX/Burn de Microsoft (.NET Desktop Runtime, VC++ 2012+)
# créent une fenêtre MÊME avec /quiet : sous DISPLAY/WAYLAND_DISPLAY vides
# (init headless), ils sortent « Failed to create window » (0x80070578) sans
# rien installer. Un display virtuel leur suffit, et `gamescope --backend
# headless` en fournit un sans rien afficher chez l'utilisateur.
#
# gamescope ne rend jamais la main (il attend TOUS ses descendants, winedevice
# compris — bug connu, vécu 27/11/2026) : le code retour de l'installateur
# passe donc par un FIFO et gamescope est tué dès qu'il est reçu. La lecture du
# FIFO est bloquante (aucun sleep/poll) et bornée par un timeout.
#
# Usage : run_installer_quiet <nom> <exe> [args...]
# Retour : code de l'installateur (0 = ok), 124 = timeout, 125 = pas de display
run_installer_quiet() {
    local name="$1"; shift
    local limit="${WINCOMPONENTS_INSTALLER_TIMEOUT:-900}"
    local rc=0

    # Session graphique normale : l'installateur trouve son display tout seul
    if [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
        timeout -k 10 "$limit" env WINEPREFIX="$WINEPREFIX" "$WINE_BIN" "$@"
        rc=$?
        if [ "$rc" -ne 0 ]; then
            echo "  ✗ $name : installateur sorti en code $rc"
        fi
        return "$rc"
    fi

    if ! command -v gamescope &>/dev/null; then
        echo "  ✗ $name : aucun display et gamescope absent (installateur à fenêtre)"
        return 125
    fi

    local tmp_dir rc_file gs_pid
    tmp_dir=$(mktemp -d "${SHARED_TMP_DIR:-/tmp}/gwine-display.XXXXXX") || return 1
    chmod 700 "$tmp_dir"
    rc_file="$tmp_dir/rc.fifo"
    mkfifo "$rc_file"

    cat > "$tmp_dir/run.sh" <<'EOS'
#!/bin/bash
# Enfant sous gamescope : exécute l'installateur, publie son code retour sur le
# FIFO (gamescope ne se termine pas seul, c'est le parent qui le tue).
rc_file="$1"; shift
"$@"
rc=$?
printf '%s' "$rc" > "$rc_file" 2>/dev/null
exit "$rc"
EOS
    chmod +x "$tmp_dir/run.sh"

    env XDG_RUNTIME_DIR="$tmp_dir" DISPLAY= WAYLAND_DISPLAY= \
        gamescope --backend headless -W 1280 -H 720 -- \
        "$tmp_dir/run.sh" "$rc_file" env WINEPREFIX="$WINEPREFIX" "$WINE_BIN" "$@" \
        >/dev/null 2>&1 &
    gs_pid=$!

    # Attente bloquante du code retour, plafonnée (le `timeout` borne `cat`)
    rc=$(timeout -k 5 "$((limit + 30))" cat "$rc_file" 2>/dev/null)
    if [ -z "$rc" ]; then
        rc=124
        echo "  ✗ $name : aucun code retour après ${limit}s"
    fi

    # Teardown du display jetable : TERM puis KILL, et par motif pour emporter
    # le reaper (gamescope ignore TERM). Rien ne doit rester vivant.
    kill -TERM "$gs_pid" 2>/dev/null
    kill -KILL "$gs_pid" 2>/dev/null
    pkill -KILL -f "$tmp_dir" 2>/dev/null
    wait "$gs_pid" 2>/dev/null
    rm -rf "$tmp_dir"

    if [ "$rc" -ne 0 ]; then
        echo "  ✗ $name : installateur sorti en code $rc"
    fi
    return "$rc"
}

prepare_wincomponents_cache() {
    ensure_dir "$WINCOMPONENTS_CACHE"
    
    if [ "${OFFLINE_MODE:-false}" = "true" ]; then
        if ! check_wincomponents_cache; then
            return 1
        fi
        return 0
    fi
    
    download_all_wincomponents
}
