---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "promoted from todo #205 (P0)"
brainstorm_ref: null
started: 2026-06-12T12:25:02Z
status: in_progress
execution_shape: vertical-slices
current_unit: 3
total_units: 5
session_id: work-2026-06-12-122502-T14
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md (no brainstorm)
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: Prove the ONE unmeasured number — does the skill layer make a coding agent measurably better (the whole value proposition). Build the paired ON/OFF/placebo A/B harness with an explicit pre-registered efficacy gate.
- Success-criteria focus: T14 acceptance criteria — pre-registration, paired design + sign test, placebo arm, per-pull attribution, draft-acceptance-rate, invented-rule positive control, PASS/FAIL/UNDERPOWERED gate.

### TDD Contract
- Effective mode: Ralph-driven TDD (plan overrides local; both agree mode=ralph)
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: Unit (deterministic verifiers, stats/sign-test wiring, attribution parser, gate classifier) + e2e (live-stack smoke run that drives the REAL mcp-server over HTTP via claude-code; no in-process reconstruction)
- Exceptions: None. The live smoke run is orchestrator-executed and serialized (standing rule: no concurrent heavy actions in subagents).

### Constitution Context
- constitution_version 2.1.0; constitution_waivers: [] (none).
- No-stubs/fakes mandate (machine-wide + project): the placebo arm is an EXPLICITLY-LABELED measurement control on the real stack, recorded as such — never a silent fallback or a production fake. No hardcoded Passed; verifiers must enforce real assertions.
- Measurement-drives-real-app (standing rule): every measured arm drives the real running mcp-server over HTTP end-to-end. No in-process snapshot/orchestrator reconstruction.
- No-arbitrary-limits (standing rule): no time/token/poll caps on solves or churners; fail loud on real stuck states only.
- Pre-registration discipline (ticket + 2026-06-11 assessment): the pass criterion is committed BEFORE any data exists; changing it after data voids the run.

### Architecture Handoff
- Artifact: plan-derived handoff (no separate architecture artifact). Plan ## Non-Goals names efficacy as downstream; owner decision 2026-06-09 promoted it into the V1.7 set.
- Feature homes: efficacy harness in `scripts/` + `tests/e2e/` over the live stack; reports in `docs/assessments/` + `tests/e2e/reports/`. No production crate changes.
- Shared / global decisions: REUSE the T20 shared measurement lib (`scripts/retrieval_metrics.py`, `scripts/retrieval_sweep.py`) for paired/sign-test machinery instead of growing a parallel stats impl. REUSE the swebench claude-code `--settings` hook-wiring pattern (`scripts/swebench/settings-swebench.json`) for the "layer ON" arm.
- Deletion test: keep task specs concrete (real corpus skill ids + deterministic verifiers), not abstracted.
- Seams: "layer ON" = claude-code with skill-layer compile_context hooks; "OFF" = same agent, no hooks; "placebo" = matched-token-mass mismatched/irrelevant skill context, explicitly labeled.
- Review guidance: /workflows:review must verify no faked outcomes, real assertions in verifiers, honest UNDERPOWERED/INSTRUMENT-FAILURE handling, and that the pre-registered criterion is cited verbatim.

## Pre-Registered Experiment (LOCKED — committed before any data; changing it voids the run)
- **Design:** Invented-rule battery. Each task requires a project-specific rule/procedure that (a) exists as a skill in the T10 262-corpus and (b) is verifiably absent from model pretraining. Outcome scored by a DETERMINISTIC rule-obeyed verifier (not an LLM judge of "quality").
- **Arms (paired per task):** ON (skill layer compile_context hooks on the live server) / OFF (no skill layer) / PLACEBO (matched-token-mass irrelevant skill context on the real stack, explicitly labeled).
- **Pass criterion (verbatim):** "ON wins >= 7 of 10 paired tasks by sign test, with no catastrophic regression on any single task."
- **Three honest outcomes:** PASS (>=7/10 wins, no catastrophic regression) / UNDERPOWERED (positive direction but below the bar, or sign test cannot distinguish at N) / FAIL (ON <= OFF).
- **Per-task sensitivity / INSTRUMENT-FAILURE:** the invented-rule design IS the per-task sensitivity control (T11 α=0 analogue). If ON fails a task whose rule is present in the corpus AND was injected (attribution shows the pull), the harness/injection path is broken -> report INSTRUMENT-FAILURE; NO efficacy verdict may be claimed from the remaining tasks.
- **Concentration reading:** if ON wins ONLY the invented-rule-hardest tasks, report "value concentrates in non-pretrained knowledge" (strongest T12/CL-bench signal).
- **Placebo reading:** ON-vs-PLACEBO stated explicitly — separates "relevant skills help" from "any extra context helps/hurts."
- **Attribution:** every retrieval pull during measured runs labeled SessionStart-priming vs mid-session find_skill, per task in the report.
- **Draft-acceptance-rate:** measured over >= 10 real `.pending` drafts from REAL captured sessions (NOT synthetic). NOTE: the replica-run corpus has 0 remaining `.pending` (all 262 accepted) — the scorer is built this batch; its >=10-draft input is sourced in the full-run follow-up from a fresh real capture.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | Pre-registration + harness contract + task-spec schema | tracer-bullet | Integrity gate: locks the criterion before any data; defines the task-spec + verifier contract everything else consumes | completed | 1 | commit efa04ea |
| 2 | Invented-rule task battery (10 specs from real T10 skills) | expansion | The measurable tasks: real corpus rule + deterministic verifier per task | completed | 1+orch fix | unit-02 |
| 3 | 3-arm A/B runner + scoring + attribution + gate + draft-acceptance scorer | expansion | The harness code that drives the real server and emits the PASS/FAIL/UNDERPOWERED verdict | completed | 2 | unit-03 |
| 4 | Live smoke run (2-3 tasks x ON/OFF/placebo, serial, orchestrator) | hardening | e2e proof the harness moves end-to-end on the real stack + discrimination check | in_progress | -- | -- |
| 5 | Schedule full run follow-up + assessment doc + index/ticket/STATE update + commit | hardening | Honest handoff; keeps the source artifacts truthful | pending | -- | -- |

## Learnings Brief
_No learnings yet._
</content>
</invoke>
