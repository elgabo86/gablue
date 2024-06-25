#!/usr/bin/bash

set -ouex pipefail

# Remove waydroid .desktop
sed -i 's@\[Desktop Entry\]@\[Desktop Entry\]\nNoDisplay=true@g' /usr/share/applications/Waydroid.desktop
