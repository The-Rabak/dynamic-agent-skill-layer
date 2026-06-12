---
ticket_id: T14
title: Prove the system is USEFUL — task-outcome A/B harness (layer ON vs OFF)
kind: efficacy
status: in_progress
status_note_2026_06_12_smoke: "BATCH 16 SCOPED WORK DONE (build + smoke; owner chose full run = follow-up; session work-2026-06-12-122502-T14, commits efa04ea + 099ab0d + this). Harness VALIDATED end-to-end on the live stack (9/9 Sonnet solves; materialize→live-injection-over-HTTP→solve→deterministic verifier→attribution→PASS/FAIL/UNDERPOWERED gate reusing T20 sign_test). Pre-registration LOCKED before data. SMOKE FINDINGS (docs/assessments/2026-06-12-t14-efficacy-harness-smoke.md): (1) P1 — the invented-rule battery does NOT discriminate against Sonnet: OFF wins even with a non-leaking prompt because the rules are within the model's default competence (the OFF-side α=0 control does not crater); (2) P1 — the production compile_context priming path no_matches verbose prompts (qwen3 floor + length dilution); only focused queries retrieve. FULL ≥10-task efficacy run DEFERRED — blocked on: re-author the battery for genuine non-pretrained discrimination + an OFF-only pre-gate; ON injection-query strategy / qwen3 floor recalibration; ≥10 real .pending drafts. No efficacy verdict claimed (honest, not spun)."
status_note: "Unblocked 2026-06-12: T10 and T13 are both completed. Restructure 2026-06-12 sequences T14 BEFORE T12's ranker work — it measures the honest baseline with the T11-validated dense-views default-ON config, and its per-pull attribution scopes T12's investment (T12 AC cites it). Amended: one invented-rule positive-control task (see Scope)."
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Non-Goals (efficacy is downstream of V1.7 retrieval); promoted into the ticket set per owner decision 2026-06-09"
source_packet_ref: "promoted from todo #205 (P0)"
feature_home: "efficacy harness (tests/e2e or scripts) over the live stack"
depends_on:
  - T10
  - T13
dependency_type: hard
serves:
  - The one unmeasured number — does the skill layer make a coding agent measurably better
files:
  - scripts/
  - tests/e2e/
  - docs/assessments/
test_command: "layer ON vs OFF A/B run over ≥10 tasks on the live stack + recorded report with per-task scores"
tdd_mode: ralph
---

# Prove the system is USEFUL — task-outcome A/B harness (layer ON vs OFF)

## Serves

Every artifact proves correctness; none proves efficacy. The whole value proposition rests on the one number nobody has measured. This builds the A/B experiment: same tasks with the skill layer ON vs OFF, plus a draft-acceptance-rate metric, with an explicit efficacy gate.

## Scope

- Efficacy harness that runs the same task set with layer ON and OFF against the live stack.
- Committed deterministic scoring rubric (judge prompt recorded in-repo if used).
- Reproducible report: measured ON-vs-OFF delta over ≥10 representative tasks, with per-task scores.
- Draft-acceptance-rate over ≥10 `.pending` drafts from REAL captured sessions (not synthetic).
- An explicit efficacy threshold in the release gate; run summary reports PASS/FAIL.
- **(Amended 2026-06-11, from `docs/assessments/2026-06-11-v1-7-midpoint-deep-grok-assessment.md`):**
  - **Pre-registration:** the pass/fail criterion is committed to this ticket BEFORE any measured run (e.g. "ON beats OFF on ≥ X of N paired tasks with no catastrophic regression" — owner sets X/N). Post-hoc readings of noisy means are not a verdict.
  - **Paired design:** every task runs both ON and OFF (same task, same rubric); the report shows per-task paired win/loss/tie and a sign test, not just aggregate means. At N≈10 the paired structure is the only thing that gives the result teeth.
  - **Three honest outcomes, not two:** PASS / FAIL / **UNDERPOWERED**. If the paired result can't distinguish the arms at the pre-registered bar, the report says "underpowered," which is distinct from "no effect" — and neither is spun.
  - **Placebo arm:** a third arm injecting irrelevant-but-plausible skill context at matched token mass (the real server serving a deliberately mismatched scope/corpus — an explicitly-labeled experimental control in the measurement harness, not a production fake). Without it, "relevant skills help" cannot be separated from "any extra context helps/hurts," and that distinction decides T12's investment.
  - **Attribution from the start:** every retrieval pull during measured runs is labeled (SessionStart priming vs mid-session `find_skill`) and lands in the per-task report — do not wait for T15 to start capturing this.
- **(Amended 2026-06-12, restructure — positive-control task):** exactly ONE task in the ≥10 set is an
  **invented-rule positive control**: a task whose correct solution requires a project-specific
  invented rule/procedure that (a) exists as a skill in the T10 corpus and (b) is verifiably absent
  from model pretraining. It plays the role for this harness that the α=0 gate played for T11 — a
  sensitivity control. Interpretation is pre-registered with the rest: if ON fails the positive
  control, the harness or injection path is broken and the run reports INSTRUMENT-FAILURE (no efficacy
  verdict may be claimed from the other 9); if ON wins ONLY the positive control, that is reported as
  "value concentrates in non-pretrained knowledge" — the strongest possible T12/CL-bench investment
  signal. This is deliberately NOT a full CL-bench band (DS-025–030 stay parked until after T14/T15 —
  extraction-fidelity confounds make them uninterpretable today); it is one task, no design change.

## Scope Fence

- Tasks/corpus come from T10 (real ingestion), not hand-authored shortcuts.
- No faked infrastructure (depends on T13). The placebo arm is an explicitly-labeled measurement control configured on the real stack, recorded as such in the report — never a silent fallback or a production path.
- A null/negative delta is documented honestly with raw data — not hidden or gamed.
- No changing the pre-registered criterion after data exists; a criterion change voids the run.

## Pre-Registration (LOCKED 2026-06-12 — committed BEFORE any measured run; changing it after data exists VOIDS the run)

Owner decisions (session work-2026-06-12-122502-T14):

- **Design — invented-rule battery.** Each task requires a project-specific rule/procedure that (a) exists
  as a skill in the T10 262-corpus and (b) is verifiably absent from model pretraining. Outcome is scored
  by a **deterministic rule-obeyed verifier** (a committed script/assertion), not an LLM judgment of
  general "quality." This is the least-confoundable design and makes the α=0-analogue sensitivity per-task.
- **Arms (paired per task):** `ON` (skill layer `compile_context` hooks against the live mcp-server) /
  `OFF` (identical agent, no skill-layer hooks) / `PLACEBO` (matched-token-mass irrelevant skill context
  served on the real stack, explicitly labeled as a measurement control — never a silent fallback).
- **Pass criterion (verbatim, the report must cite this string):**
  > **"ON wins ≥ 7 of 10 paired tasks by sign test, with no catastrophic regression on any single task."**
- **Three honest outcomes:** **PASS** (≥7/10 ON wins, no catastrophic regression) / **UNDERPOWERED**
  (positive direction but below the bar, or the sign test cannot distinguish the arms at N) / **FAIL**
  (ON ≤ OFF). A null result is reported as UNDERPOWERED, not spun as either PASS or "no effect."
- **INSTRUMENT-FAILURE (per-task sensitivity / α=0 analogue):** if `ON` fails a task whose invented rule is
  present in the corpus AND attribution shows it was injected, the harness/injection path is broken →
  report **INSTRUMENT-FAILURE**; **no efficacy verdict may be claimed** from the remaining tasks until fixed.
- **Concentration reading (pre-registered):** if ON wins ONLY the hardest non-pretrained tasks, that is
  reported as "value concentrates in non-pretrained knowledge" — the strongest T12 / CL-bench signal.
- **Solve driver:** `claude-code` on Sonnet, reusing the `scripts/swebench/settings-swebench.json` hook
  wiring; runs are **serialized by the orchestrator** (standing rule: no concurrent heavy actions).
- **Catastrophic regression (defined):** any task where `ON` obeys the rule strictly worse than `OFF` AND
  the ON solve introduces a verifier-detected harmful action the OFF solve did not (not merely a tie loss).

## Pre-Registration Amendment (2026-06-12, post-smoke — committed BEFORE any full measured run; the smoke produced no efficacy data, so this amends task selection and adds metrics without voiding anything)

- **OFF-only discrimination pre-gate (pre-committed task-selection rule):** before the full run, every
  battery task is solved with OFF alone; any task OFF passes is REJECTED as non-discriminating and
  replaced. This is the per-task α=0 control formalized as a selection rule so it can never be applied
  selectively after paired data exists. The pre-gate run is recorded with the battery.
- **Secondary metrics (pre-registered):** paired **turns-to-solve** and **token cost** per arm, reported
  alongside the primary verifier outcome. Rationale: a binary rule-obeyed verifier only measures the
  "impossible-without-the-rule" band; most real skill value is plausibly "model re-derives it
  expensively." Secondary metrics let a task where OFF eventually succeeds still contribute signal
  (ON solves in fewer turns / cheaper). They do NOT enter the ≥7/10 pass criterion (which is unchanged);
  they are reported as a separate pre-registered efficiency reading.
- **CL-bench tasks — policy (revised 2026-06-12, owner direction):** lifted CL-bench tasks are
  PERMITTED as battery tasks **only via the teach-session protocol** — never by hand-planting the rule
  as a corpus skill (planting tests only the injection pipe and violates the T10 provenance fence).
  Protocol per task: **Session A** = a genuine claude-code working session on the CL task with its rule
  material present → real pipeline (extract → `.pending` → human gate → corpus → rebuild; the drafts
  also feed the ≥10-real-drafts AC) → **fidelity gate**: deterministic check that the extracted skill
  contains the operative rule/tokens (CL-bench's invented sentinels make this checkable; same trick as
  DS-025/026). Fidelity failure = P0 EXTRACTION finding, reported as INSTRUMENT-FAILURE at the
  extraction stage — no efficacy verdict claimed or denied from that task. → **Session B** = paired
  ON/OFF/PLACEBO solve of a held-out variant requiring the rule, rule material absent, OFF pre-gate
  first (which empirically re-proves non-pretraining instead of assuming it). These tasks form the
  **acquisition band** (2-3 tasks) of the battery; the rest remain in-project organic-knowledge tasks —
  the two bands answer complementary claims and both feed the unchanged ≥7/10 criterion. Pin the
  solver checkpoint + bench version in the run report: the bench's non-pretrained property is current
  (published past every deployed model's cutoff) but EXPIRES with future checkpoints — re-run the OFF
  pre-gate on any solver change. This protocol is also the only design exercising extraction of taught
  novel rules (DS-025–030 seed directly, bypassing it) and is the dress rehearsal for DS-030/T15.
  **Band selected + planned 2026-06-12:** 10 CL-bench contexts (2 smoke + 8 full) + 3 ordered
  alternates — see `docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md` (full protocol, smoke
  definition, pre-registration deltas, full-benchmark scaling) and
  `tests/e2e/efficacy/clband/manifest.json` (pinned dataset sha, verified sentinels, eval scores);
  contexts re-materialized on demand by `scripts/fetch_clband_contexts.py` (tested 13/13 green).

- [ ] Pass/fail criterion pre-registered in this ticket before any measured run; the final report cites it verbatim.
- [ ] Efficacy harness runs the same task set with layer ON and OFF against the live stack, paired per task.
- [ ] Committed deterministic scoring rubric with the judge prompt (if used) recorded in-repo.
- [ ] Reproducible report shows the measured delta (ON vs OFF) on ≥10 tasks, with per-task paired scores, win/loss/tie counts, and a sign test.
- [ ] Placebo arm (matched-mass irrelevant context) run and reported alongside ON/OFF; the ON-vs-placebo comparison is stated explicitly.
- [ ] Per-pull retrieval attribution (priming vs `find_skill`) captured and reported per task.
- [ ] Draft-acceptance-rate over ≥10 `.pending` drafts from REAL captured sessions.
- [ ] Invented-rule positive-control task included (pre-registered interpretation as amended); its outcome reported distinctly; INSTRUMENT-FAILURE reported if ON loses it.
- [ ] Release gate includes an explicit, justified efficacy threshold; run reports PASS / FAIL / UNDERPOWERED.
- [ ] If the delta is null/negative/underpowered, documented in `docs/assessments/` with raw data.

## Local Context

- WHY source: plan `## Non-Goals` names efficacy as downstream; owner decision 2026-06-09 promoted it into the V1.7 ticket set.
- Depends on T10 (real corpus) and T13 (no-fakes substrate). Consumes T08's measured-retrieval handoff.
- T15 (#218 SWE-bench) is the flagship compounding variant of this.
- Sequenced after T20/T21 (restructure 2026-06-12): reuse the shared measurement lib (paired/sign-test
  machinery promoted from `scripts/t11_*` by T20) instead of growing a parallel stats implementation;
  run on a green tree (T21). Runs with the as-shipped default config (dense-views ON) — that IS the
  honest baseline; T12's priming improvements land after and are measured as a separate delta.

## Source

Promoted 2026-06-09 from todo #205 (P0). Original analysis in git of `todos/205-*`.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
