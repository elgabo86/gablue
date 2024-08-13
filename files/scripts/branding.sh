#!/usr/bin/bash

set -ouex pipefail

# Branding for Gablue
IMAGE_DATE=$(date +%Y%m%d.%H)
MAJOR_RELEASE_VERSION=$(grep -oP '[0-9]*' /etc/fedora-release)
sed -i "s,^PRETTY_NAME=.*,PRETTY_NAME=\"Gablue ${MAJOR_RELEASE_VERSION}.${IMAGE_DATE}\"," /usr/lib/os-release


# Remove ublue update notification
sed -i 's/dbus_notify = true/dbus_notify = false/g' /etc/ublue-update/ublue-update.toml
