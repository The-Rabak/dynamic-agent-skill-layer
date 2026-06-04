---
ticket_id: T10b
title: First-run activation proof (doctor + demo + time-to-wow)
kind: hardening # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: in_progress # ready | in_progress | blocked | completed — impl landed session work-2026-06-04-074635; 2 fidelity concerns (degraded compile_context / inflated pending count) pending review+triage
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
assessment_ref: docs/assessments/2026-06-02-skill-layer-v1-5-current-state-assessment.md
source_packet_ref: "Post-T10 adoption addendum: first-run activation proof"
feature_home: scripts
depends_on: [T10]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-A (loop closes in the body)
  - SC-V1.5-B (self-growing trigger exists)
  - SC-V1.5-E (live suite is GREEN)
files:
  - scripts/doctor.sh
  - scripts/run-demo.sh
  - README.md
  - docs/reference/capability-catalog.md
  - docs/runbooks/degraded-state.md
  - tests/fixtures/retrieval_corpus.json
test_command: scripts/doctor.sh && scripts/run-demo.sh
tdd_mode: inherit
---

# First-run activation proof (doctor + demo + time-to-wow)

## Serves
- **SC-V1.5-A / B / E as adoption proof** — after the live suite is green, a fresh user can verify the local stack, seed/demo a realistic skill, see context injection, capture a transcript through the shipped ingress path, and observe a `.pending` draft without reading the test harness.
- Bridges V1.5 completion to V2 adoption by proving "works on `docker compose up`" as a human-readable workflow, not only a CI assertion.

## Scope
Add a small activation layer after the integration gate: a doctor script, a demo script, and quickstart docs that turn the green live system into a first-run wow path.

- **Owns:** first-run diagnostics, deterministic demo flow, README quickstart handoff, and time-to-wow measurement.
- **Non-goals:** new production feature surface, web dashboard, new MCP tools, V2 quality/scoring/optimization, or changing retrieval/extraction behavior.

## Scope Fence
This ticket runs **after T10**. It consumes the already-green live stack; it must not fix failing live tests, alter retrieval thresholds, edit port/env lines owned by T08, or redefine fixture corpus owned by T09. It may reuse T09's corpus and T10's report summary. Scripts are local developer aids only and must not become a second production control plane.

## Acceptance Criteria
- [ ] `scripts/doctor.sh` checks Docker/Compose availability, required env vars, PG/Redis/Qdrant REST endpoint, Qdrant gRPC endpoint when applicable, Ollama model availability, MCP `/health`, transcript-ingest secret posture, graph_version readability, and Claude Code hook config presence.
- [ ] `doctor.sh` reports clear `ok|warn|fail` lines with actionable next steps and exits non-zero only for blockers that prevent the demo from running.
- [ ] `scripts/run-demo.sh` reuses T09's realistic retrieval corpus, seeds at least two skills, calls `compile_context`, and prints the injected skill names plus deterministic "why this matched" reasons.
- [ ] Demo captures a transcript through the **shipped** command-hook/ingest queue path (or a script-compatible payload that exercises the same endpoint + queue drain) and proves a `.pending` draft lands on disk. No hand-built relative `transcript_ref` shortcut.
- [ ] Demo output explicitly states whether the default path made any cloud calls. Default should say: `cloud_calls: none` unless the user opted into a cloud extraction provider.
- [ ] Demo writes `tests/e2e/reports/activation-demo.md` with: stack health, seeded skills, compile_context status, graph_version, extraction/queue status, pending draft paths, elapsed time, and any warnings.
- [ ] README quickstart points at `scripts/doctor.sh` and `scripts/run-demo.sh` before deeper E2E commands.
- [ ] Time-to-wow target is recorded: fresh clone to first useful injected context in **<10 minutes excluding model download**. Script reports elapsed time so this remains measurable.
- [ ] Failure mode examples are documented in `docs/runbooks/degraded-state.md`: missing Ollama model, wrong Qdrant port, no ingest secret, MCP server down, no matching skills.

## Shared / Global Notes
- This is intentionally **post-gate**: T10 proves the system is correct under live pressure; T10b proves a new human can experience the product promise quickly.
- The demo corpus is not owned here. T09 owns `tests/fixtures/retrieval_corpus.json`; T10b consumes it.
- No human-gated infra config change is expected. If implementation touches compose/env defaults, stop and request approval.

## Local Context
**WHY:** The current repo has strong architecture and increasingly strong live tests, but a new user still has to infer the happy path from Docker Compose, fixtures, ignored tests, and reports. A first-run doctor/demo makes the system feel real: "it installed, found/seeded skills, injected context, captured a transcript, and produced a pending skill."

**Product insight (2026-06-02 assessment):** Correctness makes the system safe; activation makes people want it. This ticket is the smallest activation proof that preserves the no-dashboard, filesystem-first philosophy.

## Parent Refs
- Assessment → `docs/assessments/2026-06-02-skill-layer-v1-5-current-state-assessment.md` (Recommended Next Moves: add T10b Activation Proof).
- Consumes T08 port/suppression/body-limit fixes, T09 corpus/why-this-matched output, and T10 green live report.

## Deeper-Dive Refs
- README quickstart (`README.md`).
- Capability catalog (`docs/reference/capability-catalog.md`).
- Degraded-state runbook (`docs/runbooks/degraded-state.md`).

## Coupling Notes
Singleton post-gate ticket. It touches scripts/docs and consumes fixtures/reports from T09/T10. It must not run in parallel with T10 because T10 owns final runner/report shape and rollback-flag retirement. It should not touch production Rust unless doctor/demo discovers a missing public API; if that happens, stop and file a follow-up rather than expanding this ticket.
