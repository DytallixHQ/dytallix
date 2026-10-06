#!/bin/sh
# Removes the Dytallix node from this host (host setup v1): run as root, on a
# staging install, before the production install. It asks for confirmation.
set -eu
cd "$(dirname "$0")"
exec python3 -I -B host_install.py wipe
