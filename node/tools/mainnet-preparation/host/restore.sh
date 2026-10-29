#!/bin/sh
# host_install.py restore (disaster recovery v1, R5): run as root from this
# host's bundle on a freshly installed host, before its first start, with the
# off-host copy's manifest.json and parts downloaded into one folder:
#   restore.sh FOLDER [--accept-history-loss]
set -eu
exec python3 -I -B "$(dirname "$0")/host_install.py" restore "$@"
