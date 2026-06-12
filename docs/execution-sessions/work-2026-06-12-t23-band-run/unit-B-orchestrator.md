---
unit: "Band orchestrator + auto-gate + scope guard"
unit_number: B
unit_kind: infra-packet
serves: "The driver that executes Steps 0–5 per context unattended + the auto-gate safety boundary that keeps the production gate and 262 dogfood corpus untouchable while the band auto-accepts in clband scopes only."
status: completed
attempt_count: 1
domains: [efficacy-harness, orchestration, scope-isolation, retrieval]
plan_file: docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/23-automated-clband-run.md
session_id: work-2026-06-12-t23-band-run
commits: [bb14c03, 032f040]
---

## What Was Implemented

### `scope_rebuild.py` — the clband retrieval mechanism + AUTO-GATE (canary-PROVEN live)
The smoke-proven (DP-2 Option A) path that makes an accepted clband skill retrievable, isolated, and
removable, with the safety boundary:
- clband skills live in named volume `dynamic-agent-skill-layer_test-project-skills` at
  `/skills/project/clband-<name>/{.git,.skills}`; volume writes go via one-off
  `docker run --rm -v <vol>:/skills/project alpine` (service mounts are `:ro`).
- `.git` marker makes `compile_context repo_path=/skills/project/clband-<name>` resolve to the subdir;
  retrieval filters by `source_path.starts_with(scope_path)` → ONLY that scope's skills.
- graph-builder polls the volume (~15s) → rebuild → PG/Qdrant → Redis `graph.rebuilt` → mcp-server
  `reload_and_swap` (ArcSwap, NO restart). `wait_retrievable` polls the REAL condition (no fixed sleep).
- **AUTO-GATE** `accept_all(name)`: asserts the `clband-*` scope guard before EVERY rename
  (`SKILL.md.pending` → `SKILL.md`), fails loud on dogfood/global/production paths.
- **LIVE-VALIDATED end-to-end** with a throwaway canary: write → accept → retrieve isolated (only the
  canary, project total 263) → remove → absent → **restored to exactly 262**. `scope_rebuild.py --canary`.
- `test_scope_rebuild.py`: 24 scope-guard unit tests green (accepts clband paths; rejects dogfood,
  global, traversal, unsafe names).

### `run_band.py` — two-pass Steps 0–5 orchestrator (unattended, resumable)
- **Pass 1** builds each context's isolated scope: Step 0 OFF pre-gate (each measured sibling bare;
  must FAIL to qualify — discrimination) → Step 1 teach (genuine claude-code solve) → Step 2 extract
  (`clband_extract.py`, claude-code provider, teach-doc delivery) → Step 3 fidelity gate (operative
  sentinels across drafts; RED ⇒ INSTRUMENT-FAILURE(extraction), no point, CONTINUE) → auto-gate
  accept + rebuild + `wait_retrievable`. Context #1 = the live canary.
- **Pass 2** measures surviving siblings: ON (compile_context from the context's OWN clband scope,
  focused inject-query) + PLACEBO (different context's scope, matched mass, rotation) + OFF (REUSED
  from the Step-0 pre-gate — identical bare solve, no duplicate). Verifier decides pass/fail; ON loss
  with the rule injected ⇒ INSTRUMENT-FAILURE(injection/obedience).
- **Checkpointed/resumable** per (context, step) via `checkpoint.json`. Unattended policy: harness
  breakage ⇒ STOP + stop report; per-context INSTRUMENT-FAILURE ⇒ record + CONTINUE; OFF-pass sibling
  ⇒ drop; context losing all siblings ⇒ substitute next INSTRUMENTED alternate, else continue with
  fewer. `gate_mode=auto-accept-all` + solver checkpoint + dataset sha recorded in band_results.json.
  Never deletes outputs (solve workspaces persisted under the band dir). `--plan` / `--closeout` modes.

### Supporting
- `merge_instruments.py` (the orchestrator instrument gate — see unit-A).
- `teach_delivery.py` generalized: full common-context union for full-band contexts, pinned single
  file for smoke (test_teach_delivery green).
- `INSTRUMENT_AUTHORING_SPEC.md` (the Unit A spec).

## Deviation from the workflow (noted)
The bulk of Unit B (the orchestrator + auto-gate) was built by the orchestrator directly rather than
delegated to an execution-agent, because (a) it is the corpus-safety boundary, (b) the orchestrator
holds the full smoke-proven mechanism in context, (c) the orchestrator drives + owns the run (Unit C)
and the standing rule serializes heavy actions through it, and (d) the safety-critical scope guard was
unit-tested + the whole path canary-validated live. Unit A (the parallelizable authoring) WAS delegated.

## Test Results
- `test_scope_rebuild.py`: 24 passed. `test_teach_delivery.py`: 4 passed. `efficacy_ab --self-test`: pass.
- Live canary (`scope_rebuild.py --canary`): PASS (write→accept→retrieve isolated→remove→restore 262).
- `run_band.py --plan`: resolves all 8 contexts + 12 siblings.
- Attempts: 1 (plus 2 review fixes: substitution-needs-instruments, retrieval-timeout→HarnessStop).
