#!/bin/bash

################################################################################
# wineserver-manager.sh - Redirection vers modules wineserver/
################################################################################

_WINESERVER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/wineserver" && pwd)"

source "$_WINESERVER_DIR/init.sh"
source "$_WINESERVER_DIR/master.sh"
source "$_WINESERVER_DIR/counter.sh"
source "$_WINESERVER_DIR/server.sh"
source "$_WINESERVER_DIR/cleanup.sh"
