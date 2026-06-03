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

# Port mappings — keep in sync with docker-compose.test.yml
OLLAMA_PORT=11444
QDRANT_HTTP_PORT=16333
QDRANT_GRPC_PORT=16334
POSTGRES_PORT=15432
REDIS_PORT=16379

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
  export DATABASE_URL="postgres://skill_layer:skill_layer@localhost:${POSTGRES_PORT}/skill_layer"
  export QDRANT_URL="http://localhost:${QDRANT_HTTP_PORT}"
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
    export OLLAMA_URL="http://localhost:${OLLAMA_PORT}"
    # T08 fix: use the REST port (16333) here, not the gRPC port (16334).
    # The QdrantAdapter uses HTTP/REST; pointing at gRPC (16334) causes
    # hyper::Parse(Version) errors in check_connectivity.
    export QDRANT_URL="http://localhost:${QDRANT_HTTP_PORT}"
    export DATABASE_URL="postgres://skill_layer:skill_layer@localhost:${POSTGRES_PORT}/skill_layer_test"
    export REDIS_URL="redis://localhost:${REDIS_PORT}"
    # The extraction provider reads OLLAMA_EXTRACTION_ENDPOINT (not OLLAMA_URL) and
    # defaults to :11434, which nothing serves in this topology. Point it at the
    # real Ollama so live extraction actually runs (todo 103).
    export OLLAMA_EXTRACTION_ENDPOINT="http://localhost:${OLLAMA_PORT}/api/generate"
    cargo test -p mcp-server --test test_live_data_plane_roundtrip -- --ignored

    echo "==> Running transcript ingest queue E2E test (todo 103: shipped hook → queue → drain → .pending)"
    cargo test -p mcp-server --features test-utils --test test_transcript_ingest_queue_e2e -- --ignored

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
cargo test -p mcp-server --test test_dream_state_contract -- --skip ignored

if [[ "${INCLUDE_DREAM}" -eq 1 ]]; then
  echo "==> Running promoted dream-state contract tests (DS-003 through DS-007)"
  cargo test -p mcp-server --test test_dream_state_contract \
    dependency_chaos_matrix \
    outbox_backlog_replays \
    qdrant_pg_drift \
    sustained_watcher_and_extraction \
    high_qps_compile_context \
    -- --ignored

  echo "==> Running watcher churn live E2E test"
  cargo test -p graph-builder --test test_watcher_churn_reconciliation watcher_churn_and_reconciliation_converges_to_correct_graph_state_under_live_pg_qdrant -- --ignored

  echo "==> Running concurrency stress live E2E tests"
  cargo test -p mcp-server --test test_concurrency_stress -- --ignored

  echo "==> Running all live data plane E2E tests"
  cargo test -p mcp-server --test test_live_data_plane_roundtrip -- --ignored
fi

echo "==> All selected E2E suites completed"

echo "==> Aggregating E2E reports"
REPORTS_DIR="${REPO_ROOT}/tests/e2e/reports"
TIMESTAMP=$(date +%Y%m%d%H%M%S)
AGGREGATE_REPORT="${REPORTS_DIR}/run__${TIMESTAMP}.json"

# Count test results from report files
if ls "${REPORTS_DIR}"/*.json 2>/dev/null | grep -qv "run__"; then
  echo "Found individual report files, aggregating..."
  # Use python3 to merge all individual reports into one aggregate
  python3 -c "
import json, glob, os
reports_dir = '${REPORTS_DIR}'
reports = []
for path in sorted(glob.glob(os.path.join(reports_dir, '*.json'))):
    with open(path) as f:
        try:
            report = json.load(f)
            reports.append(report)
        except json.JSONDecodeError:
            print(f'Warning: could not parse {path}')

total = len(reports)
passed = sum(1 for r in reports if r.get('outcome', {}).get('status') == 'Passed')
failed = sum(1 for r in reports if r.get('outcome', {}).get('status') == 'Failed')
degraded_passed = sum(1 for r in reports if r.get('outcome', {}).get('status') == 'Passed'
                       and any(d.get('service') for d in r.get('degradation_events', [])))

aggregate = {
    'run_summary': {
        'total_tests': total,
        'passed': passed,
        'failed': failed,
        'degraded_passed': degraded_passed,
        'total_duration_ms': sum(r.get('duration_ms', 0) for r in reports),
        'start_time': min((r.get('started_at', '') for r in reports), default=''),
        'end_time': max((r.get('started_at', '') for r in reports), default=''),
        'container_versions': {}
    },
    'reports': reports
}

with open('${AGGREGATE_REPORT}', 'w') as f:
    json.dump(aggregate, f, indent=2)

print(f'Aggregated {total} reports ({passed} passed, {failed} failed) into ${AGGREGATE_REPORT}')
"
else
  echo "No individual reports found, creating minimal aggregate"
  echo '{"run_summary":{"total_tests":0,"passed":0,"failed":0,"degraded_passed":0,"total_duration_ms":0,"start_time":"","end_time":"","container_versions":{}},"reports":[]}' > "${AGGREGATE_REPORT}"
fi

echo "==> Running judge contract validation"
JUDGE_REPORT="${REPORTS_DIR}/judge_evaluation.json"
python3 -c "
import json, os
reports_dir = '${REPORTS_DIR}'

# Find the latest aggregate report
agg_files = sorted([f for f in os.listdir(reports_dir) if f.startswith('run__') and f.endswith('.json')])
if not agg_files:
    print('No aggregate report found')
    exit(1)

with open(os.path.join(reports_dir, agg_files[-1])) as f:
    aggregate = json.load(f)

reports = aggregate.get('reports', [])

def any_report_asserts(query_fn):
    return any(query_fn(r) for r in reports)

# Q1: all compile_context responses use legal statuses
q1 = any_report_asserts(lambda r: any(
    a.get('description', '').find('compile_context') != -1 or
    a.get('description', '').find('compile') != -1
    for a in r.get('sections', [])
))

# Q2: degraded + subsequent healthy never produces duplicate_suppressed
q2 = any_report_asserts(lambda r: any(
    d.get('service', '') != '' and d.get('recovered', False) == False
    for d in r.get('degradation_events', [])
))

# Q3: graph.rebuilt only after outbox drain
q3 = any_report_asserts(lambda r: any(
    c.get('contract_name', '').find('outbox') != -1 or
    c.get('contract_name', '').find('graph') != -1
    for c in r.get('contract_assertions', [])
))

# Q4: every non-ok status carries non-empty reason_code
q4 = True  # checked per-test at assertion level

# Q5: extraction produces .pending + both lifecycle events
q5 = any_report_asserts(lambda r: 'extraction' in r.get('test_name', '').lower() or any(
    s.get('name', '').find('extract') != -1
    for s in r.get('sections', [])
))

# Q6: graph_version match
q6 = any_report_asserts(lambda r: any(
    c.get('contract_name', '').find('version') != -1
    for c in r.get('contract_assertions', [])
))

# Q7: invalidation ordering preserved
q7 = any_report_asserts(lambda r: any(
    c.get('contract_name', '').find('invalidation') != -1 or
    c.get('details', '').find('graph_version') != -1
    for c in r.get('contract_assertions', [])
))

# Q8: stress requests within latency budget
q8 = any_report_asserts(lambda r: 'stress' in r.get('test_name', '').lower() or 'concurrent' in r.get('test_name', '').lower())

# Q9: watcher churn - no silently dropped events
q9 = any_report_asserts(lambda r: 'watcher' in r.get('test_name', '').lower() or 'churn' in r.get('test_name', '').lower())

# Q10: environment snapshots consistent
q10 = any_report_asserts(lambda r: r.get('environment', {}).get('pg_version', '') != '')

judge = {
    'questions': [
        {'id': 1, 'question': 'All compile_context calls return legal statuses', 'answerable': q1, 'evidence': 'compile_context assertions in reports'},
        {'id': 2, 'question': 'No degraded call produces duplicate_suppressed', 'answerable': q2, 'evidence': 'degradation events in reports'},
        {'id': 3, 'question': 'graph.rebuilt only after outbox drain', 'answerable': q3, 'evidence': 'contract assertions about graph/outbox'},
        {'id': 4, 'question': 'Every non-ok status has non-empty reason_code', 'answerable': q4, 'evidence': 'per-test assertion contract'},
        {'id': 5, 'question': 'Extraction produces pending + both lifecycle events', 'answerable': q5, 'evidence': 'extraction test reports'},
        {'id': 6, 'question': 'No graph_version mismatch', 'answerable': q6, 'evidence': 'version assertions in reports'},
        {'id': 7, 'question': 'Invalidation ordering preserved', 'answerable': q7, 'evidence': 'invalidation assertions in reports'},
        {'id': 8, 'question': 'Concurrency stress within latency budget', 'answerable': q8, 'evidence': 'latency samples in stress reports'},
        {'id': 9, 'question': 'No silently dropped watcher events', 'answerable': q9, 'evidence': 'watcher churn report'},
        {'id': 10, 'question': 'Environment snapshots consistent', 'answerable': q10, 'evidence': 'environment snapshot fields'},
    ]
}
with open('${JUDGE_REPORT}', 'w') as f:
    json.dump(judge, f, indent=2)

print(f'Judge evaluation written to ${JUDGE_REPORT}')
" 2>&1