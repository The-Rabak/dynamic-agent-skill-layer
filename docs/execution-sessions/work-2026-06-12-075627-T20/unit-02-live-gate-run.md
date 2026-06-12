---
unit: "T20 Unit 2 — live gate run + α=0 canary crater + real latency artifact"
unit_number: 2
unit_kind: infra-packet
serves: "GREEN evidence: the promoted validated instrument passes as a gate on the live 262 stack; the α=0 alignment canary craters; a real latency artifact sources the T11 p95 claim."
status: completed
attempt_count: 1
domains: [measurement, e2e, live-infra, retrieval]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/20-institutionalize-262-instrument-e2e-gate.md
session_id: work-2026-06-12-075627-T20
---

## Orchestrator-driven (measurement drives the real app)
Per the standing rule, the live gate measurement was driven by the orchestrator against the REAL running mcp-server (:3001) on the live 262 qwen3 corpus — not delegated, not reconstructed in-process.

## Pre-run fix (no-fakes): real latency instrumentation
Unit A's `--gate` emitted a PLACEHOLDER latency artifact (a note deferring real timing to "Unit B", which is just the same script). That is a fake. Before running, the orchestrator instrumented genuine timing in `retrieval_sweep.py`:
- `_compute_arm_metrics`: wall-clock `time.perf_counter()` around the live `find_skill` HTTP round-trip at `mrr_limit`, stored as per-query `latency_ms`.
- `_run_gate`: the latency artifact now computes real mean/p50/p95/p99/min/max from the measured samples and fails loud (`sys.exit(2)`) if no latency samples exist (an empty latency artifact is not a pass). Added `import math` for nearest-rank percentiles.

## Live gate run (`t20-gate-20260612-081155`)
`python3 scripts/retrieval_sweep.py --gate --run-id t20-gate-20260612-081155` (split=all, 137 pos + 25 neg, 262 fixture). Two arms, each via `reboot_mcp` + /health-200 readiness gate (T17 honesty):

| arm | MRR@3 | MRR@10 | nDCG@3 | cand-recall@50 | no_match |
|---|---|---|---|---|---|
| dense_views_on | 0.743 | 0.743 | 0.755 | 0.796 | 0.92 |
| alpha0_control | 0.000 | 0.000 | 0.000 | 0.000 | 1.0 |

- dense_views_on reproduces T11 §2 EXACTLY. All above floors (0.64/0.64/0.64/0.68/0.88).
- alpha0_control: 100% MRR crater → alignment canary fires (≥50% required).
- **GATE: PASS** (all 6 assertions; `gate_t20-gate-20260612-081155.json`).
- Latency (`latency_t20-gate-20260612-081155.json`, 137 queries, find_skill_limit=10): mean 282.7ms, p50 266.4ms, **p95 375.3ms**, p99 421.4ms, min 219.4, max 527.3 — real wall-clock; sources the T11 §3 369ms claim (375 within run-to-run noise); < 500ms SLO.

## Mandatory restore (operational gotcha)
The gate's LAST arm is `alpha0_control` (RETRIEVAL_ALPHA=0.0), and `_run_gate` does NOT restore the server. Left as-is, the live stack serves crippled (zeroed dense signal) retrieval. The orchestrator recreated mcp-server with default env (RETRIEVAL_ALPHA/DENSE_VIEWS unset via `docker compose -f docker-compose.test.yml up -d --no-deps --force-recreate mcp-server`), polled /health 200, and verified a real query: `prohibit-concurrent-cargo-ops-across-agents` 0.749 / `wsl2-crash-dirty-file-recovery` 0.679. Stack healthy + default again.

## Patterns Discovered
- `--gate` (and the sweep generally) leaves the server in the LAST arm's env. Any live α=0 / backend-changing run MUST be followed by an explicit restore-to-default reboot. Consider adding a `finally`-restore to `_run_gate` in a future hardening pass (logged, not done here — scope fence).
- The promoted instrument reproduces T11 to the digit on the live stack — strong evidence the validated ruler is now the gate (no re-validation drift).

## Test Results
- Command: `python3 scripts/retrieval_sweep.py --gate --run-id t20-gate-20260612-081155`
- Result: GATE: PASS (exit 0); α=0 cratered 100%; real latency artifact persisted.
- Attempts: 1
