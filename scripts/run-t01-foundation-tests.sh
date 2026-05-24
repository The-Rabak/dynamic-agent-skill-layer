#!/usr/bin/env bash
set -euo pipefail

COMPOSE_FILE="docker-compose.test.yml"

cleanup() {
  docker compose --ansi never -f "${COMPOSE_FILE}" down --remove-orphans >/dev/null 2>&1 || true
}

trap cleanup EXIT

echo "==> Running workspace tests"
cargo test --workspace

echo "==> Running compose topology test"
docker compose --ansi never -f "${COMPOSE_FILE}" up --abort-on-container-exit
