#!/usr/bin/bash

set -eux

sleep 15

echo add | exec sudo tee /sys/devices/virtual/input/input*/event*/uevent

while [ -n "$(kdotool search Cage)" ]; do
    sleep 5
done

while [ -n "$(kdotool search Waydroid)" ]; do
    sleep 5
done

waydroid session stop
sleep 2
rm -f ~/.local/share/applications/*aydroid*
