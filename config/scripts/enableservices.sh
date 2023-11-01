#!/usr/bin/env bash

# Tell this script to exit if there are any errors.
# You should have this in every custom script, to ensure that your completed
# builds actually ran successfully without any errors!
set -oue pipefail

systemctl enable sshd.service
systemctl enable tailscaled.service

firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent
firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent
