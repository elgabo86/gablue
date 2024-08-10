#!/usr/bin/env bash

# Tell this script to exit if there are any errors.
# You should have this in every custom script, to ensure that your completed
# builds actually ran successfully without any errors!
set -oue pipefail

rpm-ostree cliwrap install-to-root / && \

rpm-ostree override replace --experimental \
        /tmp/kernel-rpms/kernel-[0-9]*.rpm \
        /tmp/kernel-rpms/kernel-core-*.rpm \
        /tmp/kernel-rpms/kernel-modules-*.rpm

wget https://copr.fedorainfracloud.org/coprs/sentry/kernel-fsync/repo/fedora-40/sentry-kernel-fsync-fedora-40.repo -O /etc/yum.repos.d/kernel-fsync.repo

rpm-ostree override replace --experimental --from repo='copr:copr.fedorainfracloud.org:sentry:kernel-fsync' kernel-tools kernel-tools-libs kernel-headers

curl -Lo /tmp/nvidia-install.sh https://raw.githubusercontent.com/ublue-os/hwe/main/nvidia-install.sh && \
chmod +x /tmp/nvidia-install.sh && \
IMAGE_NAME="kinoite-main" FEDORA_MAJOR_VERSION="40" /tmp/nvidia-install.sh
rm -f /usr/share/vulkan/icd.d/nouveau_icd.*.json
