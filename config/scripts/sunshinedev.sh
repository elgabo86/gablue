#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -oue pipefail

rpm-ostree install https://github.com/LizardByte/Sunshine/releases/download/v0.22.2/sunshine-fedora-39-amd64.rpm
