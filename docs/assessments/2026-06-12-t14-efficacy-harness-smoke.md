# T14 Efficacy A/B Harness — Build + Smoke Run (2026-06-12)

Session: `work-2026-06-12-122502-T14`. Owner scope (pre-registered): **build the harness + a 2–3 task
smoke proof on the live stack; the full ≥10-task run is a follow-up.** Driver: `claude-code` / Sonnet,
serialized by the orchestrator.

This is an **honest negative-leaning result**: the harness is proven end-to-end, but the smoke shows the
current task battery cannot yet measure efficacy against Sonnet. No efficacy verdict is claimed (and the
pre-registration forbids claiming the 7/10 verdict from a partial run).

## Pre-registered criterion (verbatim — cited per AC)

> "ON wins ≥ 7 of 10 paired tasks by sign test, with no catastrophic regression on any single task."

Outcomes: PASS / FAIL / UNDERPOWERED / INSTRUMENT-FAILURE. Locked in the T14 ticket before any data.
Design: invented-rule battery; arms ON / OFF / PLACEBO (matched-mass irrelevant context); deterministic
rule-obeyed verifiers; per-pull attribution.

## What was built (Units 1–3)

- `tests/e2e/efficacy/CONTRACT.md` — task-spec + verifier schema, arm defs, gate semantics.
- `tests/e2e/efficacy/tasks/*.json` (10) + `verifiers/*.sh` (10) + good/bad fixtures (20). Each verifier
  discriminates offline (good→exit 0, bad→non-zero); 10/10.
- `scripts/efficacy_ab.py` — 3-arm runner: workspace materialization, live-server injection (harness-
  mediated `compile_context` / `find_skill` over HTTP), claude-code solve, verifier scoring, per-pull
  attribution, PASS/FAIL/UNDERPOWERED/INSTRUMENT-FAILURE gate (reuses T20 `retrieval_metrics.sign_test`),
  report emitter. `--dry-run` + 51 unit tests.
- `scripts/efficacy_draft_acceptance.py` — draft-acceptance scorer (fails loud on <10 real drafts).

## What the smoke PROVED (harness end-to-end)

Run `smoke-sensitivity-141049`: 3 tasks × {ON, OFF, PLACEBO} = **9/9 solves exit 0**. Every pipeline
stage works on the live stack: workspace materialize → live-server injection over HTTP → real Sonnet
solve → deterministic verifier → attribution capture → gate → report
(`tests/e2e/reports/efficacy/smoke-sensitivity-141049/`). The stdin-prompt bug (`--add-dir` greedily
swallowed a positional prompt) was found and fixed (prompt now passed on stdin).

## What the smoke DISCOVERED (full-run blockers)

### Finding 1 (P1) — the task battery does not discriminate against Sonnet

All three arms won all three tasks. Confirmed **real, not a verifier false-positive**: for
`backend-selector-fail-loud`, the OFF arm (no skill layer) produced exactly
`Err(SearchError("QdrantHybrid search is not yet implemented"))` and passed the verifier.

Two compounding causes:
1. **Prompt leakage** — several task prompts state the required fix (e.g. "Fix the QdrantHybrid arm so
   that callers receive an explicit error … rather than silently getting dense results"). A generic solve
   obeys without any skill.
2. **The rules are within Sonnet's default competence** — and this is the deeper problem. With a
   **re-authored non-leaking prompt** ("Make the QdrantHybrid arm correct for THIS codebase's
   conventions"), OFF *still* returned `Err(...)` — returning an error for an unimplemented arm is
   Sonnet's natural default. Removing leakage did not create discrimination.

This is the **α=0 analogue failing**: the instrument's negative arm (OFF) must crater for the battery to
have measurement power. It doesn't. Per the CONTRACT, a valid task must be **answerable correctly ONLY
with the injected rule** and **verifiably absent from pretraining** — these tasks fail that bar against
Sonnet. (This empirically confirms the project's own CL-bench thesis: generic "good-practice" rules can't
show efficacy because strong models already know them; the layer's value must be measured on genuinely
non-pretrained, project-idiosyncratic knowledge where the base model is wrong by default.)

### Finding 2 (P1) — production priming path no_matches verbose prompts

`compile_context` (and `find_skill`) return `status=no_match` / `reason_code=no_relevant_skills` for the
full task prompts, but retrieve strongly for focused queries:

| Query for `all-migrations-dual-registration` | Result |
|---|---|
| full task prompt (verbose, code blocks) | `no_match`, 0 skills |
| task title | `ok`, `migration-file-unwired-from-registry` @ 0.651 |
| rule summary | `ok`, `migration-file-unwired-from-registry` @ 0.779 |

The retrieval brain works (T11-validated), but the 0.48 relevance floor (calibrated for nomic) plus
prompt-length dilution under qwen3's compressed score scale (~0.016–0.78 range) means the **as-shipped
SessionStart `compile_context` priming does not fire for realistic verbose prompts.** This corroborates
the standing note `qwen3-default-operational-findings` (scores compressed, threshold needs recalibration).

## Verdict

- **Harness: VALIDATED end-to-end.** Ready to run the full battery the moment the tasks discriminate.
- **Efficacy: NOT MEASURED.** No PASS/FAIL/UNDERPOWERED efficacy claim — the smoke shows the battery
  cannot yet separate ON from OFF against Sonnet. Reporting a tie here as "no effect" would be dishonest;
  it is an **instrument-not-yet-sensitive** result.

## Prerequisites for the full ≥10-task run (the follow-up)

1. **Re-author the battery for genuine non-pretrained discrimination.** Each task's correct answer must be
   a project-idiosyncratic rule the base model gets WRONG by default (OFF must demonstrably fail on a
   pre-run OFF-only validation pass — the harness's α=0 gate). Prompts must pose the problem WITHOUT
   stating the rule. Candidate-strong rules: ones where the model's default actively contradicts the
   project convention (e.g. dir-scan migrations vs this repo's compile-time array; RRF score vs eq.3
   `#260` exposure), not ones where the model's default already complies.
2. **Add an OFF-only discrimination pre-gate to the harness:** before a measured run, solve every task
   with OFF; any task OFF passes is rejected as non-discriminating (this is the per-task α=0 control,
   the dual of INSTRUMENT-FAILURE).
3. **Decide the ON injection-query strategy** (and likely recalibrate the qwen3 `compile_context` floor),
   since the production priming path no_matches verbose prompts. Options: focused-query injection,
   floor recalibration for qwen3, or measuring mid-session `find_skill` usage explicitly.
4. **Draft-acceptance-rate:** source ≥10 real `.pending` drafts from a fresh capture (the replica corpus
   has 0 remaining — all 262 accepted). Scorer is built and fails loud on <10.

## Acceptance-criteria status

- [x] Pass criterion pre-registered verbatim before any run; cited here.
- [x] Harness runs the same task set ON/OFF/placebo against the live stack, paired per task (proven 9/9).
- [x] Committed deterministic scoring rubric (verifiers, 10/10 discriminate offline).
- [x] Reproducible report with per-task paired table + sign test (`smoke-sensitivity-141049`).
- [x] Placebo arm wired + reported (matched-mass irrelevant control); ON-vs-placebo stated.
- [x] Per-pull attribution captured + reported.
- [x] Invented-rule design with per-task sensitivity (INSTRUMENT-FAILURE) — but the OFF-side α=0 control
      revealed the tasks are non-discriminating against Sonnet (Finding 1).
- [x] Release gate emits PASS/FAIL/UNDERPOWERED; smoke = harness-validated, efficacy NOT measured.
- [ ] **Full ≥10-task efficacy verdict — DEFERRED to the follow-up, blocked on prerequisites 1–4 above.**
- [ ] Draft-acceptance over ≥10 real drafts — scorer built; input deferred to a fresh capture.

## Reproduce

```bash
# Harness validation (offline, no model calls)
python3 scripts/efficacy_ab.py --self-test
python3 scripts/efficacy_ab.py --dry-run --tasks tests/e2e/efficacy/tasks/

# Live smoke (sensitivity config — find_skill+summary injection), serialized:
python3 scripts/efficacy_ab.py --run-id <id> --tasks tests/e2e/efficacy/tasks/ \
  --arms on,off,placebo --max-tasks 3 --inject-via find_skill --inject-query summary \
  --model sonnet --max-turns 30
```
</content>
