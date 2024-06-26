#!/usr/bin/bash

set -eux

sleep 5


while [ -n "$(kdotool search Cage)" ]; do
    sleep 30
done

while [ -n "$(kdotool search Waydroid)" ]; do
    sleep 30
done

waydroid session stop
sleep 2
rm -f ~/.local/share/applications/*aydroid*
update-desktop-database ~/.local/share/applications
