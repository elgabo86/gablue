#!/usr/bin/bash

set -eoux pipefail

rpm-ostree override remove \
    mesa-va-drivers \
    pipewire-libs-extra && \
rpm-ostree override replace \
    --experimental \
    --from repo=copr:copr.fedorainfracloud.org:kylegospo:bazzite-multilib \
        mesa-filesystem \
        mesa-libxatracker \
        mesa-libglapi \
        mesa-dri-drivers \
        mesa-libgbm \
        mesa-libEGL \
        mesa-vulkan-drivers \
        mesa-libGL \
        pipewire \
        pipewire-alsa \
        pipewire-gstreamer \
        pipewire-jack-audio-connection-kit \
        pipewire-jack-audio-connection-kit-libs \
        pipewire-libs \
        pipewire-pulseaudio \
        pipewire-utils \
        pipewire-plugin-libcamera \
        bluez \
        bluez-obexd \
        bluez-cups \
        bluez-libs \
        xorg-x11-server-Xwayland && \
rpm-ostree install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$OS_VERSION.noarch.rpm https://mirrors.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-$OS_VERSION.noarch.rpm && \
rpm-ostree install \
        mesa-va-drivers-freeworld \
        mesa-vdpau-drivers-freeworld.x86_64 \
        libaacs \
        libbdplus \
        libbluray
