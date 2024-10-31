#!/usr/bin/bash

set -eoux pipefail

# Make executable which need it
chmod +x /usr/bin/system-flatpak-setup
chmod +x /usr/libexec/gablue-update
chmod +x /usr/libexec/archgab-update
chmod +x /usr/libexec/windows-update

chmod +x /usr/share/ublue-os/gablue/scripts/*.sh
chmod +x /usr/share/ublue-os/gablue/scripts/gamepadshortcuts/*


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

# Disable repos
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/_copr_che-nerd-fonts.repo && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/_copr:copr.fedorainfracloud.org:phracek:PyCharm.repo && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/_copr_kylegospo-bazzite.repo && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/_copr_matte-schwartz-sunshine.repo && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/_copr_sramanujam-atuin.repo && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/_copr_ublue-os-akmods.repo && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/tailscale.repo && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/ublue-os-staging-fedora.repo && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/zinix01-linux-wifi-hotspot.repo && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/negativo17-fedora-rar.repo && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/negativo17-fedora-multimedia.repo

# Remove some unused .Desktop
rm -f /usr/share/applications/kde4/knetattach.desktop
rm -f /usr/share/applications/org.kde.kdebugsettings.desktop
rm -f /usr/share/applications/org.gnome.Meld.desktop
rm -f /usr/share/applications/org.kde.kdeconnect.sms.desktop
rm -f /usr/share/applications/byobu.desktop
rm -f /usr/share/applications/org.kde.plasma-welcome.desktop
rm -f /usr/share/applications/btop.desktop
rm -f /usr/share/applications/htop.desktop
rm -f /usr/share/applications/nvtop.desktop
rm -f /usr/share/applications/firewall-config.desktop
rm -f /usr/share/applications/org.kde.kjournaldbrowser.desktop
