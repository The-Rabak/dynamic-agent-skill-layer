#!/usr/bin/env bash
set -euo pipefail

INCLUDE_DREAM=0
SKIP_INFRA=0
SKIP_LIVE=0

for arg in "$@"; do
  case "$arg" in
    --include-dream)
      INCLUDE_DREAM=1
      ;;
    --skip-infra)
      SKIP_INFRA=1
      ;;
    --skip-live)
      SKIP_LIVE=1
      ;;
    *)
      echo "Unknown option: $arg" >&2
      echo "Usage: $0 [--include-dream] [--skip-infra] [--skip-live]" >&2
      exit 1
      ;;
  esac
done

COMPOSE_FILE="docker-compose.test.yml"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

cleanup_all() {
  docker compose --ansi never -f "${REPO_ROOT}/${COMPOSE_FILE}" down --remove-orphans >/dev/null 2>&1 || true
}

if [[ "${SKIP_INFRA}" -eq 0 ]]; then
  trap cleanup_all EXIT

  echo "==> Starting infrastructure test stack"
  docker compose --ansi never -f "${REPO_ROOT}/${COMPOSE_FILE}" down --remove-orphans >/dev/null 2>&1 || true
  docker compose --ansi never -f "${REPO_ROOT}/${COMPOSE_FILE}" up -d postgres redis qdrant ollama

  echo "==> Verifying topology"
  docker compose --ansi never -f "${REPO_ROOT}/${COMPOSE_FILE}" run --rm --no-deps topology-check >/dev/null

  echo "==> Running infrastructure/container E2E checks"
  ./scripts/run-t02-infrastructure-tests.sh

  echo "==> Running real-infrastructure Rust E2E tests (graph-builder -> PG + Qdrant)"
  export DATABASE_URL="postgres://skill_layer:skill_layer@localhost:15432/skill_layer"
  export QDRANT_URL="http://localhost:16333"
  cargo test -p graph-builder --test test_real_infrastructure_e2e

  echo "==> Running maintenance real-infrastructure E2E tests"
  cargo test -p maintenance --test test_maintenance_e2e

  if [[ "${SKIP_LIVE}" -eq 0 ]]; then
    echo "==> Building mcp-server and graph-builder test images"
    docker compose --ansi never -f "${REPO_ROOT}/${COMPOSE_FILE}" build mcp-server graph-builder

    echo "==> Seeding test fixture volumes"
    mkdir -p "${REPO_ROOT}/tests/fixtures/test-skills/global"
    docker compose --ansi never -f "${REPO_ROOT}/${COMPOSE_FILE}" up -d mcp-server graph-builder

    echo "==> Verifying live service topology"
    docker compose --ansi never -f "${REPO_ROOT}/${COMPOSE_FILE}" run --rm --no-deps live-e2e-check

    echo "==> Running live data plane roundtrip E2E test"
    export OLLAMA_URL="http://localhost:11444"
    export QDRANT_URL="http://localhost:16334"
    export DATABASE_URL="postgres://skill_layer:skill_layer@localhost:15432/skill_layer_test"
    export REDIS_URL="redis://localhost:16379"
    cargo test -p mcp-server --test test_live_data_plane_roundtrip -- --ignored

    echo "==> Tearing down service containers"
    docker compose --ansi never -f "${REPO_ROOT}/${COMPOSE_FILE}" rm -sf mcp-server graph-builder
  fi
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