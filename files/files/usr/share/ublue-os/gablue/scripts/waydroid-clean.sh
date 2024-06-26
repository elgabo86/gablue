#!/usr/bin/bash

set -eux

waydroid session stop
sleep 2
rm -f ~/.local/share/applications/*aydroid*
update-desktop-database ~/.local/share/applications
