#!/usr/bin/env bash

# Tell this script to exit if there are any errors.
# You should have this in every custom script, to ensure that your completed
# builds actually ran successfully without any errors!
set -ouex pipefail

curl -Lo /tmp/nvidia-install.sh https://raw.githubusercontent.com/ublue-os/hwe/main/nvidia-install.sh && \
chmod +x /tmp/nvidia-install.sh && \
IMAGE_NAME="kinoite" FEDORA_MAJOR_VERSION="40" /tmp/nvidia-install.sh
rm -f /usr/share/vulkan/icd.d/nouveau_icd.*.json

# cleanup
shopt -s extglob
rm -rf /tmp/akmods-rpms/* || true
rm -rf /var/!(cache)
rm -rf /var/cache/!(rpm-ostree)
