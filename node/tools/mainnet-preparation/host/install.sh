#!/bin/sh
# Installs this Dytallix host bundle (host setup v1). Run as root at the host
# console, after checking the bundle's SHA-256:
#   tar -xf LABEL.bundle.tar && ./dytallix-host/install.sh
set -eu
cd "$(dirname "$0")"
exec python3 -I -B host_install.py install
