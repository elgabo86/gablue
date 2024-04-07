#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -oue pipefail


sed -i 's/#DefaultLimitNOFILE=/DefaultLimitNOFILE=4096:524288/' /etc/systemd/user.conf && \
sed -i 's/#DefaultTimeoutStopSec.*/DefaultTimeoutStopSec=15s/' /etc/systemd/user.conf && \
sed -i 's/#DefaultTimeoutStopSec.*/DefaultTimeoutStopSec=15s/' /etc/systemd/system.conf
