#!/bin/bash

################################################################################
# wincomponents.sh - Redirection vers les modules de composants Windows
################################################################################

_WINCOMPONENTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

source "$_WINCOMPONENTS_DIR/wincomponents/defs.sh"
source "$_WINCOMPONENTS_DIR/wincomponents/utils.sh"
source "$_WINCOMPONENTS_DIR/wincomponents/vcrun.sh"
source "$_WINCOMPONENTS_DIR/wincomponents/dotnet.sh"
source "$_WINCOMPONENTS_DIR/wincomponents/directx.sh"
source "$_WINCOMPONENTS_DIR/wincomponents/fonts.sh"
source "$_WINCOMPONENTS_DIR/wincomponents/misc.sh"
source "$_WINCOMPONENTS_DIR/wincomponents/wmp9.sh"
source "$_WINCOMPONENTS_DIR/wincomponents/main.sh"
