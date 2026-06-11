---
unit: "Instrument gate: alpha=0 negative control"
unit_number: 4
unit_kind: hardening
serves: "AC#1 — proves the fixture can discriminate retrieval quality before any arm verdict"
status: completed
attempt_count: 2
domains: [measurement]
session_id: work-2026-06-11-192727-T11
---

## Result (real server, anchor-only, all 137 positives + 25 negatives)
- snapshot_dense (baseline): MRR@3=0.686 MRR@10=0.686 nDCG@3=0.696 hit@3=0.723 cand_recall@50=0.723 no_match=0.92
- alpha0_control (RETRIEVAL_ALPHA=0, real server config): MRR@3=0.000 ... cand_recall@50=0.000 no_match=1.0
- **CRATER: 100% relative MRR drop (rel_drop=1.000 >= 0.50 threshold) → FIXTURE DISCRIMINATES.** AC#1 PASS.
- Paired sign test alpha0 vs dense: n_a_better=99 n_b_better=0 n_tie=38 p=0.0000 (significant).
- Report: tests/e2e/reports/t11/sweep_gate.json

## Harness bug found + fixed (orchestrator, runtime)
- t11_sweep routed alpha0_control through reboot_arm, whose internal wait_ready() uses a warmup find_skill query as readiness; under α=0 that query returns empty (semantic zeroed → below floor) → timed out 600s though server was up. FIX: only backend-changing arms (snapshot_hybrid/qdrant_hybrid) use reboot_arm; alpha0_control + dense_views_on are mcp-server-only knobs → reboot_mcp (no internal warmup wait; the honest /health-200 poll is the real gate). One-line condition fix in scripts/t11_sweep.py; documented inline.

## Note
- MRR@3==MRR@10 and cand_recall@50==hit@3==0.723 on snapshot_dense → gold is either in top-3 or absent from the top-50 pool entirely; bimodal, discriminating fixture. ~28% of anchor golds are not in the candidate pool (anchor-only; judge will rescue valid alternates at verdict time).
