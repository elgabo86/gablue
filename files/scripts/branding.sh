#!/usr/bin/bash

set -ouex pipefail

# Branding for Gablue
IMAGE_DATE=$(date +%Y%m%d.%H)
MAJOR_RELEASE_VERSION=$(grep -oP '[0-9]*' /etc/fedora-release)
sed -i "s,^PRETTY_NAME=.*,PRETTY_NAME=\"Gablue ${MAJOR_RELEASE_VERSION}.${IMAGE_DATE}\"," /usr/lib/os-release

sed -i 's/<default>start-here-kde-symbolic<\/default>/<default>start-here<\/default>/g' /usr/share/plasma/plasmoids/org.kde.plasma.kickoff/contents/config/main.xml

sed -i 's/const defaultIconName = "start-here-kde-symbolic";/const defaultIconName = "start-here";/' /usr/share/plasma/plasmoids/org.kde.plasma.kickoff/contents/ui/code/tools.js

ln -sf ../../../hicolor/scalable/places/start-here.svg /usr/share/icons/Papirus/16x16/panel/start-here.svg
ln -sf ../../../hicolor/scalable/places/start-here.svg /usr/share/icons/Papirus/22x22/panel/start-here.svg
ln -sf ../../../hicolor/scalable/places/start-here.svg /usr/share/icons/Papirus/24x24/panel/start-here.svg
ln -sf ../../../hicolor/scalable/places/start-here.svg /usr/share/icons/Papirus/symbolic/places/start-here-symbolic.svg
