#!/usr/bin/env bash

# Tell build process to exit if there are any errors.
set -oue pipefail

rpm-ostree override replace https://kojipkgs.fedoraproject.org//packages/bluez/5.68/2.fc39/x86_64/bluez-5.68-2.fc39.x86_64.rpm https://kojipkgs.fedoraproject.org//packages/bluez/5.68/2.fc39/x86_64/bluez-libs-5.68-2.fc39.x86_64.rpm https://kojipkgs.fedoraproject.org//packages/bluez/5.68/2.fc39/x86_64/bluez-cups-5.68-2.fc39.x86_64.rpm
