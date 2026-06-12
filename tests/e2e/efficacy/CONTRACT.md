# T14 Efficacy A/B Harness — Contract

This is the single source of truth for the task-spec schema, the verifier contract, the arm
definitions, and the gate semantics. The task battery (`tasks/`, `verifiers/`) and the runner
(`scripts/efficacy_ab.py`) both consume this contract. **Do not** drift from it.

WHY: T14 proves the one unmeasured number — does the skill layer make a coding agent measurably
better. The pre-registered design is an **invented-rule battery** (see the LOCKED block in
`docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md`).

## Standing rules this harness MUST honor

- **Measurement drives the real app.** Every arm drives the REAL running `mcp-server` over HTTP
  (`http://127.0.0.1:3001`) end-to-end via `claude-code`. No in-process snapshot/scoring reconstruction.
- **No fakes.** The placebo arm is an explicitly-labeled measurement control on the real stack, recorded
  as such in every report — never a silent fallback or a production fake. Verifiers enforce REAL
  assertions; there is no hardcoded `Passed`.
- **No arbitrary caps.** No time/token/turn cap that throttles legitimate work. Deadlines are stuck
  detectors only and must be labeled as such. (`--max-turns` for claude-code is sized to let a real solve
  finish, not to cap it; record the value used.)
- **Serialized heavy actions.** The live solve loop is run by the orchestrator, one solve at a time.

## Arm definitions

| Arm | Skill layer | How |
|-----|-------------|-----|
| `on` | enabled | `claude-code --settings <on-settings>` wiring `compile_context` SessionStart + UserPromptSubmit hooks against the live server (reuse `scripts/swebench/settings-swebench.json` shape). |
| `off` | disabled | identical `claude-code` invocation with NO skill-layer MCP server / hooks. |
| `placebo` | mismatched | `claude-code --settings <placebo-settings>` injecting matched-token-mass IRRELEVANT skill context (mismatched scope/corpus), explicitly labeled in the run record. |

The three arms must be otherwise byte-identical (same model, same prompt, same workspace base, same
`--max-turns`). The only intended difference is the skill-context injection.

## Task-spec schema (`tests/e2e/efficacy/tasks/<task_id>.json`)

```json
{
  "task_id": "kebab-case-id",
  "title": "Human-readable task title",
  "invented_rule": {
    "summary": "The project-specific rule the correct solution must obey, in one sentence.",
    "corpus_skill_slug": "<slug under replica-run/skills/.../.skills/>",
    "corpus_skill_id": "<live skill id resolved from the running server; see note>",
    "absent_from_pretraining_rationale": "Why no public pretrained model could know this rule."
  },
  "prompt": "The exact task prompt handed to the agent. Must be answerable correctly ONLY if the agent knows the invented rule; a generic-best-practices solve must fail the verifier.",
  "workspace": {
    "kind": "scratch | repo_checkout",
    "base_ref": "<git ref when kind=repo_checkout, else null>",
    "setup": ["shell commands run in the fresh workspace before the solve (may be empty)"]
  },
  "verifier": {
    "command": "tests/e2e/efficacy/verifiers/<task_id>.sh",
    "contract": "Invoked as `<command> <workspace_dir>`. Exit 0 == invented rule OBEYED (task win). Non-zero == rule NOT obeyed. MUST print a one-line human reason to stdout. NO network, NO LLM — pure deterministic inspection of the produced workspace/diff."
  },
  "expected": {
    "on": "pass",
    "off": "fail",
    "placebo": "fail",
    "sensitivity_note": "If ON fails this with the rule injected (attribution confirms the pull), that is INSTRUMENT-FAILURE."
  }
}
```

Notes:
- `corpus_skill_id` is resolved against the live server (e.g. via `find_skill` or the corpus inventory)
  so attribution can confirm the rule's skill was actually pulled in the ON arm. If the id cannot be
  resolved, fail loud — do not invent one.
- A task is only valid if a competent agent WITHOUT the rule plausibly fails the verifier. State that
  reasoning in `absent_from_pretraining_rationale`. Tasks where pretraining alone passes are rejected.

## Verifier contract (`tests/e2e/efficacy/verifiers/<task_id>.sh`)

- Pure deterministic inspection of `$1` (the post-solve workspace). Exit 0 = rule obeyed.
- Print exactly one human-readable reason line to stdout (win or loss explanation).
- No network calls, no model calls, no reliance on harness state. Re-runnable, idempotent.
- Must be unit-testable offline against a known-good fixture (rule obeyed → exit 0) and a known-bad
  fixture (rule absent → non-zero). Ralph Red/Green is proven on these fixtures.

## Per-pull attribution

For each ON/placebo solve, the runner records every retrieval pull labeled by trigger:
`session_start_priming` (SessionStart `compile_context`) vs `mid_session_find_skill` / `user_prompt`
(UserPromptSubmit). Source = the live server's request log / response markers. The per-task report shows
the pull list. Attribution is REQUIRED to interpret INSTRUMENT-FAILURE.

## Gate semantics (the runner emits exactly one verdict)

Pre-registered criterion (verbatim, the report must print this string):
> "ON wins ≥ 7 of 10 paired tasks by sign test, with no catastrophic regression on any single task."

- Per task: `on_win` iff ON verifier exit 0; same for `off`, `placebo`. Paired outcome per task ∈
  {ON>OFF win, OFF>ON loss, tie}.
- **PASS:** ON wins ≥ 7 of N(=10) paired tasks (sign test over non-ties) AND no catastrophic regression.
- **UNDERPOWERED:** ON direction positive but < the bar, OR sign test cannot distinguish at N.
- **FAIL:** ON ≤ OFF overall.
- **INSTRUMENT-FAILURE:** any task where ON fails with the rule injected (attribution-confirmed) →
  blocks any efficacy verdict; reported instead of PASS/FAIL/UNDERPOWERED.
- Reuse the paired/sign-test machinery from `scripts/retrieval_metrics.py` (T20) — do NOT grow a parallel
  stats implementation.

## Outputs

- Per-run JSON + a human report under `tests/e2e/reports/efficacy/<run-id>/`.
- The report names the arms, prints the pre-registered criterion verbatim, the per-task paired
  win/loss/tie table, the sign-test result, the ON-vs-PLACEBO comparison, the attribution per task, and
  the single verdict. Null/negative/underpowered results are documented honestly with raw data and
  mirrored into `docs/assessments/`.

## Draft-acceptance-rate (separate metric)

- `scripts/efficacy_draft_acceptance.py`: over ≥ 10 REAL `.pending` drafts from real captured sessions,
  compute accepted/total (accepted == a human renamed `SKILL.md.pending` → `SKILL.md`). Synthetic drafts
  are rejected. NOTE: the replica-run corpus currently has 0 remaining `.pending` (all accepted); the
  ≥10-draft input is sourced in the full-run follow-up from a fresh real capture. The scorer is built and
  unit-tested this batch; it fails loud if handed < 10 real drafts rather than reporting a fake rate.
</content>
