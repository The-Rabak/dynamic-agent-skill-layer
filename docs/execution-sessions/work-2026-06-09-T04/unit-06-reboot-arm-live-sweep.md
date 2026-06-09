---
unit: "T04-D: reboot_arm + live real-server sweep"
unit_number: 6
unit_kind: hardening
serves: "The measurement that proves T04 — honest per-arm held-out quality+latency through the REAL server."
status: completed
attempt_count: 1
domains: [harness, graph-builder, mcp-server, measurement]
session_id: work-2026-06-09-T04
---

## What Was Implemented
- **`reboot_arm(overrides)`** (scripts/retrieval_quality_sweep.py): qdrant_hybrid path purges outbox events, DELETES the hybrid collection (clean rebuild), force-recreates graph-builder with arm env, polls hybrid collection until ≥30 points, force-recreates mcp-server, waits readiness, asserts collection non-empty (fail-loud 0-point guard). Snapshot arms: force-recreate graph-builder + mcp-server + wait-ready. New helpers `_delete_qdrant_collection` (404=noop), `_poll_collection_until_nonempty(min_points)`. Hybrid arms uncommented/active in CONFIGS.
- **docker-compose.test.yml**: added RETRIEVAL_BACKEND/SPARSE/RERANK env pass-through to mcp-server + graph-builder (were absent — containers couldn't receive arm selection).
- **2 PRODUCTION BUGS in qdrant_hybrid found + fixed by the live run** (mock tests missed them):
  - graph-builder rebuild.rs: Qdrant payload stored blake3-hex `skill_id` but snapshot joins on PG UUID → zero join → MRR 0.000. Fix: store `stable_skill_uuid(&skill.id)`.
  - mcp-server lib.rs: extracted `payload["payload"]["skill_id"]` (double-nested) but relay stores inner object → `payload["skill_id"]`. Fix: top-level extraction.
- qdrant.rs: `#[allow(clippy::type_complexity)]` on a test-only tuple (cleared the non-test-utils `--all-targets` gate).

## Live Results (234-skill held-out, judge-augmented, REAL server over HTTP)
| arm | MRR | nDCG@3 | hit@3 | recall@3 | no_match | p95 |
|---|---|---|---|---|---|---|
| snapshot_dense | 0.767 | 0.749 | 0.867 | 0.808 | 1.000 | 114ms |
| snapshot_hybrid | 0.767 | 0.749 | 0.867 | 0.808 | 1.000 | 128ms |
| qdrant_hybrid | 0.767 | 0.751 | 0.867 | 0.808 | 1.000 | 119ms |

Reports: tests/e2e/reports/v17-{snapshot_dense,snapshot_hybrid,qdrant_hybrid}__held_out.json

## Honest Verdict (T04 ACs MET)
- NO regression vs 0.767 current default (AC met). NO arm improves it either — hybrid/sparse gives no measurable uplift on this corpus; delta = 0 (NOT positive), so promotion is NOT justified → snapshot_dense stays default (correct).
- 0.80 aspiration UNMET by all arms. Ceiling is the embedding model + scoring weights, NOT candidate generation. The retrieval architecture is correct + functionally equivalent across arms.
- p95 < 500ms for ALL arms (114/128/119ms) — comfortably in budget.
- qdrant_hybrid end-to-end REAL (234 skills mapped via UUID skill_id; CQRS break confirmed measurable, no uplift) → T08 ADR evidence complete: keep snapshot_dense default (CQRS intact, fastest, same quality); qdrant_hybrid is infra for future fusion experiments.

## Caveat worth a T08 look
snapshot_hybrid metrics are IDENTICAL to snapshot_dense to 3 decimals. B's unit tests prove BM25 expansion engages; qdrant_hybrid nDCG differs (0.751 vs 0.749) so sparse DID do something. Likely the 30 held-out positives contain no exact-term queries that exercise BM25 — worth confirming the held-out set actually stresses lexical recall before concluding BM25 is useless.

## Gate status (honest)
- `cargo clippy --workspace --all-targets -- -D warnings`: GREEN (type_complexity fix).
- `cargo clippy --workspace --all-targets --features test-utils -- -D warnings`: RED — PRE-EXISTING e2e harness dead-code (ScopeEnvGuard/QdrantObserver/run_docker…), gated behind test-utils feature; NOT T04. Still the V1.7 final-gate blocker needing a harness cleanup pass.
- graph-builder 19 + mcp-server 33 lib tests PASS. Standing rule honored: all measurement drove the REAL server over HTTP.
