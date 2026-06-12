---
ticket_id: T20
title: Institutionalize the T11 instrument — port the 262 fixture + α=0 canary into the e2e gate, promote the scripts to a shared measurement lib
kind: hygiene
status: ready
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: T11 instrument (tests/e2e/reports/t11/T11-VALIDATION-REPORT.md) + measurement-drives-the-real-app standing rule"
source_packet_ref: "NEW 2026-06-12 — from the post-T11 follow-up assessment: the repo carries two rulers (a validated one nothing gates on, a falsified one wired into CI)"
feature_home: "tests/e2e quality harness + scripts/ measurement lib"
depends_on: []
dependency_type: none
serves:
  - One ruler, the validated one, wired into the automated gate — so future corpus/fixture drift is caught by CI, not by the next assessment
files:
  - tests/e2e/quality/labeled_corpus.rs  # DELETED (T20 — superseded by Python gate)
  - tests/e2e/test_retrieval_quality.rs  # DELETED (T20 — superseded by test_retrieval_quality_gate.rs)
  - tests/e2e/test_retrieval_quality_gate.rs  # NEW (T20 — thin Rust shim to Python gate)
  - tests/fixtures/  # stale fixtures retired; RETIRED_FIXTURES.md tombstone added
  - scripts/retrieval_metrics.py  # renamed from t11_metrics.py (T20)
  - scripts/retrieval_sweep.py    # renamed from t11_sweep.py; --gate mode added (T20)
  - tests/e2e/reports/t11/T11-VALIDATION-REPORT.md
test_command: "full e2e quality suite green on the 262 fixture against the live stack, with the α=0 canary cratering"
tdd_mode: ralph
---

# Institutionalize the T11 instrument

## Serves

The follow-up assessment (2026-06-12) found the repo carrying two rulers: the T11-validated 262
instrument lives only in `scripts/t11_*`, while the automated e2e gate
(`tests/e2e/quality/labeled_corpus.rs:82`) still loads the OLD stale fixture
(`retrieval_quality_labeled.json`) — and `retrieval_quality_meets_thresholds_on_live_stack` +
`semantic_retrieval_beats_lexical_baseline_on_disjoint` FAILED 2/4 in the 2026-06-11 full run
(non-gating diagnostic). T12/T14/T18 all lean on this instrument; it must be the one CI sees.

## Scope

- **Fixture port:** swap the e2e quality harness to `retrieval_quality_262_corpus_labeled.json`
  (adapt the loader to the strata/split/anchor schema; retire or tombstone the stale fixtures with a
  pointer). Thresholds come from the T11-measured values (anchor-only floor ~0.68 MRR@3 /
  candidate-recall@50 ~0.79 with dense-views default-ON; pick gate values just below measured so
  regressions fire, noise does not — record the chosen margins).
- **α=0 canary as a permanent test:** an `--ignored` live test that boots the server with
  `RETRIEVAL_ALPHA=0` and asserts the crater (≥50% relative MRR drop). If the fixture ever drifts
  out of alignment with the corpus, this is the test that screams.
- **Promote the scripts:** `scripts/t11_metrics.py` / `scripts/t11_sweep.py` renamed to
  `scripts/retrieval_metrics.py` / `scripts/retrieval_sweep.py` (T20, 2026-06-12).
  All references updated; `--self-test` green; historical session logs under
  `docs/execution-sessions/work-2026-06-11-192727-T11/` left untouched (immutable).
- **Close the two evidence gaps the follow-up assessment found in T11's report:**
  - Re-measure dense-views latency on the live server and PERSIST the raw per-query latency artifact
    (the cited p95 369ms currently has no source artifact in `tests/e2e/reports/t11/`).
  - Append an explicit Erratum to `T11-VALIDATION-REPORT.md`: dense_views gold-in-pool is 109/137
    (+10), not 108 (+9) — its own cand_recall 0.7956 × 137 = 109. Do not silently edit the original
    numbers; errata are appended.
- **Candidate-recall@limit in the gate:** the e2e quality test asserts candidate-recall alongside
  MRR/nDCG — T11 proved it is the lever; the gate must watch the lever.

## Scope Fence

- No production crate changes — harness, fixtures, scripts, docs only.
- No threshold gaming: gate values derive from T11-measured numbers with recorded margins, not from
  whatever the suite happens to produce.
- The stale fixture is retired loudly (removed or tombstoned with a pointer), never left as a second
  load path.

## Acceptance Criteria

- [x] e2e quality harness loads the 262 fixture; the 2 previously-failing quality tests pass against
      the live stack on T11-derived thresholds (margins recorded). **Resolved via the owner-chosen
      mechanism below: the validated Python instrument IS the gate (`--gate`); the superseded
      synthetic-seed Rust tests were retired. Live gate PASS, all 6 floor assertions green.**
- [x] α=0 canary test exists and craters live. **`alpha0_control` arm in `--gate`; live crater 100%
      (MRR 0.743→0.000); canary asserts ≥50%.**
- [x] candidate-recall@limit asserted in the gate. **`candidate_recall_at_limit` floor 0.68; live 0.796.**
- [x] Scripts promoted to the ticket-agnostic shared home; references updated; `--self-test` green.
      **`retrieval_metrics.py`/`retrieval_sweep.py`; 38/38 self-test.**
- [x] Latency raw artifact persisted; T11 report Erratum appended (109/137, latency artifact pointer).
      **`latency_t20-gate-20260612-081155.json` (137 queries; measured p95 375.3ms); erratum cites it
      + the 109/137 correction.**
- [x] Stale fixture(s) retired loudly. **`retrieval_quality_labeled.json` +
      `retrieval_quality_234_corpus_labeled.json` deleted; tombstone `tests/fixtures/RETIRED_FIXTURES.md`.**

## Resolution mechanism (owner decision 2026-06-12)

The validated T11 ruler is the Python instrument driving the REAL mcp-server over HTTP on the live
262 corpus (anchor-based) — "measurement drives the real app". The Rust `test_retrieval_quality.rs`
was a DIFFERENT, superseded instrument (seeded a synthetic corpus, measured `compile_context`); it
could not consume the anchor/strata/split 262 fixture. Owner chose to **promote the Python instrument
as the gate** (`scripts/retrieval_sweep.py --gate`) + a thin Rust `#[ignore]` shim
(`tests/e2e/test_retrieval_quality_gate.rs`) that shells to it, and **retire the synthetic Rust
instrument + stale fixtures loudly**. One validated ruler; no second 262 implementation to re-validate.

**Live GREEN evidence (gate run `t20-gate-20260612-081155`, orchestrator-driven):**
- dense_views_on: MRR@3 0.743 / MRR@10 0.743 / nDCG@3 0.755 / cand-recall@50 0.796 / no_match 0.92 —
  reproduces T11 §2 exactly; all above floors (0.64/0.64/0.64/0.68/0.88, each below the T11
  single-view-dense number with a recorded margin so the gate is robust to the dense_views flag state).
- alpha0_control: 0.000 across the board → 100% MRR crater (canary ≥50% required).
- GATE: PASS (all 6 assertions). Measured `find_skill` latency p95 375.3ms < 500ms SLO (mean 282.7,
  p50 266.4, n=137 — real wall-clock, persisted artifact; the script's original placeholder note was
  replaced with genuine timing per the no-fakes rule).
- Live stack restored to default env (RETRIEVAL_ALPHA/DENSE_VIEWS unset), /health 200, real query
  verified post-run (`prohibit-concurrent-cargo-ops-across-agents` 0.749).

## Local Context

- Sequenced immediately after T21 (gates green) so the full-suite verification run is meaningful.
- T18 hard-depends on this (the priming instrument builds in the shared lib home, not a new one-off
  script family).

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
