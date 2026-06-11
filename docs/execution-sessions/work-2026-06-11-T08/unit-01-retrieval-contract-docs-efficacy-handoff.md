---
unit: "T08 Retrieval contract docs and efficacy handoff"
unit_number: 1
unit_kind: hardening
serves: "Honest V1.7 retrieval contract docs + #205/#218 efficacy instrumentation handoff; closes Phase A"
status: completed
attempt_count: 1
domains: [docs, retrieval, measurement, efficacy-handoff]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/08-retrieval-contract-docs-efficacy-handoff.md
session_id: work-2026-06-11-T08
---

## What Was Implemented

Reconciled the V1.7 retrieval documentation with the shipped, measured implementation and wrote the efficacy handoff. Authored by the orchestrator (held the live T06/T09 measurements + the 0/30 fixture finding); every claim grounded in verified code (`RetrievalBackend` orchestrator.rs:166, `DEFAULT_EMBEDDING_MODEL` ollama.rs:18, eq.3 scoring.rs, protocol.rs tool descriptors) or live measurement.

- **NEW `docs/assessments/2026-06-11-v1-7-retrieval-contract-measured.md`** — the V1.7 measured assessment: what ships (backend selector, qwen3, multi-view, typed edges, agent graph tools), the live-proven #260 relevance fix (0.836/0.740/0.748 vs RRF 0.0164), the ranking-inert community multiplier, the honest gaps (held-out MRR/nDCG + 0.80 target NOT validatable on the dogfood corpus — 0/30 fixture, → T11; floor not re-validated for qwen3; hybrid bet still open), latency (p95 ~114ms warm), and the #205/#218 instrumentation handoff (attribution via embedding_model_metadata + /health components + per-call score/rationale/retrieval_context).
- **`docs/reference/retrieval-contract.md`** — added §0 "V1.7 delta" (additive over the grounded v1.5.1 core): qwen3 embedder, RETRIEVAL_BACKEND selector + experimental hybrid arms, dense multi-view (default-OFF), T07 skipped, typed edges (not a multiplier), #260 score/fusion_rank_score/rationale/retrieval_context, ranking-inert community term, and the un-revalidated 0.48 floor / 0.80 target. Fixed the stale "feat/v-1-5-1" header framing.
- **`docs/reference/online-retrieval-cqrs.md`** — added a V1.7 note: default snapshot_dense keeps Qdrant write-side only; snapshot_hybrid keeps CQRS intact; `qdrant_hybrid` (experimental, opt-in, no measured uplift) deliberately reads Qdrant at query time and would require a new ADR + health-marker changes if ever promoted (approval-sensitive, alters DS-003).

## Files Changed
- `docs/assessments/2026-06-11-v1-7-retrieval-contract-measured.md` — created
- `docs/reference/retrieval-contract.md` — V1.7 delta section + header
- `docs/reference/online-retrieval-cqrs.md` — qdrant_hybrid query-time read exception

## Acceptance Criteria
- Docs match code and runtime flags — ✅ (grounded in verified code).
- Default backend / embedder / reranker status / graph-search behavior clear — ✅.
- Final assessment states whether V1.7 hit 0.80 — ✅: **NOT validated** (un-measurable on the dogfood corpus until T11's aligned fixture); last honest held-out (0.767) was the 234/nomic corpus and does not transfer.
- #205/#218 can consume a clear instrumentation contract — ✅ (assessment §6).
- T07 recorded as skipped/experimental in docs — ✅.

## Test Results
- `test -f docs/reference/retrieval-contract.md && test -f docs/reference/online-retrieval-cqrs.md` → PASS (both exist; assessment also present).
- Live held-out gate (`retrieval_quality_live.py --gate --regression-floor 0.60`) → **NOT RUN (intentional, honest).** Structurally un-runnable on the live 262-corpus: the committed `234_corpus` fixture is 0/30 aligned (PG join `30|0`) → any MRR ≈ 0 → a forced run would fabricate a meaningless failure/pass. Recorded as T11's deliverable (build the aligned fixture, then run the gate). Same root cause as T09's deferred sweep. No fake pass recorded.

## Problems Encountered
- **The ticket test_command embeds a live quality gate that cannot honestly run on the current corpus.** Root cause: corpus rebuilt (T10, 262 qwen3 skills) but the labeled eval fixture still targets the retired 234 corpus. Resolution: document the gap loudly (assessment §4.1) and delegate the measured gate to T11 rather than force a 0-by-construction number. Consistent with the no-fakes / honest-measurement mandate.

## Notes
- Docs-only change; no code touched; no build/clippy impact. Phase A (T01–T09, minus skipped T07) now closed; the efficacy substrate is handed to Phase B with explicit, honest gaps.
