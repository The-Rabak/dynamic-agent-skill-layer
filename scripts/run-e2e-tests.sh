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

if [[ "${SKIP_INFRA}" -eq 0 ]]; then
  echo "==> Running infrastructure/container E2E checks"
  ./scripts/run-t02-infrastructure-tests.sh
fi

if [[ "${INCLUDE_DREAM}" -eq 1 ]]; then
  echo "==> Executing ignored dream-state contracts (expected to fail until fully implemented)"
  cargo test -p mcp-server --test test_dream_state_contract -- --ignored
fi

echo "==> All selected E2E suites completed"
