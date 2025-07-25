#!/usr/bin/bash

flatpak override --user --reset
flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
flatpak override --user --filesystem=xdg-config/MangoHud:ro
flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
flatpak override --user --filesystem=xdg-config/lsfg-vk:ro
flatpak override --user --env=OBS_VKCAPTURE=1
flatpak override --user --filesystem=/run/media
flatpak override --user --filesystem=/media
flatpak override --user --filesystem=xdg-download
flatpak override com.usebottles.bottles --user --filesystem=xdg-data/applications
flatpak override com.usebottles.bottles --user --env=DXVK_ASYNC=1
flatpak override com.usebottles.bottles --user --env=DXVK_GPLASYNCCACHE=1
flatpak override com.usebottles.bottles --user --env=WINENTSYNC=1
flatpak override com.usebottles.bottles --user --filesystem=~/Windows
flatpak override com.usebottles.bottles --user --filesystem=/tmp
flatpak override org.mozilla.firefox --user --filesystem=/run/udev:ro
flatpak override app.zen_browser.zen --user --filesystem=/run/udev:ro
flatpak override com.valvesoftware.Steam  --user --env=MANGOHUD=1

flatpak override io.github.hedge_dev.hedgemodmanager --user --filesystem=~/.var/app/io.github.hedge_dev.unleashedrecomp:ro
