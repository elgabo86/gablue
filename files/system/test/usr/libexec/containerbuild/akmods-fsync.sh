#!/usr/bin/bash

set -eoux pipefail

sed -i 's@enabled=0@enabled=1@g' /etc/yum.repos.d/_copr_ublue-os-akmods.repo && \
curl -Lo /etc/yum.repos.d/negativo17-fedora-multimedia.repo https://negativo17.org/repos/fedora-multimedia.repo && \
rpm-ostree install \
    /tmp/akmods-rpms/kmods/*xone*.rpm \
    /tmp/akmods-rpms/kmods/*openrazer*.rpm \
    /tmp/akmods-rpms/kmods/*v4l2loopback*.rpm \
    /tmp/akmods-rpms/kmods/*wl*.rpm \
    /tmp/akmods-extra-rpms/kmods/*evdi*.rpm \
    /tmp/akmods-extra-rpms/kmods/*nct6687*.rpm \
    /tmp/akmods-extra-rpms/kmods/*gcadapter_oc*.rpm \
    /tmp/akmods-extra-rpms/kmods/*bmi260*.rpm && \
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/negativo17-fedora-multimedia.repo
