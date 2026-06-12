---
source_type: plan
plan_file: docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md
work_prompt: docs/plans/2026-06-12-t14-clband-smoke-work-prompt.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
brainstorm_ref: (none — owner-directed plan is canonical WHY)
started: 2026-06-12
status: in_progress
execution_shape: measured-experiment (plan §4 per-task lifecycle is the unit contract; not vertical-slices)
current_unit: 0
total_units: 7
session_id: work-2026-06-12-clband-smoke
solver_checkpoint: "claude-code 2.1.173, --model sonnet"
dataset_sha: b28a5832a09b0d96c0cf4c22e90d7c60ede25b80
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md (owner-directed)
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: prove the teach→extract→inject→verify pipeline end-to-end on two genuinely
  novel CL-bench contexts (4.2k flywheel + 33.5k aether), GATING the full 8-context efficacy band.
- Success-criteria focus: pipeline-validation only (smoke ≠ efficacy data); the ≥7/10 pre-registered
  verdict machinery stays untouched and unscored this session.

### TDD Contract
- Effective mode: Ralph-driven (verifiers unit-tested offline on good/bad fixtures BEFORE any run).
- Effective loop: failing fixture → deterministic verifier → green on good/bad pair → commit → use.
- Required evidence: per-sibling verifier good/bad fixture results; OFF pre-gate raw outputs; fidelity
  gate grep output; Session B attribution + secondaries; every number tied to a persisted artifact.
- Exceptions: no efficacy verdict may be derived (pre-registration forbids it for a smoke).

### Constitution Context
- No docs/constitution.md governs this ticket. Binding law: the LOCKED Pre-Registration block + the
  2026-06-12 amendment + the CL-bench policy bullet in the T14 ticket; the prompt's Hard Fences;
  machine-wide no-fakes/fail-loud rule.

### Architecture Handoff (plan-derived)
- Artifact: docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md §4 (per-task lifecycle).
- Feature homes: scripts/ (fetch + harness), tests/e2e/efficacy/clband/ (band), tests/e2e/reports/efficacy/ (artifacts), docs/assessments/ (report).
- Shared/global: the live mcp-server (HTTP), the 262 dogfood corpus (MUST NOT be contaminated).
- Per-context scope: one skill-layer project scope per context (clband-<name>), isolated.
- Seams this session honors: measurement drives the REAL server over HTTP; no in-process reconstruction;
  no crate/ranking/floor changes (that is T18/T12); focused inject-query workaround LABELED per run.
- Review guidance for later /workflows:review: verify no efficacy verdict spun; provenance fence intact
  (no planting); scope isolation proven by probe; every report number has a raw artifact.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 0 | Preflight + pre-reg deltas commit | infra-packet | locks the experiment before any run | completed | 1 | unit-00-preflight.md |
| 1 | Verifier + rewrite + judge authoring | infra-packet | committed instruments before runs | completed | 1 | unit-01-instruments.md |
| 2 | OFF pre-gate | fix-item | empirical non-pretraining + discrimination | completed | 1 | unit-02-off-pregate.md |
| 3 | Session A teach sessions (2) | infra-packet | genuine capture under isolated scopes | completed | 1 | unit-03-teach-sessions.md |
| 4 | Pipeline + human gate + fidelity gate | infra-packet | proves extraction at both sizes | in_progress | 1 | -- |
| 5 | Session B paired ON/OFF/PLACEBO | infra-packet | end-to-end injection + placebo | pending | -- | -- |
| 6 | Report + closeout | infra-packet | GO/NO-GO for the full band | pending | -- | -- |

## Learnings Brief
- [scope] **DP-2 RESOLVED (owner: Option A).** Isolate clband via marker subdirs under the dogfood
  project volume: `/skills/project/clband-<name>/{.git,.skills}`. graph-builder folds all of
  `/skills/project` into one "project" scope, but retrieval filters by `source_path.starts_with(repo_path)`,
  so `compile_context repo_path=/skills/project/clband-<name>` returns ONLY that subdir's skills.
  PROVEN live (empty scope → status `no_match`, 0 skills, zero dogfood leak; broad `/skills/project`
  → `ok` w/ dogfood). Rules: ON/PLACEBO inject via `compile_context` ONLY (find_skill is unscoped);
  run NO broad queries during the window; closeout = remove clband subdirs + rebuild + re-probe to
  restore pure 262. Volume write = one-off `docker run --rm -v <vol>:/p alpine` (mounts are :ro to
  services). Volume = `dynamic-agent-skill-layer_test-project-skills`.
- [harness] `scripts/efficacy_ab.py` `compile_context_http` builds `repo_path=/tmp/t14-...` internally;
  for clband ON/PLACEBO I must thread the clband subdir path through (small additive Python knob —
  allowed; not a crate/ranking change). ON = focused `--inject-query summary`, LABELED.
- [extraction] To capture Session A into a clband scope, the extraction `repo_path` must point at the
  clband subdir so `.pending` drafts land under `/skills/project/clband-<name>/.skills/`.
