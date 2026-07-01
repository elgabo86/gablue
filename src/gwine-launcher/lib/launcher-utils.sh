#!/bin/bash

################################################################################
# launcher-utils.sh - Redirection vers modules launcher-utils/
################################################################################

_LAUNCHER_UTILS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/launcher-utils" && pwd)"

source "$_LAUNCHER_UTILS_DIR/env.sh"
source "$_LAUNCHER_UTILS_DIR/registry.sh"
source "$_LAUNCHER_UTILS_DIR/pds.sh"
source "$_LAUNCHER_UTILS_DIR/paths.sh"
source "$_LAUNCHER_UTILS_DIR/conflict.sh"
source "$_LAUNCHER_UTILS_DIR/setup.sh"
