#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -ouex pipefail

wget -O /usr/lib/udev/rules.d/60-openrgb.rules https://gitlab.com/CalcProgrammer1/OpenRGB/-/jobs/artifacts/master/raw/60-openrgb.rules?job=Linux+64+AppImage&inline=false
