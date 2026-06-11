---
ticket_id: T12
title: Trigger-aware retrieval — SessionStart priming mode + freshness slot (mechanism)
kind: expansion
status: blocked
status_note: "Restructured 2026-06-12: the 2026-06-11 Rethink is now folded into the body (the old contradictory MRR wording is gone). Instrument half split out to T18 (hard dep); #180 cross-project recurrence extracted to T19 (deferred — unmeasurable on a single-project corpus). Sequenced AFTER T14 so per-pull attribution scopes the investment."
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Agent usefulness targets + ## Retrieval Flow"
source_packet_ref: "promoted from todo #220 (P1); restructured 2026-06-12 (instrument→T18, recurrence→T19)"
feature_home: "crates/retrieval and crates/compiler (SessionStart priming path)"
depends_on:
  - T11
  - T18
dependency_type: hard
serves:
  - SessionStart priming vs mid-session task retrieval as distinct, measured intents
files:
  - crates/retrieval/src/
  - crates/compiler/src/
  - tests/e2e/reports/
test_command: "real-server priming sweep on the T18 session-start stratum (pre-registered metrics, negative control passed) + unit tests for the intent seam"
tdd_mode: ralph
---

# Trigger-aware retrieval — SessionStart priming mode + freshness slot (mechanism)

## Serves

Retrieval currently treats SessionStart priming and mid-session `find_skill` identically.
SessionStart should PRIME (a bounded, high-value set: the project's recurrence-baseline skills plus a
freshness slot for high-value brand-new skills), while prompt/`find_skill` does task retrieval. T11
settled the task-retrieval side (ranking saturated at judge-aug 0.91; dense multi-view default-ON
captured the recall win); what T11 did NOT settle — the priming intent — is this ticket.

## What T11 settled (binding constraints, folded from the 2026-06-11 Rethink)

1. **Candidate-recall, not ranking, is the lever at this scale.** MRR@3 == MRR@10 for every arm —
   there is no rank-4..10 population to re-order. A task-retrieval re-rank signal (centrality,
   recent-use) is therefore presumptively inert: it ships ONLY if it measurably raises
   candidate-recall@limit (pulls a missing gold INTO the pool) per the T18 pre-registered threshold.
   The bar is high and the default is DROP.
2. **Priming is never scored with MRR/nDCG.** Priming has no single gold and MRR is
   quantized/saturated (T11). The metrics are T18's pre-registered set: set-coverage of the labeled
   baseline set, freshness hit-rate, judge-rated usefulness.
3. **No new candidate sources.** BM25/lexical candidate fusion is net-negative (snapshot_hybrid lost
   23 golds from the pool). The freshness slot is a bounded explicit injection / re-rank over the
   existing dense pool — never a new candidate source.
4. **The freshness slot is the motivated part.** T11 measured ~28% of golds missing from the top-50
   anchor-only pool; a slot guaranteeing brand-new high-value skills surface addresses exactly that
   recall/cold-start (#217) gap on the priming path.
5. **Conclusions are scale-bound** (262 skills). Any kept/dropped verdict is recorded as
   corpus-size-conditional; centrality may matter at 5k where candidate-recall is the predictor.

## Scope

- **Trigger-aware seam:** a typed `RetrievalIntent` (`Priming` vs `Task`) threaded through the
  retrieval orchestrator and the compiler SessionStart path — NOT another env flag
  (`RetrievalConfig` is already env-heavy; per-intent env config would multiply the matrix).
  `Task` intent behavior is byte-identical to today.
- **Priming ranker (bounded N):** primed set = high-recurrence project-baseline skills (recurrence
  WITHIN the project scope — cross-project recurrence is T19, deferred) + a bounded freshness slot
  (explicit injection over the dense pool per constraint 3).
- **Centrality / recent-use:** implement only behind the T18 candidate-recall threshold; default
  outcome is drop, with the measured delta recorded either way.
- **Measure on the T18 instrument:** baseline-vs-primed paired per-query + sign-test verdicts on the
  session-start stratum, negative control re-run with the new ranker, raw artifacts persisted.
  Keep/drop decisions cite the T18 pre-registered thresholds verbatim.
- **Use T14's attribution data to scope investment:** T14 (sequenced before this ticket) labels every
  pull as priming vs `find_skill`. If priming pulls contribute ~nothing to task outcomes, implement
  the minimal seam + freshness slot and stop; if they dominate, the full ranker is justified. Record
  which branch was taken and why.

## Scope Fence

- Priming stays within the constitutional 500ms SessionStart budget; no LLM call on the hot path.
- Signals that don't clear their pre-registered threshold are dropped, not shipped.
- No new candidate sources; no broadened candidate generation (T11 constraint).
- Bounded primed set — never "inject more context" as a strategy.
- No cross-project recurrence work here (T19 owns it, gated on a multi-project corpus).
- Every measured claim drives the real running mcp-server over HTTP (standing rule).

## Acceptance Criteria

- [ ] Typed `RetrievalIntent` seam: SessionStart → `Priming`, prompt/`find_skill` → `Task`;
      documented and matching code; `Task` path byte-identical to pre-T12 behavior (proven by the
      existing quality gate staying green).
- [ ] Priming ranker (recurrence-baseline + bounded freshness slot, bounded N) implemented; a
      thin/vague session-start prompt surfaces high-value project baseline skills incl. a relevant
      brand-new one.
- [ ] Measured on the T18 session-start stratum with T18's pre-registered metrics; negative control
      passed before any verdict; paired + sign-test verdicts; raw artifacts persisted.
- [ ] Each signal's keep/drop decision cites the T18 pre-registered threshold verbatim; dropped
      signals recorded with their measured delta; verdicts stated as scale-bound.
- [ ] Centrality/recent-use shipped ONLY if candidate-recall@limit (task) or coverage (priming)
      cleared their pre-registered bars.
- [ ] T14 attribution data reviewed and cited in the investment decision (minimal seam vs full
      ranker); the branch taken is recorded.
- [ ] SessionStart p95 within the 500ms budget, measured live, raw latency artifact persisted.

## Local Context

- WHY source: plan `## Agent usefulness targets`; referenced in the plan's source_docs (todo #220).
- The freshness slot connects to the cold-start concern (#217).
- History: 2026-06-11 amendments added the T11 dep + pre-registration; the post-T11 Rethink
  (2026-06-11) reframed the ticket; the 2026-06-12 restructure folded that Rethink into this body,
  split the instrument to T18, and extracted #180 recurrence to T19. Prior wording lives in git.
- See `tests/e2e/reports/t11/T11-VALIDATION-REPORT.md` and [[v17-t11-hybrid-verdict-dense-views-win]].

## Source

Promoted 2026-06-09 from todo #220 (P1). Original analysis in git of `todos/220-*`.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
