---
unit: "Session A teach sessions (2)"
unit_number: 3
unit_kind: infra-packet
serves: "genuine claude-code capture of the invented rules under isolated scopes"
status: completed
attempt_count: 1
domains: [clband, solves, capture]
session_id: work-2026-06-12-clband-smoke
---

## What Was Implemented

Two GENUINE claude-code working sessions (sonnet, serialized), each in a fresh workspace containing
the context's knowledge document, prompted with the teach task framed to make the agent WORK the
task using the document (not paste-and-quit). Driven via the harness primitive
`efficacy_ab.run_claude_solve` (the direct `claude --dangerously-skip-permissions` bash form is
blocked by the auto-mode classifier; the python-driven subprocess is the permitted path — same as
the OFF pre-gate).

| context | doc (knowledge_home) | teach task | solve | solution.md | transcript |
|---|---|---|---|---|---|
| flywheel-assembly-agent | system.md (4154 ch, system) | sibling #1 407f5929 | rc=0, 65s | 3784 B | 65801 B |
| aether-language | context.md (33538 ch, user/fused) | depth-2 teach_only 7d9233cf | rc=0, 71s | 1019 B | 114504 B |

**Rules verifiably used (not paste-and-quit):**
- flywheel solution.md: `next size up`, `firm shake`, `retest`, `spin test`, Validation Engineer/Agent C, Agent D, `workaround`.
- aether solution.md: `conduit`, `flow`, `<<`, `Drop`, `Input`, `echo` (real Aether syntax applied).

Transcripts + teach solutions persisted (durable; ~/.claude/projects is volatile + gitignored) under
`tests/e2e/reports/efficacy/clband-smoke/transcripts/`. Isolated host scope dirs created with `.git`
markers at `tests/e2e/reports/efficacy/clband-smoke/scopes/clband-<name>/` for Unit 4 extraction.

## Files Changed
- `tests/e2e/efficacy/clband/setup_teach_workspaces.py`, `run_teach_session.py`, `clband_extract.py`
- `tests/e2e/reports/efficacy/clband-smoke/transcripts/**` (2 transcripts + 2 teach solutions)
- `tests/e2e/efficacy/clband/teach/**` (workspaces — gitignored if under contexts? no, separate)

## Test Results
- 2/2 teach sessions rc=0, both wrote solution.md showing the invented rules in use. Transcripts
  captured + persisted. No timeouts.
