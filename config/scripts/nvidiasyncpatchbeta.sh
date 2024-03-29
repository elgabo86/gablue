#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -oue pipefail

# Install Explicit Sync Patches on Nvidia builds
rpm-ostree override replace --experimental https://download.copr.fedorainfracloud.org/results/gloriouseggroll/nvidia-explicit-sync/fedora-39-x86_64/07116611-xorg-x11-server-Xwayland/xorg-x11-server-Xwayland-23.2.4-5.20240307git64341c4.fc39.x86_64.rpm
rpm-ostree override replace --experimental https://download.copr.fedorainfracloud.org/results/gloriouseggroll/nvidia-explicit-sync/fedora-39-x86_64/07116519-egl-wayland/egl-wayland-1.1.13-2.fc39.x86_64.rpm

