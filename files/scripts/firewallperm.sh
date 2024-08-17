#!/usr/bin/bash

set -ouex pipefail

# Open ports for shared networks
firewall-cmd --add-port=1025-65535/tcp --zone=nm-shared --permanent
firewall-cmd --add-port=1025-65535/udp --zone=nm-shared --permanent
