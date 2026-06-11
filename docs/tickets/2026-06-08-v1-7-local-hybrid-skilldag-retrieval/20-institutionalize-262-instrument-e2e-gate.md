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
  - tests/e2e/quality/labeled_corpus.rs
  - tests/e2e/test_retrieval_quality.rs
  - tests/fixtures/
  - scripts/t11_metrics.py
  - scripts/t11_sweep.py
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
- **Promote the scripts:** `scripts/t11_metrics.py` / `scripts/t11_sweep.py` are permanent
  measurement infrastructure named after a closed ticket; T12/T14/T18 are about to import them.
  Rename to a ticket-agnostic shared home (e.g. `scripts/retrieval_metrics.py` /
  `scripts/retrieval_sweep.py`), update all references (tickets, reports, docs), keep `--self-test`.
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

- [ ] e2e quality harness loads the 262 fixture; the 2 previously-failing quality tests pass against
      the live stack on T11-derived thresholds (margins recorded).
- [ ] α=0 canary test exists and craters live.
- [ ] candidate-recall@limit asserted in the gate.
- [ ] Scripts promoted to the ticket-agnostic shared home; references updated; `--self-test` green.
- [ ] Latency raw artifact persisted; T11 report Erratum appended (109/137, latency artifact pointer).
- [ ] Stale fixture(s) retired loudly.

## Local Context

- Sequenced immediately after T21 (gates green) so the full-suite verification run is meaningful.
- T18 hard-depends on this (the priming instrument builds in the shared lib home, not a new one-off
  script family).

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
