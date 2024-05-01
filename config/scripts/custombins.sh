#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -ouex pipefail

wget https://github.com/aandrew-me/tgpt/releases/latest/download/tgpt-linux-amd64 -O /usr/bin/tgpt
chmod +x /usr/bin/tgpt
