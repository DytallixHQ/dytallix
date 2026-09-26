#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_DIR="${ROOT_DIR}/logs"
PID_FILE="${LOG_DIR}/local-node.pid"
LOG_FILE="${LOG_DIR}/local-node.log"

find_node_repo() {
  if [[ -n "${DYTALLIX_NODE_DIR:-}" ]] && [[ -d "${DYTALLIX_NODE_DIR}" ]]; then
    printf '%s\n' "${DYTALLIX_NODE_DIR}"
    return 0
  fi

  local candidates=(
    "${ROOT_DIR}/../dytallix-node"
    "${ROOT_DIR}/dytallix-node"
    "$(pwd)/../dytallix-node"
    "$(pwd)/dytallix-node"
  )

  local candidate
  for candidate in "${candidates[@]}"; do
    if [[ -d "${candidate}" ]] && [[ -f "${candidate}/Cargo.toml" ]]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done

  return 1
}

if [[ -f "${PID_FILE}" ]]; then
  running_pid="$(cat "${PID_FILE}" 2>/dev/null || true)"
  if [[ -n "${running_pid}" ]] && kill -0 "${running_pid}" 2>/dev/null; then
    echo "Local node is already running (pid ${running_pid})."
    echo "RPC:  http://localhost:3030"
    echo "Logs: ${LOG_FILE}"
    exit 0
  fi
  rm -f "${PID_FILE}"
fi

NODE_REPO="$(find_node_repo || true)"
if [[ -z "${NODE_REPO}" ]]; then
  echo "Unable to locate the dytallix-node repository." >&2
  echo "Set DYTALLIX_NODE_DIR=/absolute/path/to/dytallix-node and retry." >&2
  exit 1
fi

mkdir -p "${LOG_DIR}"
touch "${LOG_FILE}"

(
  cd "${NODE_REPO}"
  nohup cargo run -p dytallix-fast-node --bin dytallix-fast-node --release >>"${LOG_FILE}" 2>&1 &
  echo $! > "${PID_FILE}"
)

node_pid="$(cat "${PID_FILE}")"

for _ in $(seq 1 30); do
  if curl -fsS -m 2 "http://localhost:3030/status" >/dev/null 2>&1; then
    echo "Local node started (pid ${node_pid})."
    echo "RPC:  http://localhost:3030"
    echo "Logs: ${LOG_FILE}"
    echo "Use: dytallix config network local"
    exit 0
  fi
  sleep 1
done

echo "Local node process started (pid ${node_pid}), but /status did not respond within 30s." >&2
echo "Check logs: ${LOG_FILE}" >&2
