#!/bin/bash

################################################################################
# component-d7vk.sh - Gestion de D7VK (Direct3D 7/6/5/3 via Vulkan)
################################################################################

# Installe D7VK dans le préfixe Wine depuis le cache local.
# D7VK ne remplace PAS le ddraw d'un jeu : il s'installe au niveau système du
# préfixe (syswow64, les jeux D3D7 sont tous 32-bit) et agit comme un proxy —
# il traduit D3D7/6/5/3 (immediate-mode) vers le backend D3D9 de DXVK embarqué,
# et délègue le vrai DirectDraw à l'implémentation Wine qu'il recharge depuis
# ddraw_.dll. Procédure identique à Proton-CachyOS (PROTON_D7VK_DDRAW) :
#   1. sauvegarde du ddraw builtin Wine -> ddraw_.dll
#   2. ddraw.dll de D7VK -> syswow64
#   3. override ddraw = native,builtin
install_d7vk() {
    echo "Installation de D7VK depuis le cache..."

    if ! get_wine_system_paths; then
        return 1
    fi

    # Sauvegarde du ddraw builtin AVANT écrasement. Uniquement si :
    # - ddraw_.dll n'existe pas déjà (idempotent, jamais re-sauvegarder)
    # - ddraw.dll porte le marqueur "Wine builtin" (ne jamais sauvegarder
    #   un ddraw.dll qui est déjà D7VK — le proxy bouclerait sur lui-même)
    if [ ! -f "$win32_sys_path/ddraw_.dll" ]; then
        if [ -f "$win32_sys_path/ddraw.dll" ] && [ "$(grep -ac 'Wine builtin' "$win32_sys_path/ddraw.dll" 2>/dev/null)" != "0" ]; then
            if ! cp -p "$win32_sys_path/ddraw.dll" "$win32_sys_path/ddraw_.dll"; then
                echo "Erreur: Impossible de sauvegarder le ddraw builtin de Wine"
                return 1
            fi
        else
            echo "Erreur: ddraw builtin de Wine introuvable dans $win32_sys_path (sauvegarde ddraw_.dll impossible)"
            return 1
        fi
    fi

    # Le zip D7VK ne contient que x32/ddraw.dll : copy_dll_files copie le
    # x64 s'il existe (absent ici), le x32 vers syswow64. Le ddraw 64-bit
    # de system32 reste le builtin Wine (aucun jeu D3D7 n'est 64-bit).
    if install_dll_component "D7VK" "$D7VK_CACHE_DIR" "d7vk-*" "ddraw.dll" "ddraw"; then
        echo "D7VK installé avec succès"
        return 0
    fi
    return 1
}

# Aplatit le dossier racine du zip D7VK (d7vk-X.Y.Z/d7vk-vX.Y.Z/x32/... ->
# d7vk-X.Y.Z/x32/...). Le layout attendu par install_dll_component est
# d7vk-X.Y.Z/x32/ddraw.dll. No-op si déjà aplati.
_flatten_d7vk_dir() {
    local d7vk_dir="$1"
    [ -n "$d7vk_dir" ] || return 0
    [ -d "$d7vk_dir/x32" ] && return 0
    local inner_dir
    inner_dir=$(find "$d7vk_dir" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | head -1)
    if [ -n "$inner_dir" ] && [ -d "$inner_dir/x32" ]; then
        mv "$inner_dir"/* "$d7vk_dir/" 2>/dev/null || true
        rmdir "$inner_dir" 2>/dev/null || true
    fi
    return 0
}

# Télécharge D7VK (source unique officielle WinterSnowfall/d7vk)
download_d7vk() {
    local target_version="${1:-}"
    local no_confirm=false

    if [ "$1" = "--no-confirm" ]; then
        no_confirm=true
        target_version=""
    fi

    ensure_dir "$D7VK_CACHE_DIR"

    if [ -z "$target_version" ]; then
        target_version=$(get_latest_d7vk_version)
    fi

    if [ -z "$target_version" ]; then
        echo "Impossible de récupérer la version de D7VK"
        return 1
    fi

    local current_d7vk=""
    local d7vk_folder
    d7vk_folder=$(find_component_dir "$D7VK_CACHE_DIR" "d7vk-*")
    [ -n "$d7vk_folder" ] && current_d7vk=$(basename "$d7vk_folder" | sed 's/^d7vk-//')

    if [ -n "$current_d7vk" ] && [ "$current_d7vk" = "$target_version" ]; then
        # Version cible déjà en cache : réparer au besoin un dossier non aplati
        # (legacy d'un téléchargement antérieur au mécanisme d'aplatissement)
        _flatten_d7vk_dir "$d7vk_folder"
        echo "D7VK $target_version est déjà installé"
        return 0
    fi

    echo "D7VK - Installé: ${current_d7vk:-Aucun}, Cible: $target_version"

    # Asset zip contenant un dossier racine d7vk-vX.Y.Z/ (layout x32/ à
    # l'intérieur) — contrairement aux tarballs DXVK sans dossier racine.
    local d7vk_tag="v${target_version}"
    local d7vk_url="https://github.com/WinterSnowfall/d7vk/releases/download/${d7vk_tag}/d7vk-${d7vk_tag}.zip"

    _do_download() {
        download_github_component "$D7VK_CACHE_DIR" "d7vk" "$target_version" "$d7vk_url" "zip" "$no_confirm"
    }

    if update_component_with_backup "$D7VK_CACHE_DIR" "d7vk-*" _do_download; then
        # Aplatir le dossier racine du zip : le layout attendu par
        # install_dll_component est d7vk-X.Y.Z/x32/ddraw.dll
        _flatten_d7vk_dir "$(find_component_dir "$D7VK_CACHE_DIR" "d7vk-*")"
        echo "✓ D7VK $target_version installé"
        return 0
    else
        echo "✗ Échec de l'installation de D7VK"
        return 1
    fi
}
