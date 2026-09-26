#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="${ROOT_DIR}/logs"
PID_FILE="${LOG_DIR}/local-node.pid"

if [[ ! -f "${PID_FILE}" ]]; then
  echo "No local node pid file found at ${PID_FILE}."
  exit 0
fi

node_pid="$(cat "${PID_FILE}" 2>/dev/null || true)"
if [[ -z "${node_pid}" ]]; then
  rm -f "${PID_FILE}"
  echo "Removed empty pid file."
  exit 0
fi

if ! kill -0 "${node_pid}" 2>/dev/null; then
  rm -f "${PID_FILE}"
  echo "Process ${node_pid} is not running. Removed stale pid file."
  exit 0
fi

kill "${node_pid}" 2>/dev/null || true
for _ in $(seq 1 10); do
  if ! kill -0 "${node_pid}" 2>/dev/null; then
    rm -f "${PID_FILE}"
    echo "Stopped local node (pid ${node_pid})."
    exit 0
  fi
  sleep 1
done

kill -9 "${node_pid}" 2>/dev/null || true
rm -f "${PID_FILE}"
echo "Force-stopped local node (pid ${node_pid})."
