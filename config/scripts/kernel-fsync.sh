#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -oue pipefail



# Install kernel-fsync
wget https://copr.fedorainfracloud.org/coprs/sentry/kernel-fsync/repo/fedora-39/sentry-kernel-fsync-fedora-39.repo -O /etc/yum.repos.d/kernel-fsync.repo
rpm-ostree override replace --experimental --freeze --from repo='copr:copr.fedorainfracloud.org:sentry:kernel-fsync' kernel kernel-core kernel-modules kernel-modules-core kernel-modules-extra
rm /etc/yum.repos.d/kernel-fsync.repo

