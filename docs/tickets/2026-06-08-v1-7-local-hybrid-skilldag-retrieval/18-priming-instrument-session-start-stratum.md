---
ticket_id: T18
title: Priming instrument — session-start stratum, pre-registered priming metrics, negative control
kind: measurement
status: ready
status_note: "Ready: T10/T11/T20 all completed (T20 landed 2026-06-12); index already shows ready."
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Agent usefulness targets; T11 instrument-first discipline (tests/e2e/reports/t11/T11-VALIDATION-REPORT.md)"
source_packet_ref: "split out of T12 (restructure 2026-06-12) — instrument half of the former trigger-aware-retrieval ticket"
feature_home: "scripts/ measurement lib + tests/fixtures (NO crate changes)"
depends_on:
  - T10
  - T11
  - T20
dependency_type: hard
serves:
  - A priming-appropriate measurement instrument so T12's signals earn (or lose) their place on evidence, not vibes
files:
  - scripts/build_t11_fixture.py
  - tests/fixtures/
  - tests/e2e/reports/
test_command: "negative-control gate (wrong-scope prime craters coverage) passes BEFORE any baseline number is recorded; baseline prime measured on the real server"
tdd_mode: ralph
---

# Priming instrument — session-start stratum, pre-registered priming metrics, negative control

## Serves

T11 proved measurement must precede verdicts: the priming half of T12 cannot be judged until an
instrument exists that measures the *priming* distribution (thin/vague session-opening prompts, no
single gold skill) with *priming-appropriate* metrics. The shipped T11 fixture is entirely
task-shaped — this ticket authors the missing stratum and pre-registers the metrics and thresholds
T12 will be graded against. This is T11's "instrument first, sweep second" pattern applied to
priming, as its own ticket so the instrument exists before the mechanism it measures.

## Scope

- **Author the session-start stratum** (new fixture or extension of
  `tests/fixtures/retrieval_quality_262_corpus_labeled.json` under a distinct `session_start` kind):
  thin/vague session-opening prompts drawn from the 24 genuine transcripts' *opening* turns, authored
  via the real claude CLI per the T11 protocol. Gold = a labeled SET of project-baseline skills a
  useful prime would surface (multi-gold, not single-anchor), mapped via `source_session_id`.
  Anti-circularity: prompts come from opening turns / fresh-vocabulary paraphrases, NEVER from the
  gold skills' own `use_when`/description text; verify with the token-overlap probe (headline overlap
  must stay in the ~0.3 band T11's transcript/disjoint strata achieved, not the 0.6+ band).
- **Pre-register the priming metrics** (recorded in this ticket BEFORE any measured run): set-coverage@N
  of the labeled baseline set, "≥1 relevant fresh skill surfaced" (freshness hit-rate), and judge-rated
  usefulness of the bounded primed set. MRR/nDCG are explicitly NOT priming metrics (T11: quantized,
  saturated, no single gold).
- **Pre-register per-signal ROI thresholds** for every signal T12 may ship (recurrence-baseline,
  freshness slot, centrality, recent-use): the minimum paired delta on the pre-registered metrics that
  keeps the signal. "Drop signals that don't help" only has teeth if "help" is defined before data exists.
- **Negative-control gate (the α=0 analogue for priming):** a prime computed from a deliberately wrong
  scope/project (or scrambled baseline labels) must crater set-coverage relative to the true-scope
  prime. If it does not, the coverage metric is vacuous and the stratum is rejected — no T12 verdict
  may ride on it. The gate runs FIRST.
- **Measure the BASELINE prime** (current SessionStart behavior, dense-views default-ON config) on the
  new stratum against the real mcp-server, so T12 has an honest before-number. Persist raw per-query
  artifacts (T11 report format: raw vectors + paired-ready per-query JSON).

## Scope Fence

- NO crate changes — this is scripts/fixtures/reports only. The mechanism is T12.
- Standing rule: every measured claim drives the real running mcp-server over HTTP; no in-process
  reconstruction.
- No number cited without its persisted raw artifact (close the T11 latency-claim gap class: if a
  latency or score is reported, its raw per-query data lands in `tests/e2e/reports/`).
- Authoring and threshold pre-registration are by the same hands — the thresholds and judge rubric
  MUST be committed to this ticket before the stratum queries are authored, to close the remaining
  self-authoring circularity channel.

## Acceptance Criteria

- [ ] Session-start stratum authored (≥20 queries from the 24 transcripts' opening turns, multi-gold
      baseline sets labeled, anti-circularity verified via token-overlap probe ~0.3 band).
- [ ] Priming metrics + per-signal ROI thresholds + judge rubric recorded in this ticket BEFORE the
      stratum was authored and BEFORE any measured run; later docs cite them verbatim.
- [ ] Negative-control gate ran FIRST and cratered (wrong-scope prime coverage collapse); result
      persisted with raw data.
- [ ] Baseline prime measured on the real server on the new stratum; raw per-query artifacts persisted.
- [ ] Instrument lives in the shared measurement lib home (T20), not a new one-off script family.

## Local Context

- Split out of T12 by the 2026-06-12 restructure (post-T11 follow-up assessment): the former T12
  carried instrument + mechanism + policy in one packet and self-contradicted after its Rethink.
- Parallel-safe with T14 in feature-home terms (scripts/fixtures vs efficacy harness), but both drive
  the live server — default singleton sequencing holds; no concurrent heavy runs (standing rule).

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
