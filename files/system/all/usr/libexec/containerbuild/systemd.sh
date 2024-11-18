#!/usr/bin/bash

set -eoux pipefail


systemctl enable -f system-flatpak-setup.service
systemctl enable -f sunshine-workaround.service
systemctl enable -f tailscaled.service
systemctl enable -f gamescope-workaround.service
systemctl enable -f earlyoom.service

systemctl disable -f scx.service
systemctl disable -f tailscaled.service
systemctl disable -f displaylink.service

if [ "$SOURCE_IMAGE" == "kinoite" ]; then
    systemctl enable -f kde-sysmonitor-workaround.service
    systemctl enable -f usr-share-sddm-themes.mount
fi
