#!/usr/bin/bash

set -eoux pipefail


systemctl enable -f system-flatpak-setup.service
systemctl enable -f sunshine-workaround.service
systemctl enable -f tailscaled.service
systemctl enable -f gamescope-workaround.service
systemctl enable -f earlyoom.service

systemctl disable -f scx.service

if [ "$GABLUE_VARIANT" == "main" ]; then
    systemctl enable -f waydroid-workaround.service
    systemctl disable -f waydroid-container.service
fi

if [ "$SOURCE_IMAGE" == "kinoite" ]; then
    systemctl enable -f kde-sysmonitor-workaround.service
fi
