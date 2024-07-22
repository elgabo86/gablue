#!/usr/bin/bash

set -ouex pipefail

sed -i 's/#UserspaceHID=true/UserspaceHID=false/' /etc/bluetooth/input.conf
