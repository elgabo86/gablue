#!/bin/bash

set -eou pipefail

XBOX_CONFIG_FILE="$HOME/.local/share/gwine/options"

get_current_state() {
    if [ -f "$XBOX_CONFIG_FILE" ]; then
        local val
        val=$(grep "^xbox_default=" "$XBOX_CONFIG_FILE" 2>/dev/null | cut -d'=' -f2)
        if [ "$val" = "on" ]; then
            echo "on"
            return 0
        fi
    fi
    echo "off"
}

get_current_filter() {
    if [ -f "$XBOX_CONFIG_FILE" ]; then
        local val
        val=$(grep "^xbox_filter=" "$XBOX_CONFIG_FILE" 2>/dev/null | cut -d'=' -f2)
        if [ "$val" = "ds4" ] || [ "$val" = "dualsense" ]; then
            echo "$val"
            return 0
        fi
    fi
    echo "all"
}

current_state=$(get_current_state)
current_filter=$(get_current_filter)

if [ "$current_state" = "on" ]; then
    filter_text=""
    case "$current_filter" in
        ds4)        filter_text="DualShock 4 uniquement" ;;
        dualsense)  filter_text="DualSense uniquement" ;;
        *)          filter_text="Tous (DS4+DualSense)" ;;
    esac

    kdialog --yesno "Le mode Xbox est actuellement ACTIVé ($filter_text).\n\nVoulez-vous le désactiver ?" --title "Mode Xbox" --yes-label "Désactiver" --no-label "Annuler"

    if [ $? -eq 0 ]; then
        /usr/bin/gwine --xbox-off
        kdialog --msgbox "Mode Xbox désactivé." --title "Mode Xbox"
    fi
else
    choice=$(kdialog --menu "Le mode Xbox est actuellement DÉSACTIVÉ.\n\nChoisissez le filtre pour l'activation :" \
        "all" "Tous (DS4+DualSense)" \
        "ds4" "DualShock 4 uniquement" \
        "dualsense" "DualSense uniquement" \
        --title "Mode Xbox")

    if [ $? -eq 0 ] && [ -n "$choice" ]; then
        if [ "$choice" = "all" ]; then
            /usr/bin/gwine --xbox-on
        else
            /usr/bin/gwine --xbox-on --xbox-"$choice"
        fi
        filter_text=""
        case "$choice" in
            ds4)        filter_text="DualShock 4 uniquement" ;;
            dualsense)  filter_text="DualSense uniquement" ;;
            *)          filter_text="Tous (DS4+DualSense)" ;;
        esac
        kdialog --msgbox "Mode Xbox activé ($filter_text)." --title "Mode Xbox"
    fi
fi
