---
unit: "Preflight + pre-registration deltas"
unit_number: 0
unit_kind: infra-packet
serves: "locks the experiment before any run; verifies fetch, stack, and scope mechanism"
status: completed-with-owner-gate
attempt_count: 1
domains: [efficacy-harness, scope-isolation, docker, retrieval]
plan_file: docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md
session_id: work-2026-06-12-clband-smoke
---

## What Was Implemented

**0.1 — Pre-registration deltas LOCKED into the T14 ticket** (committed before any measured run).
Added a `### CL Acquisition-Band Pre-Registration Deltas (LOCKED 2026-06-12 ...)` block folding the
protocol plan §6 deltas verbatim-by-reference: roster fixed; instruments committed before their run;
INSTRUMENT-FAILURE taxonomy (extraction vs injection/obedience); solver checkpoint + dataset sha
recorded (`claude-code 2.1.173, --model sonnet`; sha `b28a5832...`); injection mode labeled per run;
smoke produces no efficacy data.

**0.2 — Contexts fetched + sentinels verified (fresh).** `fetch_clband_contexts.py --only 7833ca0b
bc874bce`:
- `flywheel-assembly-agent`: 12 tasks (12 held-out-capable), knowledge in **system** (4154 chars), sentinels OK (4).
- `aether-language`: 3 tasks (2 held-out-capable), knowledge in **user** (33538 chars), sentinels OK (4).
- No drift warning ⇒ live dataset sha still matches the pin `b28a5832a09b0d96c0cf4c22e90d7c60ede25b80`.

**0.3 — Live stack readiness.** `/health` 200; all containers healthy; `embedding_arm
model=qwen3-embedding:4b dim=2560 collection=skills__qwen3-embedding-4b`; `retrieval_backend
backend=snapshot_dense`. T17 honesty gate satisfied.

**0.4 — Scope-isolation mechanism (the DP-2 finding).** Investigated with file:line evidence (two
Explore sweeps). Result: the plan's "one isolated project scope per context, separate from the 262
dogfood corpus" is **NOT natively supported by current code**. Details below.

## Scope mechanism — evidence

- **Corpus location:** the 262 dogfood skills live in the **project** scope at
  `/skills/project/.skills/<slug>/SKILL.md` (`SKILL_PROJECT_ROOT=/skills/project`,
  docker-compose.test.yml:156). `/skills/global` is **empty**.
- **Retrieval scope resolution** (`crates/infrastructure/src/scope.rs:79-134`): given a `repo_path`,
  walk up for `.git`/`SKILL_PROJECT_MARKER`; the first marker dir becomes the scope, with
  `paths=[that dir]`. `scope_id` is **always** the literal `"project"` (scope.rs:45) — isolation is
  **purely by `source_path.starts_with(scope_path)` prefix** (`crates/retrieval/src/dual_scope.rs:261-265`),
  NOT by scope_id. If `repo_path` doesn't resolve, falls back to `SKILL_PROJECT_ROOT=/skills/project`.
- **graph-builder builds exactly ONE project scope** rooted at `GRAPH_BUILDER_PROJECT_ROOT=/skills/project`
  + one global scope (`crates/graph-builder/src/main.rs:31-50`). **No sub-marker discovery.** Every
  `SKILL.md` under the project root is assigned scope_id="project" with `source_path` = full SKILL.md
  path (`build.rs:117`); `scope_for_path` is plain prefix matching (`watcher.rs:355-366`). Full
  rebuild on every change, polls every `GRAPH_BUILDER_POLL_INTERVAL_MS` (default 15s); manual rebuild
  via admin `trigger_full_rebuild`.
- **`find_skill` is UNSCOPED** — hardcoded `retrieve(prompt, None)` (`find_skill.rs:105`); only
  `compile_context` honors `repo_path` (`compile_context.rs:133`). ⇒ the smoke MUST inject ON/PLACEBO
  via `--inject-via compile_context` (with a clband `repo_path`), never `find_skill`.
- **No mechanism** for multiple isolated project scopes in one container; retrieval hardcodes
  `configured_scope_ids() == ["project","global"]` (`scope_resolution.rs`).

## Consequence (why this is DP-2)

Isolated READS (what the experiment actually needs — ON from the context's own scope, PLACEBO from
the OTHER context's scope, neither containing dogfood) ARE achievable WITHOUT code change via
marker-isolated subdirs + clband-scoped `compile_context` queries:
- place flywheel skills under `/skills/project/clband-flywheel-assembly-agent/.skills/...` + a `.git`
  marker at that subdir root; same for aether. `compile_context repo_path=<clband subdir>` then
  resolves to that subdir and prefix-matching returns ONLY that context's skills (dogfood excluded;
  the two clband contexts mutually excluded). This gives clean ON + cross-scope PLACEBO reads.
- **Residual impurity:** because graph-builder folds everything under `/skills/project` into the one
  "project" scope, a BROAD `repo_path=/skills/project` (or unresolved-path fallback) query would see
  the clband skills too — i.e. clband would leak into the *dogfood* scope for broad queries. The smoke
  runs **no** broad/dogfood queries, and closeout removes the clband subdirs + rebuilds to restore the
  pure 262 — but persisting clband under the dogfood tree touches the "don't contaminate the 262 /
  T18 substrate" fence, so the convention is an **owner call (DP-2)**, not a unilateral hack.

## Files Changed
- `docs/tickets/.../14-efficacy-task-outcome-ab-harness.md` — LOCKED CL-band pre-reg deltas block.
- `docs/execution-sessions/work-2026-06-12-clband-smoke/STATE.md` + this file.

## Test Results
- fetch sentinel verification: PASS (both smoke contexts, 4 sentinels each).
- /health: 200, qwen3 arm, snapshot_dense.
- Scope investigation: read-only; no heavy actions.
