#!/bin/bash

################################################################################
# vcrun.sh - Installation de Visual C++ Redistributables
################################################################################

# DLL natives à poser par version, MFC comprise. Wine n'implémente ni mfc100*,
# ni mfc110*, ni mfc120*, ni mfc140* : une DLL tierce qui les importe (ex.
# data/CChromaEditorLibrary.dll d'un jeu GameMaker) ne charge pas et le jeu
# meurt en « Error defining an external function » (vécu 27/11/2026).
_vcrun_dll_list() {
    local version="$1" list="" prefix="" loc
    case "$version" in
        2010) list="atl100 mfc100 mfc100u mfcm100 mfcm100u msvcp100 msvcr100 vcomp100"
              prefix="mfc100" ;;
        2012) list="atl110 mfc110 mfc110u mfcm110 mfcm110u msvcp110 msvcr110 vcomp110"
              prefix="mfc110" ;;
        2013) list="atl120 mfc120 mfc120u mfcm120 mfcm120u msvcp120 msvcr120 vcomp120"
              prefix="mfc120" ;;
        2022) list="concrt140 mfc140 mfc140u mfcm140 mfcm140u msvcp140 msvcp140_1
msvcp140_2 msvcp140_atomic_wait msvcp140_codecvt_ids vcamp140 vccorlib140
vcomp140 vcruntime140 vcruntime140_threads vcruntime140_1"
              prefix="mfc140" ;;
    esac
    # DLL de ressources MFC localisées, embarquées dans les mêmes cabs
    for loc in chs cht deu enu esn fra ita jpn kor rus; do
        list="$list $prefix$loc"
    done
    echo "$list"
}

_vcrun_sentinels() {
    case "$1" in
        2010) echo "msvcp100 mfc100 mfc100u" ;;
        2012) echo "msvcp110 mfc110 mfc110u" ;;
        2013) echo "msvcp120 mfc120 mfc120u" ;;
        2022) echo "msvcp140 mfc140 mfc140u" ;;
    esac
}

# Extrait les DLL d'un redistributable au lieu de lancer son installateur :
#   2010      SFX InstallShield → vc_red.cab, DLL F_CENTRAL_<dll>_x86
#   2012/2013 bundle WiX → cabs aN, DLL F_CENTRAL_<dll>_x86 / _x64
#   2022      bundle WiX → cabs aN, DLL <dll>.dll_x86 / <dll>.dll_amd64
# Les installateurs Microsoft (Burn/WiX) créent une fenêtre MÊME avec /quiet :
# sous DISPLAY vidé (init headless) ils sortent « Failed to create window »
# (0x80070578) sans rien installer, et l'échec était masqué par
# `2>/dev/null || true` → préfixe reconstruit sans aucune DLL native (msvcp140
# restait celui de Wine), régression muette jusqu'au jeu suivant. L'extraction
# ne demande aucun display et se contrôle sur des sentinelles.
_install_vcrun_from_cabs() {
    local version="$1" bits="$2" cache_file="$3" dest="$4"

    if ! command -v cabextract &>/dev/null; then
        echo "  ✗ redist $version $bits : cabextract absent"
        return 1
    fi

    local tmp_dir
    tmp_dir=$(mktemp -d)
    mkdir -p "$tmp_dir/payload" "$tmp_dir/out"
    cabextract -q -d "$tmp_dir/payload" "$cache_file" >/dev/null 2>&1

    # La charge utile change de forme selon la version : un cab unique
    # (vc_red.cab, 2010) ou des cabs numérotés a0, a1, a10… (bundles WiX).
    local inner
    for inner in "$tmp_dir"/payload/*.cab "$tmp_dir"/payload/a*; do
        [ -f "$inner" ] || continue
        cabextract -q -d "$tmp_dir/out" "$inner" >/dev/null 2>&1 || true
    done

    local dll_list f base norm copied=0
    dll_list=$(_vcrun_dll_list "$version")
    for f in "$tmp_dir"/out/*; do
        [ -f "$f" ] || continue
        base=$(basename "$f")
        # Suffixe d'architecture du redist → nom de DLL normalisé
        case "$base" in
            *_x86)   [ "$bits" = "x86" ] || continue; norm="${base%_x86}" ;;
            *_x64)   [ "$bits" = "x64" ] || continue; norm="${base%_x64}" ;;
            *_amd64) [ "$bits" = "x64" ] || continue; norm="${base%_amd64}" ;;
            *) continue ;;
        esac
        norm="${norm%.dll}"
        norm="${norm#F_CENTRAL_}"
        case " $dll_list " in
            *" $norm "*) cp -f "$f" "$dest/$norm.dll" && copied=$((copied + 1)) ;;
        esac
    done
    rm -rf "$tmp_dir"

    local sentinel missing=""
    for sentinel in $(_vcrun_sentinels "$version"); do
        [ -f "$dest/$sentinel.dll" ] || missing="$missing $sentinel"
    done
    if [ "$copied" -eq 0 ] || [ -n "$missing" ]; then
        echo "  ✗ redist $version $bits : DLL natives manquantes :$missing"
        return 1
    fi
    return 0
}

install_vcrun() {
    local version="$1"
    local arch="${2:-both}"
    
    echo "Installation de Visual C++ $version..."
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    local win64_sys="$WINEPREFIX/drive_c/windows/system32"
    
    ensure_dir -s "$win32_sys"
    [ "$WINEARCH" = "win64" ] && ensure_dir -s "$win64_sys"
    
    local component_name cache_file
    if [ "$arch" = "x86" ] || [ "$arch" = "both" ]; then
        component_name="vcrun${version}_x86"
        cache_file="$WINCOMPONENTS_CACHE/$component_name/$(basename "${COMPONENT_URLS[$component_name]}")"
        
        if [ -f "$cache_file" ]; then
            echo "  - Extraction des DLL VC++ $version x86..."
            _install_vcrun_from_cabs "$version" x86 "$cache_file" "$win32_sys" \
                || WINCOMPONENTS_FAILED+=("$component_name")
        fi
    fi
    
    if [ "$WINEARCH" = "win64" ] && { [ "$arch" = "x64" ] || [ "$arch" = "both" ]; }; then
        component_name="vcrun${version}_x64"
        cache_file="$WINCOMPONENTS_CACHE/$component_name/$(basename "${COMPONENT_URLS[$component_name]}")"
        
        if [ -f "$cache_file" ]; then
            echo "  - Extraction des DLL VC++ $version x64..."
            _install_vcrun_from_cabs "$version" x64 "$cache_file" "$win64_sys" \
                || WINCOMPONENTS_FAILED+=("$component_name")
        fi
    fi
    
    local vcrun_dlls=""
    local vcrun_dlls_x64=""
    case "$version" in
        2010) vcrun_dlls="msvcp100 msvcr100 vcomp100 atl100 mfc100 mfc100u mfcm100 mfcm100u" ;;
        2012) vcrun_dlls="msvcp110 msvcr110 vcomp110 atl110 mfc110 mfc110u mfcm110 mfcm110u" ;;
        2013) vcrun_dlls="msvcp120 msvcr120 vcomp120 atl120 mfc120 mfc120u mfcm120 mfcm120u" ;;
        2022) vcrun_dlls="concrt140 msvcp140 msvcp140_1 msvcp140_2 msvcp140_atomic_wait msvcp140_codecvt_ids vcamp140 vccorlib140 vcomp140 vcruntime140 mfc140 mfc140u mfcm140 mfcm140u"
              vcrun_dlls_x64="vcruntime140_1" ;;
    esac
    
    for dll in $vcrun_dlls; do
        WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*$dll" /d native,builtin /f >/dev/null 2>&1 || true
    done
    
    if [ "$WINEARCH" = "win64" ] && [ -n "$vcrun_dlls_x64" ]; then
        for dll in $vcrun_dlls_x64; do
            WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*$dll" /d native,builtin /f >/dev/null 2>&1 || true
        done
    fi
    
    return 0
}

install_vcrun6() {
    echo "Installation de VCRUN6 (MFC42)..."
    
    local win32_sys="$WINEPREFIX/drive_c/windows/syswow64"
    local cache_file="$WINCOMPONENTS_CACHE/vcrun6/VC6RedistSetup_deu.exe"
    
    if [ ! -f "$cache_file" ]; then
        echo "  ✗ Fichier VCRUN6 non trouvé"
        return 1
    fi
    
    local tmp_dir=$(mktemp -d)
    local vcredist="$WINCOMPONENTS_CACHE/vcrun6/vcredist.exe"
    
    if [ ! -f "$vcredist" ]; then
        if command -v cabextract &>/dev/null; then
            cabextract -d "$tmp_dir" "$cache_file" >/dev/null 2>&1 || true
            if [ -f "$tmp_dir/vcredist.exe" ]; then
                cp "$tmp_dir/vcredist.exe" "$vcredist" 2>/dev/null || true
            fi
        fi
    fi
    
    if [ -f "$vcredist" ] && command -v cabextract &>/dev/null; then
        cabextract -d "$tmp_dir" "$vcredist" >/dev/null 2>&1 || true
        
        for dll in mfc42.dll mfc42u.dll msvcirt.dll; do
            if [ -f "$tmp_dir/$dll" ]; then
                cp -f "$tmp_dir/$dll" "$win32_sys/" 2>/dev/null || true
            fi
        done
    fi
    
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*mfc42" /d native,builtin /f >/dev/null 2>&1 || true
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*mfc42u" /d native,builtin /f >/dev/null 2>&1 || true
    WINEPREFIX="$WINEPREFIX" "$WINE_BIN" reg add 'HKEY_CURRENT_USER\Software\Wine\DllOverrides' /v "*msvcirt" /d native,builtin /f >/dev/null 2>&1 || true
    
    rm -rf "$tmp_dir"
    echo "  ✓ VCRUN6 (MFC42) installé"
    return 0
}
