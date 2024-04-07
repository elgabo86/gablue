#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -oue pipefail


sed -i 's/#DefaultLimitNOFILE=/DefaultLimitNOFILE=4096:524288/' /usr/etc/systemd/user.conf && \
sed -i 's/#DefaultTimeoutStopSec.*/DefaultTimeoutStopSec=15s/' /usr/etc/systemd/user.conf && \
sed -i 's/#DefaultTimeoutStopSec.*/DefaultTimeoutStopSec=15s/' /usr/etc/systemd/system.conf
