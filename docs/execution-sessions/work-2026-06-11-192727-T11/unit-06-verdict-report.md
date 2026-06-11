---
unit: "Verdict, report, contract-doc update; tie-gate stop on Rust arm"
unit_number: 6
unit_kind: hardening
serves: "the earned hybrid-vs-dense verdict + the discharged 0.80 efficacy gate"
status: completed
attempt_count: 1
domains: [measurement, docs, verdict]
session_id: work-2026-06-11-192727-T11
---

## What Was Implemented
- Judge-augmented sweep (real claude CLI) dense vs dense_views_on → tests/e2e/reports/t11/sweep_judged.json.
- Latency probe dense_views_on: p95 369ms < 500ms SLO.
- Full report: tests/e2e/reports/t11/T11-VALIDATION-REPORT.md.
- Updated docs/reference/retrieval-contract.md (§0 V1.7 delta: hybrid falsified/qdrant-equivalent; dense-views validated, recommend default-ON; 0.80 now VALIDATED; floor confirmed).
- Appended §8 to docs/assessments/2026-06-11-v1-7-retrieval-contract-measured.md (efficacy gate DISCHARGED).
- index.md Batch 13 → completed, last_completed_batch 12→13, T11 row completed, T12 unblocked→ready.
- T11 ticket status → completed, all 10 ACs checked with measured evidence.
- Fixed t11_sweep paired call ordering (cosmetic label) for future runs.

## VERDICT (final)
- Frozen 0.80 aspiration MET (judge-aug held-out): dense 0.884/0.804/0.92, dense_views 0.912/0.839/0.92.
- SPARSE/BM25 hybrid FALSIFIED (snapshot_hybrid net-negative). qdrant_hybrid EXACT tie (no promotion).
- DENSE multi-view (T09) VALIDATED → recommend RETRIEVAL_DENSE_VIEWS default-ON (pending owner flag flip).
- Candidate-recall is the lever (MRR@3==MRR@10). Floor 0.48 well-calibrated.
- Tie gate → STOPPED, no Rust lexical arm (owner decision).

## Tie-gate / Rust-arm decision
Per owner decision 2026-06-11 ("Stop at the tie gate and report"): the env-gated lexical-ranking
Rust arm was NOT built. dense≡qdrant_hybrid tied exactly; BM25-as-candidate already hurt → strong
prior a BM25 ranking term would not win. Deferred to a separate owner decision. T11 stayed
measurement-only (no crates/retrieval changes).

## Test Results
- All AC evidence is live real-server measurement. No fakes, no fabricated zeros, no in-process rig.
