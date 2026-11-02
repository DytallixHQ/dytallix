#!/bin/sh
# Installs or replaces this host's monitor settings (monitoring v1): run as
# root with the settings file from your own media. The URLs never enter the
# bundle, and changing them does not touch the node keys. FILE may be
# relative to where this is run from.
set -eu
exec python3 -I -B "$(dirname "$0")/host_install.py" monitor-settings "$1"
