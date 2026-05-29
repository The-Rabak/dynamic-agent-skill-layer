#!/usr/bin/env bash
set -euo pipefail

INCLUDE_DREAM=0
SKIP_INFRA=0

for arg in "$@"; do
  case "$arg" in
    --include-dream)
      INCLUDE_DREAM=1
      ;;
    --skip-infra)
      SKIP_INFRA=1
      ;;
    *)
      echo "Unknown option: $arg" >&2
      echo "Usage: $0 [--include-dream] [--skip-infra]" >&2
      exit 1
      ;;
  esac
done

COMPOSE_FILE="docker-compose.test.yml"

cleanup_infra() {
  docker compose --ansi never -f "${COMPOSE_FILE}" down --remove-orphans >/dev/null 2>&1 || true
}

if [[ "${SKIP_INFRA}" -eq 0 ]]; then
  trap cleanup_infra EXIT

  echo "==> Starting infrastructure test stack"
  docker compose --ansi never -f "${COMPOSE_FILE}" down --remove-orphans >/dev/null 2>&1 || true
  docker compose --ansi never -f "${COMPOSE_FILE}" up -d postgres redis qdrant ollama

  echo "==> Verifying topology"
  docker compose --ansi never -f "${COMPOSE_FILE}" run --rm --no-deps topology-check >/dev/null

  echo "==> Running infrastructure/container E2E checks"
  ./scripts/run-t02-infrastructure-tests.sh

  echo "==> Running real-infrastructure Rust E2E tests (graph-builder -> PG + Qdrant)"
  export DATABASE_URL="postgres://skill_layer:skill_layer@localhost:15432/skill_layer"
  export QDRANT_URL="http://localhost:16333"
  cargo test -p graph-builder --test test_real_infrastructure_e2e

  echo "==> Running maintenance real-infrastructure E2E tests"
  cargo test -p maintenance --test test_maintenance_e2e
fi

echo "==> Running realistic MCP E2E tests"
cargo test -p mcp-server \
  --test test_compile_context \
  --test test_dual_scope \
  --test test_extract_session \
  --test test_live_data_plane_roundtrip \
  --test test_concurrency_stress

echo "==> Running realistic graph-builder E2E tests"
cargo test -p graph-builder \
  --test test_watcher_rebuild \
  --test test_watcher_churn_reconciliation

echo "==> Validating dream-state contract tests compile and register"
cargo test -p mcp-server --test test_dream_state_contract

if [[ "${INCLUDE_DREAM}" -eq 1 ]]; then
  echo "==> Executing ignored dream-state contracts"
  cargo test -p mcp-server --test test_dream_state_contract -- --ignored
fi

echo "==> All selected E2E suites completed"