#!/bin/bash

################################################################################
# mode-init.sh - Redirection vers les modules d'initialisation
################################################################################

_MODE_INIT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

source "$_MODE_INIT_DIR/modes/init-main.sh"
source "$_MODE_INIT_DIR/modes/init-ensure.sh"
