#!/bin/bash

################################################################################
# dotnet.sh - Installation de .NET Desktop Runtime
################################################################################

install_dotnet() {
    local version="$1"
    local arch="${2:-both}"

    echo "Installation de .NET Desktop Runtime $version..."

    # L'installateur windowsdesktop-runtime est un bundle WiX/Burn : il crée une
    # fenêtre même avec /quiet et échoue sans display (0x80070578, init
    # headless). run_installer_quiet lui fournit un display virtuel jetable.
    local component_name cache_file
    if [ "$arch" = "x86" ] || [ "$arch" = "both" ]; then
        component_name="dotnetdesktop${version}_x86"
        cache_file="$WINCOMPONENTS_CACHE/$component_name/$(basename "${COMPONENT_URLS[$component_name]}")"

        if [ -f "$cache_file" ]; then
            echo "  - Installation .NET $version x86..."
            run_installer_quiet ".NET $version x86" "$cache_file" /quiet /norestart \
                || WINCOMPONENTS_FAILED+=("$component_name")
        fi
    fi

    if [ "$WINEARCH" = "win64" ] && { [ "$arch" = "x64" ] || [ "$arch" = "both" ]; }; then
        component_name="dotnetdesktop${version}_x64"
        cache_file="$WINCOMPONENTS_CACHE/$component_name/$(basename "${COMPONENT_URLS[$component_name]}")"

        if [ -f "$cache_file" ]; then
            echo "  - Installation .NET $version x64..."
            run_installer_quiet ".NET $version x64" "$cache_file" /quiet /norestart \
                || WINCOMPONENTS_FAILED+=("$component_name")
        fi
    fi

    return 0
}
