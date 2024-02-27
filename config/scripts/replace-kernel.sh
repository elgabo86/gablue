#!/usr/bin/env bash

# Tell this script to exit if there are any errors.
set -oue pipefail

# Install kernel-fsync
wget https://copr.fedorainfracloud.org/coprs/sentry/kernel-fsync/repo/fedora-$(rpm -E %fedora)/sentry-kernel-fsync-fedora-$(rpm -E %fedora).repo -O /etc/yum.repos.d/_copr_sentry-kernel-fsync.repo && \
rpm-ostree cliwrap install-to-root / && \
rpm-ostree override replace \
--experimental \
--from repo=copr:copr.fedorainfracloud.org:sentry:kernel-fsync \
    kernel \
    kernel-core \
    kernel-modules \
    kernel-modules-core \
    kernel-modules-extra \
    kernel-uki-virt
