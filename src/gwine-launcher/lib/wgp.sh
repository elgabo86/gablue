#!/bin/bash

################################################################################
# wgp.sh - Redirection vers les modules WGP
################################################################################

_WGP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

source "$_WGP_DIR/wgp/core.sh"
source "$_WGP_DIR/wgp/mount.sh"
source "$_WGP_DIR/wgp/overlay.sh"
source "$_WGP_DIR/wgp/symlinks.sh"
source "$_WGP_DIR/wgp/modes.sh"
