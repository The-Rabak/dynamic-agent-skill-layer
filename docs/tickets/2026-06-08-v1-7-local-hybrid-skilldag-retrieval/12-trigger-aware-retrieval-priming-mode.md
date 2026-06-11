---
ticket_id: T12
title: Trigger-aware retrieval — priming mode + recurrence-based global + freshness slot
kind: expansion
status: blocked
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Agent usefulness targets + ## Retrieval Flow"
source_packet_ref: "promoted from todo #220 (P1)"
feature_home: "crates/retrieval and crates/compiler (SessionStart priming path)"
depends_on:
  - T10
  - T11
dependency_type: hard
serves:
  - SessionStart priming vs mid-session task retrieval as distinct, measured intents
files:
  - crates/retrieval/src/
  - crates/compiler/src/
  - scripts/retrieval_quality_live.py
  - tests/e2e/reports/
test_command: "real-server priming-mode quality sweep on the T10 corpus (#210 rig) + unit tests for ranker signals"
tdd_mode: ralph
---

# Trigger-aware retrieval — priming mode + recurrence-based global + freshness slot

## Serves

Retrieval currently treats SessionStart priming and mid-session `find_skill` identically. SessionStart should PRIME (centrality + recent usage + a freshness slot for high-value brand-new skills), while prompt/`find_skill` does task retrieval. "Global appropriateness" should be defined by cross-project recurrence (#180), not a static flag. Each signal must earn its place via measured quality delta on the real corpus.

## Scope

- Make retrieval trigger-aware: SessionStart → priming, prompt/`find_skill` → task retrieval; both documented and matching code.
- Implement a priming ranker (centrality + recent usage + bounded freshness slot, bounded N).
- Define global appropriateness via cross-project recurrence (#180).
- Measure each signal's MRR/nDCG impact on the T10 corpus (#210 rig); drop signals that don't help.
- **(Amended 2026-06-11)** Measure priming on the PRIMING query distribution: coordinate with T11's fixture authoring so it includes a session-start stratum (thin/vague session-opening prompts — the distribution priming actually serves), distinct from the specific task-query strata. Priming signals evaluated against task-shaped queries would answer the wrong question.
- **(Amended 2026-06-11)** Pre-register per-signal ROI thresholds BEFORE any measured sweep: for each signal (centrality, recent-use, freshness), record in this ticket what minimum paired quality delta keeps it. "Drop signals that don't help" only has teeth if "help" is defined before the data exists.

## Scope Fence

- Priming must stay within the constitutional 500ms SessionStart budget.
- No LLM call on the SessionStart hot path.
- Signals that don't measurably help are dropped, not shipped.
- Do not blindly inject more context — priming surfaces a bounded, high-value set.

## Acceptance Criteria

- [ ] Retrieval is trigger-aware: SessionStart → priming, prompt/`find_skill` → task retrieval; documented and matching code.
- [ ] Priming ranker (centrality + recent usage + freshness slot, bounded N) implemented; MRR/nDCG impact measured on the T10 corpus; a thin/empty session-start prompt surfaces high-value project baseline skills incl. a relevant brand-new one.
- [ ] Global appropriateness defined via cross-project recurrence (#180), measured, matching code.
- [ ] Each signal's measured quality delta recorded; non-helping signals dropped.
- [ ] Per-signal ROI thresholds were recorded in this ticket BEFORE the measured sweeps ran; the keep/drop decisions cite them.
- [ ] Priming measured on a session-start query stratum (from the T11 fixture), not only on task-shaped queries.
- [ ] T15 (#218) source-attribution (priming vs find_skill) reviewed and used to scope investment.

## Local Context

- WHY source: plan `## Agent usefulness targets`; referenced in the plan's source_docs (todo #220).
- Needs the T10 corpus to measure; the freshness slot connects to the cold-start concern (#217).
- Coordinates with T15 (#218) which captures retrieval-source attribution per pull.
- **Amendment 2026-06-11:** T11 added as a hard dependency — every measured claim in this ticket runs through the quality instrument, and T11's midpoint-assessment findings showed the prior instrument could not see arm differences (saturated fixture, mean-equality verdicts). Measuring priming signals on a broken ruler would ship noise as ROI. See `docs/assessments/2026-06-11-v1-7-midpoint-deep-grok-assessment.md`.

## Rethink (post-T11, 2026-06-11)

T11 ran on the discriminating 262-corpus fixture and its findings reshape this ticket. Fold these in
BEFORE executing; they override conflicting wording above where noted.

1. **The task-retrieval re-ranking signals are likely inert — deprioritize or cut them.** T11 proved
   that at this corpus scale **candidate-recall, not ranking, is the lever**: MRR@3 == MRR@10 for every
   arm (the first relevant hit is in the top-3 or absent from the top-10 entirely; there is no
   rank-4..10 population to re-order). Mid-session `find_skill` ranking is already saturated (judge-aug
   0.91) and dense multi-view (now default-ON) already captured the recall win. Centrality / recent-use
   re-ranking on a saturated list will, by the same structural argument that flattened the hybrid arms,
   show ~0 MRR delta. **Keep a task-retrieval signal only if it measurably raises candidate-recall@limit
   (pulls a missing gold INTO the pool) — not MRR.** A re-rank signal cannot do that, so the bar is high.

2. **Priming's success metric is wrong as written (AC#2 "MRR/nDCG impact").** SessionStart priming has
   no single gold skill, and T11 showed MRR@3 is quantized/saturated. **Pre-register a priming-appropriate
   metric instead:** set-coverage of the project's high-recurrence baseline skills + "≥1 relevant fresh
   skill surfaced," and/or judge-rated usefulness of the bounded primed set. Do NOT score priming with MRR.

3. **AC#6 has a hidden gap: the T11 fixture contains NO session-start stratum.** The shipped
   `tests/fixtures/retrieval_quality_262_corpus_labeled.json` is entirely task-shaped (transcript /
   disjoint / lexical / multiview / use_when / negative). T12 must **author the session-start stratum
   itself** — reuse `scripts/build_t11_fixture.py` + the anti-circularity discipline (thin/vague
   session-opening prompts from the 24 transcripts' *opening* turns, gold = the project-baseline skills
   a useful prime would surface). Do not assume T11 provided it.

4. **Do not broaden candidate generation.** T11 showed adding a BM25/lexical candidate source is
   net-negative (snapshot_hybrid lost 23 golds from the pool). The freshness slot must be a **bounded
   explicit injection / re-rank over the existing dense pool**, never a new candidate source.

5. **The freshness slot is MORE motivated, not less.** T11 measured ~28% of golds missing from the
   top-50 pool (anchor-only) and dense-views recovered some; a freshness slot that guarantees brand-new
   high-value skills surface addresses exactly that recall/cold-start (#217) gap — for the priming path.

6. **Reuse T11's instrument + honesty discipline.** Run every claim through the real server on the
   262 corpus; reuse the α=0-style negative control, candidate-recall@limit, and **paired per-query +
   sign-test** verdicts (not 3-decimal mean equality). Note explicitly that **+0.03 MRR is within
   1-query noise at N≈137** before citing any MRR delta. State conclusions are **scale-bound** (262
   skills; candidate-gen cannot move ranking here by construction) — centrality may matter more at 5k,
   where candidate-recall is the predictor.

**Net reframing:** T12 becomes "**SessionStart priming + freshness slot + recurrence-based global
appropriateness**" (the parts T11 did not already settle), measured with priming-appropriate metrics on
a self-authored session-start stratum. The task-retrieval re-ranking signals are dropped unless
candidate-recall (not MRR) earns them. See `tests/e2e/reports/t11/T11-VALIDATION-REPORT.md` and
[[v17-t11-hybrid-verdict-dense-views-win]].

## Source

Promoted 2026-06-09 from todo #220 (P1). Original analysis in git of `todos/220-*`.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
