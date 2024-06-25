#!/usr/bin/bash

set -eux

/usr/share/ublue-os/gablue/scripts/waydroid-killer.sh &

#cage -- bash -uxc "wlr-randr --output WL-1 --custom-mode "1280x720" && waydroid show-full-ui"
cage -- waydroid show-full-ui
