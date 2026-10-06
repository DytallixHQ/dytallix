#!/bin/sh
# host_install.py switch (host setup v1, release switch): run as root from this
# host's bundle for the new release.
set -eu
cd "$(dirname "$0")"
exec python3 -I -B host_install.py switch
