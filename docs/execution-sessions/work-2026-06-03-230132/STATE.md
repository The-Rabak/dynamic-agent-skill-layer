---
source_type: ticket-index
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_index: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/10-green-live-suite-and-ci-gate.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
source_packet_ref: "## Execution Slices > Slice 3.3: Turn the 12 live tests + DS-003…007 green and CI-gate them"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-06-03T23:01:32Z
status: in_progress
execution_shape: vertical-slices
current_unit: 3
total_units: 3
session_id: work-2026-06-03-230132
batch: 9
ticket: T10
status_note: all 3 units completed + unit-04 (made the full run-e2e-tests.sh --include-dream run end-to-end GREEN 18/18 by fixing pre-existing Docker/runner/test breakage). Full wrapper proof done. PR pending.
---

## WHY Context

### Problem Narrative
The V1.5 live E2E suite is RED by design and was blocked before reaching assertions (Qdrant port mismatch, fixed by T08). With all upstream wiring/quality/harness tickets (T01–T03, T05, T06, T08, T09) green, the remaining work is the integration gate: confirm the upstream fixes compose into a green `run-e2e-tests.sh --include-dream`, prove the CQRS read-model takes a punch (DS-003), and gate the proof in CI. Until this gate is green, the "close the loop" release cannot claim it works on `docker compose up`.

### User Story
As a solo developer who deploys the skill layer with `docker compose up`, I need the whole live data plane proven green — not a test written to fail — so that I can trust the deployed system retrieves real skills, self-grows from sessions, and demonstrably takes a punch (Qdrant down ⇒ read path still serves), with that proof readable outside the test harness and CI-gated against regression.

### Architectural Context
- Integration gate (NOT a standalone vertical slice). Composes 7 upstream slices; no independent demo.
- Option A (CQRS read model): online reads = PG-loaded `RetrievalSnapshot`; Qdrant is the durable write-side store, never queried at read time. DS-003 must prove this: Qdrant down ⇒ `compile_context` still `Ok`/`NoMatch`, only the write-side health marker degrades.
- `domain`/`retrieval` purity invariant: no `sqlx`/`redis`/`qdrant` may leak in (CI purity gate enforces via `cargo tree`).
- Env rollback flags are "keep-with-expiry": removal criterion is "first green CI on main" — T10 is the single removal point so no flag is orphaned.

### Success Criteria
- SC-V1.5-E: `./scripts/run-e2e-tests.sh --include-dream` passes green on live containers, CI-gated; the 12 live tests + DS-003…007 green; Qdrant port + suppression-isolation defects fixed (done upstream).
- SC-V1.5-F (partial): `cargo clippy` strict + `rustfmt` pass (T10 owns the residual e2e + protocol.rs clippy cleanup).

### TDD Contract
- Effective mode: Ralph-driven TDD.
- Effective loop: failing tests first → minimal implementation → refactor → post-refactor rerun.
- Required evidence: Unit (`cargo test --workspace`) Red→Green→post-refactor Green for each new/changed behavior, + E2E (`./scripts/run-e2e-tests.sh --include-dream` against live containers).
- Exceptions: none. An honest `#[ignore]` is allowed only with a one-line logged reason (no silent truncation).

### Constitution Context
- Constitution v2.1.0. Relevant: Local-first (data plane stays local; no cloud on default path), Zero-touch (<500ms — no eager per-request Qdrant check in DS-003), Human gate, No stubs (strict clippy/fmt), Quality Standards (Docker Compose E2E, strict clippy, rustfmt, <500ms bench).
- **Human gates (stop and confirm before commit):** CI workflow `.github/workflows/live-e2e.yml` + the `run-e2e-tests.sh` CI gate stanza are infrastructure-config changes → stage and confirm.
- Waivers: none.

### Architecture Handoff
- Artifact: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
- Feature homes: tests/e2e (owned by T10) + scripts/ + .github/. Touches mcp-server/infrastructure only for the teardown-deadlock seam (T06/teardown) and the protocol.rs strict-clippy residue.
- Shared / global decisions: Option A CQRS read model; `SkillRetriever` trait is the stable retrieval seam; usage-persistence seam writes async-off-response-path with observable failure.
- Deletion test: env rollback flags KEEP-WITH-EXPIRY → removal criterion "first green CI on main" reached here; T10 removes all 5.
- Interfaces as test surfaces: DS-003 = positive proof of Option A resilience (Qdrant-down read path); health map markers (`qdrant_write_side`) are the behavioral contract.
- Seams T10 must honor: Qdrant transport (T08 owns port/env in runner; T10 only appends CI stanza — line-ownership fence); usage persistence (teardown must drain writers before TRUNCATE without changing the production write path).
- Review guidance: `/workflows:review` must verify line-ownership fence on `run-e2e-tests.sh`, that DS-003 is not `#[ignore]`d, that no flag is orphaned, and that `domain`/`retrieval` stay infra-free.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | Unblock & green the live suite (teardown drain, clippy, DS-003 rewrite, burst rebalance, timing de-flake, ignored-reasons) | hardening | SC-V1.5-E, SC-V1.5-F | completed | 1 | unit-01-green-live-suite.md |
| 2 | Human-readable run summary (`latest-summary.md`) | hardening | SC-V1.5-E (adoption proof) | completed | 1 | unit-02-run-summary.md |
| 3 | CI gate + purity check + retire all V1.5 rollback flags (HUMAN GATE) | hardening | SC-V1.5-E, SC-V1.5-F | completed | 1 | unit-03-ci-gate-flag-removal.md |

## Learnings Brief
- [mcp-server] `LiveServerComponents::teardown` must drain the background usage writer (close sender → abort → await) BEFORE `truncate_all_tables`; the writer holds `RowExclusive` locks until it exits, deadlocking against `TRUNCATE … CASCADE`. Production hot-path write behavior is unchanged.
- [e2e] `from_environment` snapshots PG at boot; skills seeded after boot are invisible until a new server boot or a Redis `graph.rebuilt` refresh. Tests that seed + retrieve in one instance must build a fresh server post-seed.
- [e2e] Under Option A, Qdrant is write-side only: Qdrant down ⇒ read path (in-memory snapshot) still serves `Ok|NoMatch` and only `qdrant_write_side` health degrades. Ollama down ⇒ `Degraded` (embedding is a read dep). DS-003 encodes this.
- [testing] `cargo test` accepts only ONE positional TESTNAME before `--`; run multiple dream tests individually or via a regex after `--`. granite4:3b on CPU/WSL2 ≈ 17s/inference — size live extraction waits accordingly.
- [report] `tests/e2e/report.rs` exposes `ReportBuilder` (push_action, record_degradation_event, add_contract_assertion, record_latency, build → E2EReport). Unit 2 adds `latest-summary.md` emission on top of this; do not change the JSON `build()` shape.
