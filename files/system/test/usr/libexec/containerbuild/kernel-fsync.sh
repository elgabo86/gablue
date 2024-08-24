#!/usr/bin/bash

set -eoux pipefail

rpm-ostree cliwrap install-to-root / && \
rpm-ostree override replace --experimental \
    /tmp/fsync-rpms/kernel-[0-9]*.rpm \
    /tmp/fsync-rpms/kernel-core-*.rpm \
    /tmp/fsync-rpms/kernel-modules-*.rpm
