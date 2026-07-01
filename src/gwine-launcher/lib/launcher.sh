#!/bin/bash

################################################################################
# launcher.sh - Redirection vers les modules de lancement
################################################################################

_LAUNCHER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

source "$_LAUNCHER_DIR/launcher-utils.sh"
source "$_LAUNCHER_DIR/launcher-main.sh"
