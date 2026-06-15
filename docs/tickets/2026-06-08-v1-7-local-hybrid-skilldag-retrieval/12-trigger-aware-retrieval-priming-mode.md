---
ticket_id: T12
title: Trigger-aware retrieval — SessionStart priming mode + freshness slot (mechanism)
kind: expansion
status: implemented-owner-decisions-deferred
status_note_2026_06_15_t12_done: "T12 mechanism BUILT + MEASURED (session work-2026-06-15-t12-priming, branch feat/v-1-7). Units: (1) typed RetrievalIntent seam, SessionStart trigger→Priming, Task byte-identical; (2) query-side multi-view max-over-segments; (3) Priming-scoped floor (0.30) + recurrence/freshness ranker via skills.created_at on the snapshot; (4) live sweep on the T18 session_start stratum through compile_context. MEASURED VERDICT: neg-control PASS (62.5% crater); baseline reproduced T18 0.0685 exactly; primed cov@3=0.0805 → paired +0.012, sign_p=1.0 → FAILS the +0.10 recurrence-baseline bar. Per-signal ablations: recurrence INERT (Δ=0.000 @ w=0.1 and 0.6 — priors sparse/uniform @262), freshness slot INERT (isolated Δ=0.000), centrality DROP-by-default, query-side multi-view INERT for cov@3 (identical 0.0805 at caps 1/2/3/8). REAL WIN: no_match 14%→0% (the motivating production fix; prime never empty) from the lower floor alone, at single-embed latency (cap=1 verbose p95 564ms ≈ T18 baseline 560ms). LATENCY FLAG: multi-view verbose p95 2240ms@8 breaches the 500ms budget (Ollama semaphore serializes per-segment embeds). OWNER 2026-06-15: gate #1 (default-ON) — keep multi-view ON, FLAG for reconsideration (default priming_max_segments=8; needs a latency fix before production flip); gate #2 (per-signal verdicts) — HOLDING (reviewing artifacts). Both owner decisions DEFERRED → ticket implementation-complete but not owner-closed. Raw artifacts tests/e2e/reports/retrieval/t12_priming_*.json. Verdict: docs/execution-sessions/work-2026-06-15-t12-priming/unit-04-measurement-verdict.md."
status_note_2026_06_13_priority_rise: "PRIORITY RISE (owner reprioritization post-T23 band): this ticket's FIRST scope item — fix the production compile_context verbose-prompt no_match — is the single highest-value real-usage retrieval fix. Every efficacy run to date (CL smoke, CL band) WORKED AROUND it via focused inject-query mode instead of measuring the real SessionStart path. It is now on the real-usage critical path (T18 instrument → T12 fix → T15 measure-through-the-fixed-path) and BLOCKS T15 (the new primary efficacy gate). The 'sequenced after T14 for attribution' constraint is DISCHARGED — the band yielded ~no attribution; design input is T18's verbose substratum. Depends on T18 only now. Rationale: docs/plans/2026-06-13-v1-7-reprioritization-post-clband.md."
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
- **FIRST scope item (added 2026-06-12, from T14 smoke Finding 2): fix verbose-prompt priming.**
  The production `compile_context` path returns `no_match` for realistic verbose prompts
  (prompt-length dilution under qwen3 + the 0.48 floor); any ranker work before this is fixed
  re-ranks an empty result. Candidate mechanisms, decided on the T18 instrument (verbose substratum),
  not by intuition: (a) **intent-conditional floor** — `Priming` intent uses bounded top-N with a
  lower/no floor (priming is advisory; `Task` intent keeps 0.48 and its measured no-match precision
  0.92), (b) **query-side multi-view** — segment the verbose prompt and max-over-segments, the
  symmetric remedy to T09's doc-side max-over-views win (same length-dilution disease), (c)
  focused-query distillation (mind the no-LLM-on-hot-path fence). Do NOT naively lower the global
  floor: that trades away the T11-measured negative rejection.
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

- [x] Typed `RetrievalIntent` seam: SessionStart → `Priming`, prompt/`find_skill` → `Task`;
      documented and matching code; `Task` path byte-identical to pre-T12 behavior (Unit 1; existing
      retrieval + mcp-server lib gates green; Task branch is the unchanged pre-T12 code path).
- [x] Priming ranker (recurrence-baseline + bounded freshness slot, bounded N) implemented (Unit 3:
      Priming-scoped floor 0.30, N=5, recurrence prior boost, freshness slot via `skills.created_at`
      on the snapshot). A thin session-start prompt now surfaces project-baseline skills (no_match
      eliminated). [Measured caveat: recurrence/freshness are INERT on the 262 corpus — see below.]
- [x] Measured on the T18 session-start stratum with T18's pre-registered metrics through the
      production `compile_context`; negative control (permutation) PASSED FIRST (62.5% crater); paired
      + sign-test computed; raw artifacts persisted (`tests/e2e/reports/retrieval/t12_priming_*.json`).
- [~] Each signal's keep/drop decision cites the T18 threshold verbatim; measured deltas recorded
      (recurrence Δ0.000; freshness-slot Δ0.000; multi-view Δ0.000 cov@3; centrality default-DROP).
      VERDICT FINALIZATION DEFERRED — owner gate #2 holding (reviewing artifacts). Scale-bound (262).
- [x] Centrality/recent-use NOT shipped (default DROP — not implemented; T11 ranking-inert,
      corroborated by the measured recurrence inertia).
- [x] T14 attribution branch recorded: the T14 CL band was an instrument-failure (no usable
      attribution; reprioritization 2026-06-13 discharged the constraint) → the FULL mechanism was
      built and graded on the T18 instrument. Branch = full ranker (then measured down to floor-only kernel).
- [~] SessionStart p95 measured live, raw artifact persisted. WITHIN budget at single-embed
      (cap=1 verbose p95 564ms ≈ T18 baseline 560ms); BREACHES at multi-view-on (2240ms@8). Owner kept
      multi-view ON and FLAGGED the latency for reconsideration before a production default-ON flip.

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
