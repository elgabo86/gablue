#!/usr/bin/bash

set -ouex pipefail

# Branding for Gablue
sed -i '/^PRETTY_NAME/s/Kinoite/Gablue/' /usr/lib/os-release
