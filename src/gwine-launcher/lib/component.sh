#!/bin/bash

################################################################################
# component.sh - Redirection vers les modules components
################################################################################

_COMPONENT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

source "$_COMPONENT_DIR/components/utils.sh"
source "$_COMPONENT_DIR/components/dxvk.sh"
source "$_COMPONENT_DIR/components/mono.sh"
source "$_COMPONENT_DIR/components/nvapi.sh"
source "$_COMPONENT_DIR/components/winetricks.sh"
