---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/01-measurement-harness-arms.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "## Execution Slices > Slice 1"
brainstorm_ref: null
started: 2026-06-09T04:07:46Z
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-06-09-070746
batch: 1
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: Build the V1.7 measurement surface that all later ranking changes must pass through, so current default retrieval can be honestly compared against qwen/hybrid/rerank arms without changing production defaults.
- Success-criteria focus: Reports identify the current default arm and each V1.7 experimental arm; reports include backend, embedder model, dense/sparse/rerank flags, latency, MRR, nDCG, hit@3, recall@3, no-match precision; no new fake retrieval path; validation drives the real running mcp-server.

### TDD Contract
- Effective mode: Ralph-driven TDD (plan overrides local)
- Effective loop: Failing tests/assertions first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: Unit (harness assertion that arm metadata + new metrics are recorded) + E2E (real running mcp-server over HTTP, cold-boot full sweep — user-approved 2026-06-09)
- Exceptions: None

### Constitution Context
- Version: 2.1.0; waivers: none.
- T01 only extends Python measurement scripts + reports. It does NOT change ranking behavior, models, schema, event contracts, or infra config — so no human-gate/approval trigger fires.
- STANDING RULE preserved: all retrieval quality measurement drives the REAL running mcp-server over HTTP; NO in-process reconstruction of retrieval/scope-fusion/scoring (memory: measurement-drives-real-app-no-in-process-reconstruction; #210 lesson).
- STANDING RULE: no stubs/fakes/placeholders in production or non-unit test paths — fail loud.

### Architecture Handoff
- Artifact: plan-derived handoff (no separate architecture artifact; parent plan ## Architectural Context + ## Proposed V1.7 Architecture).
- Feature homes: `scripts/retrieval_quality_live.py`, `scripts/retrieval_quality_sweep.py`, `tests/e2e/reports/` (measurement harness only).
- Shared / global decisions: retrieval measurement is a repo-wide quality gate; keep helpers reusable but do NOT move ranking/business logic into the harness.
- Arm metadata must follow existing `RetrievalConfig::from_env` env-var patterns (RETRIEVAL_*, plus future OLLAMA_EMBED_MODEL / backend / rerank flags).
- Seams to honor: harness talks to the live server only via the real `find_skill` MCP tool over HTTP (MCP_URL) and judges via the real `claude` CLI; sweep reboots only the mcp-server container with env overrides.
- Review guidance: confirm no in-process retrieval reconstruction; confirm reports attribute deltas to backend/model/config arm; confirm gates not lowered and 0.80 not faked green.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T01 V1.7 measurement harness arms | tracer-bullet | Measurement surface every later V1.7 ranking change must pass through | completed | 1 | unit-01-measurement-harness-arms.md |

## Learnings Brief

- [measurement] `OLLAMA_EMBED_MODEL` is NOT yet read by the mcp-server — `build_embedding_service()` in `crates/mcp-server/src/lib.rs` hardcodes `nomic-embed-text`. T02 must wire the server side; T01 reads it harness-side with the production default as honest fallback.
- [retrieval] eq.3 weight sweeps (`beta_heavy`/`alpha_heavy`) HURT MRR (0.525/0.429 vs 0.662 default). Default weighting is near-optimal for snapshot-dense; real headroom is the backend (T04 hybrid), not the weights. Do not chase weight tuning to reach 0.80.
- [measurement] Current measured held-out baseline (this run): judge-aug MRR=0.767, nDCG@3=0.756, no_match_precision=1.0, p95≈99ms on the v1.7-baseline arm (snapshot_dense / nomic-embed-text). This is the number later arms (T02/T04/T07) must beat. 0.80 aspiration still UNMET.
- [process] Arm metadata block shape now in reports: `arm={backend, embedder_model, dense, sparse, rerank}` + `latency_ms={mean, p50, p95, n}`. Downstream tickets read/extend these via env names `RETRIEVAL_BACKEND`, `RETRIEVAL_SPARSE`, `RETRIEVAL_RERANK`, `OLLAMA_EMBED_MODEL`.
