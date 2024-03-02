#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -oue pipefail

# Install Explicit Sync Patches on Nvidia builds
wget https://copr.fedorainfracloud.org/coprs/gloriouseggroll/nvidia-explicit-sync/repo/fedora-$(rpm -E %fedora)/gloriouseggroll-nvidia-explicit-sync-fedora-$(rpm -E %fedora).repo?arch=x86_64 -O /etc/yum.repos.d/_copr_gloriouseggroll-nvidia-explicit-sync.repo
rpm-ostree override replace --experimental --from repo=copr:copr.fedorainfracloud.org:gloriouseggroll:nvidia-explicit-sync xorg-x11-server-Xwayland
rpm-ostree override replace --experimental --from repo=copr:copr.fedorainfracloud.org:gloriouseggroll:nvidia-explicit-sync egl-wayland
rm /etc/yum.repos.d/_copr_gloriouseggroll-nvidia-explicit-sync.repo
