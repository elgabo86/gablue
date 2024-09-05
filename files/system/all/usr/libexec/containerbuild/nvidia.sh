#!/usr/bin/env bash

# Tell this script to exit if there are any errors.
# You should have this in every custom script, to ensure that your completed
# builds actually ran successfully without any errors!
set -ouex pipefail

rpm-ostree install \
        mesa-libEGL.i686

curl -Lo /tmp/nvidia-install.sh https://raw.githubusercontent.com/ublue-os/hwe/main/nvidia-install.sh && \
chmod +x /tmp/nvidia-install.sh && \
IMAGE_NAME="${SOURCE_IMAGE}" FEDORA_MAJOR_VERSION="${FEDORA_MAJOR_VERSION}" /tmp/nvidia-install.sh
rm -f /usr/share/vulkan/icd.d/nouveau_icd.*.json
ln -s libnvidia-ml.so.1 /usr/lib64/libnvidia-ml.so
