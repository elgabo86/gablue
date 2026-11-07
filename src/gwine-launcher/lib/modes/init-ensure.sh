#!/bin/bash

################################################################################
# init-ensure.sh - Fonctions de vérification/création du préfixe
#
# Progression publiée par `progress_step` (ui.sh) : lisible en terminal, et
# reprise par Gablue Manager quand gwine tourne sous sa fenêtre de progression.
################################################################################

ensure_wineprefix() {
    if [ ! -d "$WINEPREFIX" ] || [ ! -f "$WINEPREFIX/system.reg" ]; then
        echo "Préfixe Wine non trouvé, création automatique..."

        local TOTAL_STEPS=3

        progress_step 1 "$TOTAL_STEPS" "Préparation des composants"
        prepare_local_cache

        progress_step 2 "$TOTAL_STEPS" "Création du préfixe Wine"
        if ! wineboot_init_prefix; then
            echo "Erreur: échec de l'initialisation du préfixe Wine" >&2
            return 1
        fi

        progress_step 3 "$TOTAL_STEPS" "Installation de Wine Mono et Gecko"
        if ! install_wine_mono_gecko; then
            echo "Attention: échec de l'installation de Mono/Gecko" >&2
        fi

        progress_done "Préfixe Wine prêt"
    fi
}

ensure_wineprefix_full() {
    if [ ! -d "$WINEPREFIX" ] || [ ! -f "$WINEPREFIX/system.reg" ]; then
        echo "Préfixe Wine non trouvé, création automatique..."

        local TOTAL_STEPS=6
        if is_nvidia_gpu; then
            TOTAL_STEPS=7
        fi
        local CURRENT_STEP=0

        ((CURRENT_STEP++))
        progress_step "$CURRENT_STEP" "$TOTAL_STEPS" "Préparation des composants"
        prepare_local_cache

        ((CURRENT_STEP++))
        progress_step "$CURRENT_STEP" "$TOTAL_STEPS" "Création du préfixe Wine"
        if ! wineboot_init_prefix; then
            echo "Erreur: échec de l'initialisation du préfixe Wine" >&2
            return 1
        fi

        ((CURRENT_STEP++))
        progress_step "$CURRENT_STEP" "$TOTAL_STEPS" "Installation de Wine Mono et Gecko"
        if ! install_wine_mono_gecko; then
            echo "Attention: échec de l'installation de Mono/Gecko" >&2
        fi

        ((CURRENT_STEP++))
        progress_step "$CURRENT_STEP" "$TOTAL_STEPS" "Installation des composants Windows"
        install_winetricks_components

        ((CURRENT_STEP++))
        progress_step "$CURRENT_STEP" "$TOTAL_STEPS" "Installation de DXVK/VKD3D/D7VK"
        install_dxvk_vkd3d
        install_d7vk

        if is_nvidia_gpu; then
            ((CURRENT_STEP++))
            progress_step "$CURRENT_STEP" "$TOTAL_STEPS" "Installation de DXVK-NVAPI (NVIDIA)"

            local latest_nvapi
            latest_nvapi=$(get_latest_dxvk_nvapi_version)
            local current_nvapi=""
            local nvapi_folder
            nvapi_folder=$(find "$DXVK_NVAPI_CACHE_DIR" -maxdepth 1 -type d -name "dxvk-nvapi-*" 2>/dev/null | sort -V | tail -1)
            [ -n "$nvapi_folder" ] && current_nvapi=$(basename "$nvapi_folder" | sed 's/^dxvk-nvapi-v//; s/^dxvk-nvapi-//')

            if [ -z "$current_nvapi" ] || { [ -n "$latest_nvapi" ] && compare_versions "$latest_nvapi" "$current_nvapi"; }; then
                download_dxvk_nvapi --no-confirm 2>/dev/null || true
            fi

            install_dxvk_nvapi 2>/dev/null || true
        fi

        progress_done "Préfixe Wine prêt"

        echo "Préfixe créé avec succès"
    fi
}
