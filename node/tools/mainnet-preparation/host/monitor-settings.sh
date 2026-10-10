#!/bin/sh
# Installs or replaces this host's monitor settings (monitoring v1): run as
# root with the settings file from your own media. The URLs never enter the
# bundle, and changing them does not touch the node keys.
set -eu
cd "$(dirname "$0")"
exec python3 -I -B host_install.py monitor-settings "$1"
