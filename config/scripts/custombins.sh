#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -oue pipefail

wget https://github.com/aandrew-me/tgpt/releases/latest/download/tgpt-linux-amd64 -O /usr/bin/tgpt
