---
unit: "Invented-rule task battery"
unit_number: 2
unit_kind: expansion
serves: "The measurable tasks — real corpus invented-rule + deterministic verifier per task; the α=0-analogue per-task sensitivity control"
status: completed
attempt_count: 1
domains: [efficacy, testing, corpus]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md
session_id: work-2026-06-12-122502-T14
---

## What Was Implemented
10 invented-rule task specs (`tests/e2e/efficacy/tasks/*.json`) each pointing at a REAL T10 corpus
skill (slug + live-resolved skill_id, semantic score ≥0.76), one deterministic shell verifier each
(`tests/e2e/efficacy/verifiers/*.sh`), and 20 good/bad fixtures proving each verifier discriminates
offline. Plus `tasks/README.md` (index + absent-from-pretraining rationale + skill-id provenance).

Tasks: all-migrations-dual-registration, backend-selector-fail-loud, arcswap-rcu-return-value,
cargo-required-features-check, env-var-fail-loud-all-binaries, claude-cli-fence-stripping,
cold-start-guard-retirement, rrf-score-not-exposed, anthropic-forced-tool-use, blank-env-treat-as-absent.
Dropped: read-before-write-file-edit (not generalizable), migration-file-unwired (dup).

## Files Changed
- tests/e2e/efficacy/tasks/*.json (10) + tasks/README.md — created
- tests/e2e/efficacy/verifiers/*.sh (10) — created, chmod +x
- tests/e2e/efficacy/fixtures/*/{good,bad}/ (20) — created

## Problems Encountered
### Problem 1: file-less `grep` stdin-readers (orchestrator-caught regression)
- **Error:** independent orchestrator sweep HUNG on `anthropic-forced-tool-use.sh` (good fixture).
- **Root cause:** 4 `grep -qE '...'` calls across 3 verifiers (anthropic ×2, blank-env ×1, rrf ×1) had
  NO file argument → read from stdin → block forever. 3 were redundant duplicates of the `"$file"`
  grep on the next line; rrf's was missing `"$file"`.
- **Fix (orchestrator-applied):** removed the 3 redundant stdin greps; added `"$file"` to rrf's grep.
  Re-ran the full sweep timeout-guarded + stdin=/dev/null — 10/10 OK, no hangs.

## Patterns Discovered
- Absence-checking verifiers are safer in embedded Python (docstring/comment stripping) than shell grep.
- A file-less `grep` in a verifier is a latent hang; ALWAYS pass the file and run verifiers with
  stdin=/dev/null + a timeout in any sweep harness.
- `find_skill` slug resolution needs the skill's own description verbatim or it returns a sibling skill.

## TDD Evidence
- **Red:** each verifier run against its `bad` fixture → non-zero (rule absent detected). 2 verifiers
  needed rewrites (claude-cli fence backtick parse error; cold-start docstring false-positive) before
  Red was stable.
- **Green:** each verifier against its `good` fixture → exit 0 (rule obeyed).
- **Post-Refactor Green:** full sweep after the orchestrator stdin-grep fix — 10/10 good→exit0,
  bad→non-zero, timeout-guarded, no hangs.

## Test Results
- Command: timeout-guarded verifier sweep (good→0, bad→nonzero) over all 10
- Result: PASS (10/10 OK after stdin-grep fix)
- Attempts: 1 (agent) + 1 orchestrator regression repair
</content>
