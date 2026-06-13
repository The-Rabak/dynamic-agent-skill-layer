---
ticket_id: T14
title: Prove the system is USEFUL — task-outcome A/B harness (layer ON vs OFF)
kind: efficacy
status: in_progress
status_note_2026_06_13_clband_band: "CL ACQUISITION BAND RAN (T23, unattended; report docs/assessments/2026-06-13-t14-clband-band.md). VERDICT vs the LOCKED >=7/10 criterion: INSTRUMENT-FAILURE — 0 clean paired efficacy points; the efficacy question is UNANSWERED (not PASS/FAIL/UNDERPOWERED). The harness is PROVEN end-to-end (OFF pre-gate discriminates 7/8; auto-gate scope-guarded + isolated; canary + dartman built+retrieved; dogfood re-probe = 262 pristine). Binding constraint = EXTRACTION FIDELITY (refines T22): rules/procedures/structure survive, but extraction DROPS verifier-precise specifics — 5/8 contexts are GENUINE gaps (verifier-against-drafts confirms ON would fail: <1 megaohm, MATCH|MISMATCH|MISSING enums, 40% RH floor, diagnosis/prognosis, M-WARN-01), 1/8 fidelity-gate FALSE-NEGATIVE (quartermaster recoverable), 1/8 non-discriminating (ezlang OFF=win), 1/8 timeout-confounded (dartman depth-8 >20min). Efficacy STILL UNPROVEN. Recommend follow-ups: (1) extraction value-preservation ticket; (2) verifier-based fidelity gate; (3) task-design fixes (dartman timeout, ezlang sibling, instrument the 3 alternates); (4) re-run. NO mid-run protocol change; pre-registration intact."
status_note_2026_06_12_t22_gate: "Forensic re-read (same day, assessment addendum): the extraction failure has THREE components — sanitizer drops the system-home document (missed by the smoke report), extractor worldview refuses non-recurring taught knowledge (verified verbatim, survives visibility), and gate sentinels mis-leveled vs the verbatim-capable preference channel (the 11 drafts carry invented operative specifics verbatim). Fixes ticketed as T22 (Batch 17, 22-teach-path-extraction.md); the full 8-context band's GO gate = T22's smoke re-run green on operative-tier sentinels. Work prompt: docs/plans/2026-06-12-t22-teach-path-extraction-work-prompt.md."
status_note_2026_06_12_clband_smoke: "CL-BAND SMOKE DONE (session work-2026-06-12-clband-smoke; report docs/assessments/2026-06-12-t14-clband-smoke.md). Lifecycle ran on both smoke contexts (flywheel 4.2k, aether 33.5k). OFF pre-gate: all 4 candidate siblings discriminate (OFF=loss; the discrimination the self-authored battery lacked). Teach Session A: 2 genuine captures, rules verifiably used. **HEADLINE FINDING (P0): both contexts FAIL the fidelity gate → INSTRUMENT-FAILURE(extraction), at BOTH sizes** — NOT the hypothesized size threshold. The extractor's recurrence+generalization design (correct for organic dogfood) blocks invented-rule capture: aether REFUSED ('fictional language, nothing would recur'→0 drafts); flywheel ABSTRACTED the invented SOP to generic principles (11 drafts, 0/4 sentinels). Session B not run (no rule-bearing skills). **GO/NO-GO: NO-GO for the full 8-context band** until a teach-mode/verbatim extraction path preserves invented specifics (fidelity_gate.sh is the ready acceptance test); all 8 would hit the same filter. Scope isolation (DP-2 marker-subdir) proven + dogfood 262 left pristine. 11 real .pending drafts produced+owner-accepted as >=10-draft AC evidence (labeled non-faithful-for-clband). No efficacy verdict (smoke != efficacy data); pre-registration untouched."
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

### CL Acquisition-Band Pre-Registration Deltas (LOCKED 2026-06-12 — committed BEFORE the smoke's first measured run; session work-2026-06-12-clband-smoke)

These fold the protocol plan's §6 deltas into this ticket verbatim-by-reference
(`docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md` §6). They lock task selection and the
no-verdict rules before any CL-band run produces data; changing them after band data exists VOIDS
the affected run (consistent with the LOCKED block above):

1. **Roster fixed.** The band roster (plan §3: 2 smoke + 8 full) and the alternate-substitution order
   (A1→A2→A3) are fixed. Substitution happens ONLY via a context losing all siblings to the OFF
   pre-gate (plan §4 Step 0) — never after paired data exists for that context.
2. **Instruments committed before their run.** Per measured sibling, the deterministic verifier core
   (≥5 checks), the de-referenced question rewrite, and the claude-CLI judge prompt (verbatim rubrics)
   are committed to the repo BEFORE that sibling's measured run; verifiers are unit-tested offline on
   a good/bad fixture pair first (Ralph Red/Green).
3. **INSTRUMENT-FAILURE taxonomy (two classes, distinct from the organic battery's α=0 analogue):**
   - `INSTRUMENT-FAILURE(extraction)` — a context whose manifest sentinels fail the post-extraction
     fidelity gate (plan §4 Step 3). P0 extraction finding with the context size attached; that
     context yields NO Session B data point and NO "layer doesn't help" reading.
   - `INSTRUMENT-FAILURE(injection/obedience)` — ON fails a sibling whose rule-bearing skills
     attribution confirms were injected (plan §4 Step 4). Distinct from the extraction class; blocks
     any efficacy verdict for that sibling until fixed.
4. **Solver checkpoint + dataset sha recorded per run** (CL-bench's non-pretrained property is current
   but expires with future checkpoints). A solver change re-runs the OFF pre-gate before results are
   comparable. Smoke session: `claude-code 2.1.173, --model sonnet`; dataset sha
   `b28a5832a09b0d96c0cf4c22e90d7c60ede25b80`.
5. **Injection mode labeled per run.** Each measured run records whether ON was fed via the focused
   inject-query workaround (`--inject-query summary`-class, the smoke's Finding-2 mitigation) or the
   production `compile_context`-on-prompt priming path. The smoke uses the labeled focused workaround;
   the production path is re-tested after T18/T12 and re-labeled.

**Smoke scope note:** the smoke (contexts #1–2) produces NO efficacy data — its outcomes are
pipeline-validation findings only; the ≥7/10 criterion stays untouched and unscored until the full
band runs (plan §5).

- [ ] Pass/fail criterion pre-registered in this ticket before any measured run; the final report cites it verbatim.
- [ ] Efficacy harness runs the same task set with layer ON and OFF against the live stack, paired per task.
- [ ] Committed deterministic scoring rubric with the judge prompt (if used) recorded in-repo.
- [ ] Reproducible report shows the measured delta (ON vs OFF) on ≥10 tasks, with per-task paired scores, win/loss/tie counts, and a sign test.
- [ ] Placebo arm (matched-mass irrelevant context) run and reported alongside ON/OFF; the ON-vs-placebo comparison is stated explicitly.
- [ ] Per-pull retrieval attribution (priming vs `find_skill`) captured and reported per task.
- [x] Draft-acceptance-rate over ≥10 `.pending` drafts from REAL captured sessions. — clband smoke: 11 real `.pending` drafts from the flywheel teach session (real captured session), 11/11 accepted (`tests/e2e/reports/efficacy/clband-smoke/draft_acceptance.json`); labeled non-faithful-for-clband (acceptance reflects general-skill plausibility, separate from the clband fidelity gate).
- [ ] Invented-rule positive-control task included (pre-registered interpretation as amended); its outcome reported distinctly; INSTRUMENT-FAILURE reported if ON loses it.
- [ ] Release gate includes an explicit, justified efficacy threshold; run reports PASS / FAIL / UNDERPOWERED.
- [ ] If the delta is null/negative/underpowered, documented in `docs/assessments/` with raw data.

### CL Acquisition-Band AUTO-GATE Amendment (LOCKED 2026-06-12 — committed BEFORE the first band datum; executed by T23, session work-2026-06-12-t23-band-run)

This amendment is legitimate **only because it lands before any paired band data exists** (the same
window in which the deltas above were locked; after the first band datum, nothing here may change or
the affected run VOIDS). It governs the FULL 8-context band run (the smoke produced no efficacy data,
so nothing is voided). Owner decision (2026-06-12, post-T22 GO): the band runs **fully automated and
unattended** — the owner will not manually review the ~150 benchmark `.pending` drafts the band
produces. The human gate is replaced, for the benchmark scopes only, by a pre-registered
auto-acceptance policy.

1. **`gate_mode = auto-accept-all`, `clband-*` benchmark scopes ONLY.** In the band run, plan §4
   Step 2's human gate is replaced by programmatic acceptance: **every** extracted `.pending` draft
   in a context's `clband-<name>` scope is accepted via the REAL acceptance action (rename
   `SKILL.md.pending` → `SKILL.md`, the structural definition in
   `scripts/efficacy_draft_acceptance.py`), followed by the real scope rebuild. Acceptance is uniform
   across all contexts and all arms; no draft is filtered. The production human gate and the 262
   **dogfood corpus are UNTOUCHED** — auto-accept fires only after a hard assertion that the target
   path lies inside a `clband-*` scope, and fails loud on any non-clband path. A post-run dogfood
   re-probe (corpus reads exactly 262, zero leakage) is a T23 acceptance criterion.
2. **Why accept-all, not a filter.** Any selective auto-filter would place an unvalidated judge
   inside the measured pipeline — unreproducible and confounding. Accept-all is the reproducible
   policy AND a conservative one: the ON arm faces the **unpruned** draft set (possible retrieval
   dilution / draft-count inflation — 19 drafts/context at the T22 smoke re-run), so if ON wins
   anyway the result is conservative relative to a human-gated production deployment.
3. **`gate_mode` recorded verbatim.** Every run report (and the morning verdict) cites
   `gate_mode=auto-accept-all (clband-* scopes only)` verbatim, and the auto-gate log lists every
   acceptance with its scope assertion. The reader can always tell the band's gate policy from
   production's.
4. **Roster + substitution unchanged from the deltas above.** The 8 full contexts + 3 ordered
   alternates (plan §3) are fixed; the ONLY substitution path is a context losing all siblings to the
   OFF pre-gate (plan §4 Step 0) → next alternate (A1→A2→A3). No change after paired data exists.
5. **Unattended continue/stop policy (pre-committed).** Overnight there is no STOP-and-ask:
   - HARNESS-LEVEL breakage (process crash, scope leak, `/health` failure, dataset-sha drift,
     auto-gate scope-guard trip) ⇒ STOP, preserve state + the last (context, step) checkpoint, write
     a morning stop report. The run is resumable without re-burning completed work.
   - Per-context INSTRUMENT-FAILURE (fidelity gate RED, or ON failing a sibling with
     attribution-confirmed injection) ⇒ record with the taxonomy (extraction vs injection/obedience),
     that context yields no efficacy point, and the band CONTINUES (contexts are independent).
   - An OFF pre-gate PASS (non-discriminating sibling) ⇒ drop that sibling; a context losing ALL
     siblings ⇒ substitute the next alternate (the only substitution path, per item 4).
   - Standing laws hold: no fakes (fail loud); measurement drives the REAL mcp-server over HTTP;
     heavy actions serialized by the orchestrator; never delete this run's outputs; never truncate
     graph_state; drain-until-done (no arbitrary time/token caps).
6. **Solver checkpoint re-pin.** The band runs on `claude-code 2.1.175, --model sonnet` (the smoke
   used 2.1.173 — a solver bump). Per the plan §1 expiry rule a solver change re-runs the OFF
   pre-gate before results are comparable; the band runs the OFF pre-gate fresh per context (plan §4
   Step 0), so this is satisfied by construction. Dataset sha
   `b28a5832a09b0d96c0cf4c22e90d7c60ede25b80` (fetch re-verifies on use; fails loud on drift).

This amendment changes ONLY the band's gate policy (human → auto-accept-all, clband scopes only) and
pre-commits the unattended run policy. It does NOT touch the ≥7/10 sign-test criterion, the
INSTRUMENT-FAILURE taxonomy, the roster, the instruments, or the dogfood/production gates.

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
