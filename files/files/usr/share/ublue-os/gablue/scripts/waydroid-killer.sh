#!/usr/bin/bash

set -eux

sleep 5


while [ -n "$(kdotool search Cage)" ]; do
    sleep 5
done

while [ -n "$(kdotool search Waydroid)" ]; do
    sleep 5
done

waydroid session stop
sleep 2
rm -f ~/.local/share/applications/*aydroid*
