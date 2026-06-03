---
ticket_id: T10
title: Turn the 12 live tests + DS-003…007 green and CI-gate them
kind: hardening # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: completed # ready | in_progress | blocked | completed (done 2026-06-04, session work-2026-06-03-230132, 3 sub-units)
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 3.3: Turn the 12 live tests + DS-003…007 green and CI-gate them"
feature_home: tests/e2e
depends_on: [T01, T02, T03, T05, T06, T08, T09]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-E (live suite is GREEN)
files:
  - tests/e2e/test_dream_state_contract.rs
  - tests/e2e/test_concurrency_stress.rs
  - tests/e2e/test_watcher_churn_reconciliation.rs
  - tests/e2e/test_live_data_plane_roundtrip.rs
  - tests/e2e/report.rs
  - scripts/run-e2e-tests.sh
  - .github/workflows/live-e2e.yml
test_command: ./scripts/run-e2e-tests.sh --include-dream
tdd_mode: inherit
---

# Turn the 12 live tests + DS-003…007 green and CI-gate them

## Serves
- **SC-V1.5-E** — `./scripts/run-e2e-tests.sh --include-dream` passes green on live containers, CI-gated. This is the final proof the whole loop closes.
- Plan SC-5/SC-7; assessment "RED ≠ GREEN".

## Scope
**Integration gate (NOT a standalone vertical slice).** With the upstream fixes in place, fix per-test assertion/timing issues, rewrite DS-003 to the Option-A contract, get green runs, and gate the suite in CI. Confirm the upstream fixes compose; do not re-do work owned by prior tickets.

- **Owns:** green live suite + CI gating + honest deferral notes + one human-readable run summary that makes the proof understandable outside the test harness.
- **Non-goals:** promoting DS-001/002/008–024 (V1.1-provable-later or V2); re-doing port/quality/usage work.

## Scope Fence
Only DS-003…007 + the 12 live tests are in V1.5's green bar. **MUST NOT start until T01, T02, T05, T06, T08, T09 are each individually green.** Touches `scripts/run-e2e-tests.sh` only to add the CI gate stanza — the port/env lines are owned by T08.

## Acceptance Criteria
- [x] `run-e2e-tests.sh --include-dream` is green on live containers. — Unit 1 (targeted live tests green under DEFAULT config: roundtrip 5/5, DS-003…007, concurrency burst, watcher churn) + full `--include-dream` proof (session work-2026-06-03-230132).
- [x] CI runs the live suite (or a documented subset) and fails on regression. — `.github/workflows/live-e2e.yml`: dedicated `live-e2e` job, service stack via `docker-compose.test.yml` + healthchecks, `--test-threads=1` (runner-owned for dream tests), `timeout-minutes: 20`, artifact upload on success AND failure. Non-zero suite exit fails the job. (First actual CI run executes on merge.) — Unit 3
- [x] **DS-003 rewritten to the Option-A contract:** Qdrant stopped ⇒ `compile_context` `Ok`/`NoMatch` AND `qdrant_write_side` degraded; not `#[ignore]`; no eager per-request Qdrant check. — Unit 1 (`dependency_chaos_matrix`; orchestrator re-verified live 5.12s)
- [x] Fix DS-004 restart/version monotonicity; DS-006/007 concurrency budgets. — Unit 1 (promoted dream contracts green: outbox_backlog_replays, qdrant_pg_drift, sustained_watcher, high_qps)
- [x] Replace fixed `sleep()` calls in dream tests with bounded readiness-polling (backoff, ~30s cap). — Unit 1
- [x] The bulk runner doesn't mix panic-stubs with promoted tests. — Unit 1 (promoted tests separated from `#[ignore]` dream-state stubs)
- [x] Every still-ignored contract has a one-line logged reason (no silent truncation). — Unit 1 (`#[ignore = "…"]` reasons) + Unit 2 (generator surfaces them, flags any missing reason)
- [x] `run-e2e-tests.sh --include-dream` emits `tests/e2e/reports/latest-summary.md` with status, p50/p95/p99 latency, graph_version progression, extraction attempts/completions, pending draft count, degraded/recovery events, ignored contracts+reasons. — Unit 2 (`scripts/generate-e2e-summary.py`) + Unit 3 (runner step)
- [x] CI uploads `latest-summary.md` as a report artifact on success and failure. — Unit 3 (`if: always()` artifact upload of summary + JSON)
- [x] **CI purity gate:** workflow runs `cargo tree -p domain`/`-p retrieval` and fails if `sqlx`/`redis`/`qdrant` appear. — Unit 3 (both currently PURE)
- [x] **Scope-line ownership (shared `run-e2e-tests.sh`):** appends ONLY the summary/CI step; T08's port/env lines unchanged (verified by diff). — Unit 3
- [x] **All env rollback flags removed** (`MCP_RETRIEVAL_MODE`, `MCP_GRAPH_REFRESH`, `MCP_USAGE_LOGGING`, `MCP_TRANSCRIPT_RECONCILE`→shipped as `MAINTENANCE_TRANSCRIPT_DRAIN` + `ScopeRoot` alias). — Unit 3 (`grep remove-after-v1.5-green` = 0; human-gate approved 2026-06-04)

## Shared / Global Notes
- **Infrastructure configuration change — HUMAN GATE:** CI workflow + the `run-e2e-tests.sh` CI stanza are infra-config; stage and confirm.
- **Flag-removal coordination:** this ticket retires the `// TODO(remove-after-v1.5-green)` flags introduced by T01/T02/T06/T07 — the single removal point so no flag is orphaned.
- File-conflict fence honored: T08 owns the port/env lines in `run-e2e-tests.sh`; this ticket only appends the CI gate stanza.
- `latest-summary.md` is deliberately owned by T10 rather than T10b: T10 proves the system takes a punch; T10b proves a new user gets a wow moment. The proof report belongs to the integration gate.

## Local Context
**WHY:** The live suite is RED by design and was blocked by the Qdrant port mismatch before reaching assertions. Once T08 (preflight/isolation), T09 (retrieval quality), and the Phase-1/2 wiring land, the remaining work is per-test assertion/timing fixes + CI gating + the DS-003 rewrite. This is an integration gate: it confirms the upstream fixes compose into a green `--include-dream` run.

**Adoption WHY (2026-06-02 assessment):** Raw JSON reports prove correctness to maintainers but not to future users or contributors. A compact markdown summary turns "system passed" into visible evidence: context injected, graph refreshed, extraction completed, latency stayed in budget, and remaining dream contracts were intentionally deferred.

**Open question to surface:** if a specific dream contract genuinely cannot be made deterministic in V1.5, keep it `#[ignore]` with an explicit logged reason rather than shipping a flaky gate — flag which one and why.

## Parent Refs
- Plan → Slice 3.3 (reframed as integration gate per architecture rec 1); Architecture artifact.
- Source packet: `## Execution Slices > Slice 3.3`.

## Deeper-Dive Refs
- Plan §Deepening Research Insights §3.3 + §3.1 (CI job shape; bounded readiness-polling).
- Plan WHY Reassessment R-5 (DS-003 rewrite); R-3 (roundtrip is Phase-3 evidence, not T01's).

## Coupling Notes
One unit because it is the single composition checkpoint for SC-E — splitting CI gating from the assertion fixes would let the suite pass locally but un-gated, or gate a still-RED suite. It is the terminal singleton batch: it hard-depends on every wiring/quality/harness ticket and is the one place V1.5's rollback flags are retired. No parallelism — it is the final gate.
