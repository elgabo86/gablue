#!/usr/bin/bash

set -eoux pipefail

rpm-ostree cliwrap install-to-root / && \
rpm-ostree override replace --experimental \
    /tmp/kernel-rpms/kernel-[0-9]*.rpm \
    /tmp/kernel-rpms/kernel-core-*.rpm \
    /tmp/kernel-rpms/kernel-modules-*.rpm
