---
unit: "Live smoke run (orchestrator-serialized) + live-loop implementation"
unit_number: 4
unit_kind: hardening
serves: "e2e proof the harness moves end-to-end on the real stack + the OFF-side α=0 discrimination check"
status: completed
attempt_count: 1
domains: [efficacy, e2e, live-stack]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md
session_id: work-2026-06-12-122502-T14
---

## What Was Implemented (orchestrator — the live loop U3 deliberately left unimplemented)
Replaced the exit-1 stub with a real `run_live` loop in `scripts/efficacy_ab.py`: `materialize_workspace`,
`compile_context_http` + `find_skill_http` (live injection over HTTP, fail-loud), `run_claude_solve`
(claude-code subprocess, prompt via STDIN), harness-mediated injection + exact attribution, aggregate →
gate → report. Added `--inject-via {compile_context,find_skill}`, `--inject-query {prompt,summary,title}`,
`--server-url`, `--model`, `--max-tasks`, `--solve-timeout` (stuck-detector, not a cap).

## Live runs
- `smoke-probe-140528` (1 task, on,off, compile_context+prompt) — caught the stdin bug.
- `smoke-sensitivity-141049` (3 tasks × on,off,placebo, find_skill+summary) — **9/9 solves exit 0**;
  every pipeline stage works. Report at `tests/e2e/reports/efficacy/smoke-sensitivity-141049/`.

## Findings (full write-up: docs/assessments/2026-06-12-t14-efficacy-harness-smoke.md)
1. **Harness PROVEN end-to-end** (9/9 + diagnostics).
2. **P1 — tasks don't discriminate against Sonnet:** all arms win all tasks. Confirmed real (OFF produced
   the exact `Err(...)` fix). Cause: prompt leakage AND the rules are within Sonnet's default competence —
   a non-leaking re-authored prompt STILL had OFF win. The OFF-side α=0 control does not crater → battery
   has no measurement power yet. Per CONTRACT, tasks must be non-pretrained (OFF must fail).
3. **P1 — production `compile_context` priming no_matches verbose prompts** (qwen3 floor + length
   dilution); only focused queries retrieve. Needs an injection-query strategy / floor recalibration.

## Problems Encountered
### Problem 1: claude-code "Input must be provided" (both arms exit 1)
- **Root cause:** `--add-dir <ws>` is greedy and consumed the positional prompt as a second directory.
- **Fix:** pass the prompt via STDIN (`input=prompt`), drop the positional arg.

## TDD Evidence
- **Red:** `smoke-probe-140528` solve_exit=1 (claude got no prompt) — the missing behavior (a real solve).
- **Green:** after the stdin fix, `smoke-sensitivity-141049` 9/9 solve_exit=0, verifiers ran, report written.
- **Post-Refactor Green:** after adding inject knobs + live loop, regression guard re-run — 10/10 verifiers
  discriminate, 51 unit tests OK, `--dry-run` OK.

## Test Results
- Command: live smoke `efficacy_ab.py --arms on,off,placebo --max-tasks 3 --inject-via find_skill --inject-query summary`
- Result: harness PASS (9/9 solves, report emitted, gate=UNDERPOWERED honestly). Efficacy NOT measured
  (battery non-discriminating — documented, not spun).
- Attempts: 1 (after stdin fix)
</content>
