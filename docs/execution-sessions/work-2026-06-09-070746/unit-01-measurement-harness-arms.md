---
unit: "T01 — V1.7 measurement harness arms"
unit_number: 1
unit_kind: tracer-bullet
serves: "Slice 1 — the measurement surface every later V1.7 ranking change (qwen embedder, hybrid dense/BM25, reranker) must pass through for honest, attributable comparison."
status: completed
attempt_count: 1
domains: [measurement, retrieval, testing, python]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/01-measurement-harness-arms.md
session_id: work-2026-06-09-070746
---

## What Was Implemented

Extended the existing REAL-server retrieval-quality harness so every report attributes its numbers to a specific V1.7 retrieval "arm" (backend, embedder model, dense/sparse/rerank flags) plus per-query HTTP latency — without changing any production default retrieval behavior.

- `scripts/retrieval_quality_live.py`:
  - `ARM_METADATA_DEFAULTS` — current production arm identity (`backend=snapshot_dense`, `embedder_model=nomic-embed-text`, `dense=True`, `sparse=False`, `rerank=False`).
  - `build_arm_metadata(env_overrides=None)` — reads arm identity from env vars mirroring `RetrievalConfig::from_env` (`OLLAMA_EMBED_MODEL`, `RETRIEVAL_BACKEND`, `RETRIEVAL_SPARSE`, `RETRIEVAL_RERANK`), falling back to honest production defaults where the env name does not yet exist server-side.
  - `_latency_stats()` — mean/p50/p95/n over real per-query latencies captured with `time.monotonic()` around the live `urlopen` HTTP call.
  - Report dict now carries `arm` and `latency_ms`; console prints both.
- `scripts/retrieval_quality_sweep.py`:
  - Arm env keys cleared between configs; `("v1.7-baseline", {})` reference arm added to `CONFIGS`; T02/T04/T07 arm rows left as honest commented placeholders.
  - Arm + latency threaded through each per-config result and the summary JSON; winner selection excludes `v1.7-*` label rows.
- `tests/e2e/quality/test_arm_metadata.py` — 10 unit tests (arm field/defaults/env-override flow-through + report-shape: arm block, six IR metrics, latency block).

## Files Changed
- `scripts/retrieval_quality_live.py` — modified (arm metadata + latency capture + report wiring)
- `scripts/retrieval_quality_sweep.py` — modified (arm/latency threading, v1.7-baseline row, winner filter)
- `tests/e2e/quality/test_arm_metadata.py` — created (10 unit tests)
- `tests/e2e/reports/v17-baseline__held_out.json` — generated live report (arm + latency + six metrics)

## Problems Encountered
None. Implementation matched the architecture handoff on the first attempt.

## Patterns Discovered
- `OLLAMA_EMBED_MODEL` is NOT yet read by the mcp-server — `build_embedding_service()` in `crates/mcp-server/src/lib.rs` hardcodes `nomic-embed-text`. Reading it harness-side with the production default as fallback is the correct forward-compatible approach; T02 wires the server side.
- `beta_heavy`/`alpha_heavy` configs HURT (MRR 0.525 / 0.429 vs 0.662 default): shifting weight off embedding similarity toward subunit/graph signal degrades ranking. Default weighting is near-optimal for the current snapshot-dense backend; the real headroom is the backend itself (T04 hybrid), not the eq.3 weights.
- `v1.7-baseline` reproduced `default` exactly (same server/weights/no overrides), confirming the winner-exclusion filter works.

## TDD Evidence
- **Red**
  - Command: `python3 -m pytest tests/e2e/quality/test_arm_metadata.py -v` (before implementation)
  - Result: FAIL — `ImportError: cannot import name 'build_arm_metadata' from 'retrieval_quality_live'`
  - Evidence: arm-metadata behavior was absent before the change (import-level absence of the new contract, not setup noise).
- **Green**
  - Command: `python3 -m pytest tests/e2e/quality/test_arm_metadata.py -v` (after implementation)
  - Result: PASS (10/10)
  - Evidence: default arm production values, env-override flow-through for all four env vars, and report shape (arm block + six metrics + latency block) all verified.
- **Post-Refactor Green**
  - Command: `python3 -m pytest tests/e2e/quality/test_arm_metadata.py -v`
  - Result: PASS (10/10). No structural refactor was needed; rerun confirms no regression.
- **E2E (real mcp-server, cold-boot full sweep — user-approved)**
  - Command: `python3 scripts/retrieval_quality_sweep.py` then gated held-out baseline
  - Result: PASS, gate exit = 0
  - Winner held-out: MRR=0.767, nDCG@3=0.756, no_match_precision=1.0, p95≈99ms (regression floor MRR≥0.60 + no_match≥0.90 met; 0.80/0.80 aspiration honestly UNMET and logged, not faked green).
  - Live report `tests/e2e/reports/v17-baseline__held_out.json` verified on disk: `arm={backend:snapshot_dense, embedder_model:nomic-embed-text, dense:true, sparse:false, rerank:false}`, `latency_ms={mean:123.1, p50:61.9, p95:109.6, n:30}`, judge_augmented has mrr/ndcg_at_3/hit_at_3/recall_at_3/p_at_1.

## Test Results
- Unit command: `python3 -m pytest tests/e2e/quality/test_arm_metadata.py -q` → 10 passed (re-run by orchestrator: 10 passed in 0.03s)
- E2E command: full real-server sweep → gate exit 0
- Attempts: 1
