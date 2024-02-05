#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -oue pipefail

rpm-ostree install https://github.com/LizardByte/Sunshine/releases/download/nightly-dev/sunshine-fedora-39-amd64.rpm
