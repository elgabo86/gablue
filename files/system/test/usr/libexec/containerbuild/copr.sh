#!/usr/bin/bash

set -eoux pipefail

curl -Lo /usr/bin/copr https://raw.githubusercontent.com/ublue-os/COPR-command/main/copr && \
chmod +x /usr/bin/copr && \
curl -Lo /etc/yum.repos.d/_copr_kylegospo-bazzite.repo https://copr.fedorainfracloud.org/coprs/kylegospo/bazzite/repo/fedora-"${FEDORA_MAJOR_VERSION}"/kylegospo-bazzite-fedora-"${FEDORA_MAJOR_VERSION}".repo && \
curl -Lo /etc/yum.repos.d/_copr_matte-schwartz-sunshine.repo https://copr.fedorainfracloud.org/coprs/matte-schwartz/sunshine/repo/fedora-"${FEDORA_MAJOR_VERSION}"/matte-schwartz-sunshine-fedora-"${FEDORA_MAJOR_VERSION}".repo && \
curl -Lo /etc/yum.repos.d/_copr_sramanujam-atuin.repo https://copr.fedorainfracloud.org/coprs/sramanujam/atuin/repo/fedora-"${FEDORA_MAJOR_VERSION}"/sramanujam-atuin-fedora-"${FEDORA_MAJOR_VERSION}".repo  && \
curl -Lo /etc/yum.repos.d/_copr_luisbocanegra-kde-material-you-colors.repo https://copr.fedorainfracloud.org/coprs/luisbocanegra/kde-material-you-colors/repo/fedora-"${FEDORA_MAJOR_VERSION}"/luisbocanegra-kde-material-you-colors-fedora-"${FEDORA_MAJOR_VERSION}".repo && \
curl -Lo /etc/yum.repos.d/tailscale.repo https://pkgs.tailscale.com/stable/fedora/tailscale.repo && \
sed -i 's@gpgcheck=1@gpgcheck=0@g' /etc/yum.repos.d/tailscale.repo
