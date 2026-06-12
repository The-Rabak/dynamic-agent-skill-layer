---
unit: "Instruments at scale — 8 full contexts"
unit_number: A
unit_kind: infra-packet
serves: "Per-context measurement eligibility: deterministic verifiers + de-referenced tasks + judge prompts + operative sentinels, committed BEFORE each context's measured run (the pre-registration fence)."
status: completed
attempt_count: 1
domains: [efficacy-harness, instruments, verifiers, sentinels]
plan_file: docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/23-automated-clband-run.md
session_id: work-2026-06-12-t23-band-run
commit: dddeb82
---

## What Was Implemented

For each of the 8 full-band contexts, the measured instruments (mirroring the smoke's
`author_smoke_instruments.py` + `verifiers/flywheel-assembly.sh` + two-tier sentinels):
- a deterministic **verifier** (`verifiers/<name>.sh`, ≥5 checks compiled VERBATIM from the CL-bench
  rubrics in `tasks.json`; knowledge rubrics only — persona/format dropped to the judge),
- **good/bad fixtures** self-tested (good→exit 0, bad→non-0; the Ralph RED/GREEN),
- 1–2 **de-referenced measured task specs** (`tasks/clband-<name>-<short8>.json`, system-naming frame +
  verbatim question + workspace instr, prior_turns dropped/inlined for self-containment),
- a **judge prompt** (`judge/...md`, verbatim rubrics, secondary score),
- a **teach workspace** (`teach/<name>/{doc,prompt.txt}`),
- **operative + document sentinels** (`instruments/<name>.json`), operative derived verbatim from the
  verifier checks.

### Method — 8 parallel sonnet execution-agents + an orchestrator gate
Authored by 8 PARALLEL `compound-engineering:workflow:execution-agent` runs (sonnet, file-disjoint per
context: each owns `verifiers/<name>.sh`, `fixtures/<name>-*`, `tasks/clband-<name>-*`, `judge/...`,
`teach/<name>/`, `instruments/<name>.json`). Each agent was fenced: NO cargo/docker/mcp/model-storms,
NO shared-file edits. Shared spec: `INSTRUMENT_AUTHORING_SPEC.md`.

The orchestrator then ran the AUTHORITATIVE gate `merge_instruments.py` (does NOT trust self-reports):
re-verified every operative sentinel verbatim against the COMMON context text (system.md+context.md
union — zero hallucinations), re-ran every verifier on its fixtures, confirmed task specs + judge +
teach + ≥5 checks, then merged sentinels into `manifest.json`.

### Result (12 measured siblings across 8 contexts)
| context | doc | measured siblings | operative sentinels |
|---|---|---|---|
| material-handler-sops | system.md | 2 | 7 |
| source-integrity-agent | system.md | 2 | 9 |
| quartermaster-hold-inventory | system.md | 2 | 8 |
| dartman-game | context.md | 1 (depth-8 self-contained) | 7 |
| ezlang-language | context.md | 1 (depth-4 inlined) | 5 |
| drywave-3000-manual | context.md | 1 (depth-6) | 8 |
| 123corp-hr-policy | context.md | 1 (depth-6) | 5 |
| dpms-agent-m | system.md | 2 | 8 |

All `self_test`: good_exit=0, bad_exit=1, sentinels_verified=true. `run_band.py --plan` resolves all 8.

## Watch-items (carried into the run)
- **dpms-agent-m** is the highest-risk context: its specific invented codes (M-WARN-01, schema 4.2,
  Agent F posterior) are NOT in EITHER context file — they live in the task scenarios. Its operative
  sentinels are doc-grounded (DPMS, RISK_CLASSIFICATION, VIOLATION_CODES…), so the fidelity gate is a
  looser check for dpms; the VERIFIER still enforces the specific codes. Mitigation: `teach_delivery`
  now delivers the full common-context union; the teach TRANSCRIPT carries the scenario codes. If
  extraction still misses them → recorded as a per-context finding, band continues.
- Alternates (shelbys/agent04/micro-moonshine) were NOT instrumented (8 full only). A full context
  failing the OFF pre-gate entirely is recorded + the band continues with fewer contexts (substitution
  needs instrumented alternates, which don't exist) — honest, N still up to 12.

## Files Changed
- `tests/e2e/efficacy/clband/INSTRUMENT_AUTHORING_SPEC.md` (shared spec)
- `tests/e2e/efficacy/clband/merge_instruments.py` (orchestrator gate; committed under Unit B)
- `verifiers/<8>.sh`, `fixtures/<8>-{good,bad}/solution.md`, `tasks/clband-*<12>.json`,
  `judge/clband-*<12>.md`, `teach/<8>/` (gitignored), `instruments/<8>.json`, `manifest.json`

## Test Results
- merge_instruments gate: 8/8 contexts PASS (every operative sentinel verbatim; every verifier good=0/bad≠0).
- Independent orchestrator re-run of 3 verifiers: all good=0/bad=1.
- Attempts: 1
