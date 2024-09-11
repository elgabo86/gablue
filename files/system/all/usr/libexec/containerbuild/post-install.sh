#!/usr/bin/bash

set -eoux pipefail

# Make executable which need it
chmod +x /usr/bin/system-flatpak-setup
chmod +x /usr/libexec/gablue-update

# Add tgpt bin
wget https://github.com/aandrew-me/tgpt/releases/latest/download/tgpt-linux-amd64 -O /usr/bin/tgpt
chmod +x /usr/bin/tgpt

#Use bore config from CachyOS
curl -Lo /usr/lib/sysctl.d/99-bore-scheduler.conf https://github.com/CachyOS/CachyOS-Settings/raw/master/usr/lib/sysctl.d/99-bore-scheduler.conf

# Branding for Gablue
IMAGE_DATE=$(date +%Y%m%d.%H)
MAJOR_RELEASE_VERSION=$(grep -oP '[0-9]*' /etc/fedora-release)
sed -i "s,^PRETTY_NAME=.*,PRETTY_NAME=\"Gablue ${MAJOR_RELEASE_VERSION}.${IMAGE_DATE}\"," /usr/lib/os-release

if [ "$SOURCE_IMAGE" == "kinoite" ]; then
    sed -i 's/<default>start-here-kde-symbolic<\/default>/<default>start-here<\/default>/g' /usr/share/plasma/plasmoids/org.kde.plasma.kickoff/contents/config/main.xml
    sed -i 's/const defaultIconName = "start-here-kde-symbolic";/const defaultIconName = "start-here";/' /usr/share/plasma/plasmoids/org.kde.plasma.kickoff/contents/ui/code/tools.js
    ln -sf ../../../hicolor/scalable/places/start-here.svg /usr/share/icons/Papirus/16x16/panel/start-here.svg
    ln -sf ../../../hicolor/scalable/places/start-here.svg /usr/share/icons/Papirus/22x22/panel/start-here.svg
    ln -sf ../../../hicolor/scalable/places/start-here.svg /usr/share/icons/Papirus/24x24/panel/start-here.svg
    ln -sf ../../../hicolor/scalable/places/start-here.svg /usr/share/icons/Papirus/symbolic/places/start-here-symbolic.svg
fi

# Hide update & change min battery %
sed -i 's/dbus_notify = true/dbus_notify = false/g' /usr/etc/ublue-update/ublue-update.toml
sed -i 's/min_battery_percent = 50\.0/min_battery_percent = 30.0/g' /usr/etc/ublue-update/ublue-update.toml

# set scx_lavd default
sed -i "s/^SCX_SCHEDULER=.*/SCX_SCHEDULER=scx_lavd/" /etc/default/scx

# set userpace HID to true
sed -i 's/#UserspaceHID.*/UserspaceHID=true/' /etc/bluetooth/input.conf

if [ "$GABLUE_VARIANT" == "main" ]; then
    # Remove waydroid .desktop
    sed -i 's@\[Desktop Entry\]@\[Desktop Entry\]\nNoDisplay=true@g' /usr/share/applications/Waydroid.desktop
fi

# fix qt6 bus errors
if [ "$SOURCE_IMAGE" == "kinoite" ]; then
    ln -s /bin/qdbus /bin/qdbus6
fi

#remove atuin default config
if [ -f /etc/profile.d/atuin.sh ]; then
    # Supprime le fichier
    rm -f /etc/profile.d/atuin.sh
    echo "Le fichier /etc/profile.d/atuin.sh a été supprimé."
else
    echo "Le fichier /etc/profile.d/atuin.sh n'existe pas."
fi

# fix topgrade warning bypass
sed -i '/^ExecStart/a Environment="TOPGRADE_SKIP_BRKC_NOTIFY=true"' /usr/lib/systemd/system/ublue-update.service
echo "TOPGRADE_SKIP_BRKC_NOTIFY=true" >> "/etc/environment"

# alternatives cannot create symlinks on its own during a container build
ln -sf /usr/bin/ld.bfd /etc/alternatives/ld && ln -sf /etc/alternatives/ld /usr/bin/ld

# fix cosmic login issues
if [ "$SOURCE_IMAGE" == "cosmic" ]; then
    grep -E '^greetd:' /usr/etc/passwd | sudo tee -a /etc/passwd
    grep -E '^cosmic-greeter:' /usr/etc/passwd | sudo tee -a /etc/passwd
    grep -E '^greetd:' /usr/etc/group | sudo tee -a /etc/group
    grep -E '^cosmic-greeter:' /usr/etc/group | sudo tee -a /etc/group
fi
