---
source_type: ticket-index
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_index: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/08-fix-qdrant-ports-and-suppression-isolation.md
source_packet_ref: "## Execution Slices > Slice 3.1: Fix Qdrant port handling + suppression test isolation + compose alignment"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-06-03T00:10:46Z
completed: 2026-06-03T00:10:46Z
status: completed
execution_shape: vertical-slices
batch: "7 (T08 — singleton)"
current_unit: 1
total_units: 1
session_id: work-2026-06-03-001046
review_mode: bulk
execution_model: sonnet # maintainer directive: execution agents are always sonnet
---

## WHY Context

### Problem Narrative
The 2026-05-31 deep-grok assessment scored V1.1 at 82% — "the loop closes on the bench, not yet in the body." Every live test in `test_live_data_plane_roundtrip` fails identically at the connectivity preflight (`GET http://localhost:16334/collections` → `hyper Parse(Version)`; 16334 is gRPC, REST `/collections` belongs on 16333), so the live suite can never reach its assertions. Suppression has no per-instance namespace, so a second `build_live_server()` reusing a session id returns `DuplicateSuppressed` instead of `Ok`. These are missing wiring/proof, not missing features.

### User Story
As a solo developer who deploys the skill layer with `docker compose up`, I need the whole live data plane proven green — including the live suite preflight connecting on the first try and two live servers on one Redis not leaking suppression — so I can trust the deployed system actually does what it promises instead of trusting a test that was written to fail.

### Architectural Context
9-crate workspace; no new crates. T08 feature home is `crates/infrastructure` (Qdrant adapter REST/gRPC port derivation, health labelling) + `crates/mcp-server` (suppression isolation, DefaultBodyLimit on the axum router). Seam: "Qdrant transport — the adapter derives REST (:6333/host :16333) and gRPC (:6334/host :16334) from one configured base so preflight and operational client never disagree; `run-e2e-tests.sh` exports the base the adapter expects." T03 (Option A / CQRS health relabelling, earlier batch) is already committed; T08 builds on its `vector/qdrant.rs` role labelling — no concurrent edit. Constitution v2.1.0 deferred-risk guard: admin/MCP surfaces are localhost/private-network only this phase — motivates the body-limit + loopback-posture checks.

### Success Criteria
- SC-V1.5-E (Live suite is GREEN): `./scripts/run-e2e-tests.sh --include-dream` passes; the Qdrant REST/gRPC port handling and suppression test-isolation defects are fixed. Plan SC-7. This ticket unblocks the harness so T09/T10 can reach real behavior.
- SC-V1.5-F adjacency: no production stub/false-health paths remain (carried by T03; T08 must not regress it).

### TDD Contract
- Effective mode: Ralph-driven TDD (plan `tdd.mode: ralph`, precedence `plan_overrides_local`, `inherit` ticket resolves to plan).
- Effective loop: failing tests first → minimal implementation → refactor → post-refactor rerun, per change.
- Required evidence: Unit (`cargo test --workspace` / targeted crate tests showing Red→Green→Post-Refactor Green for port derivation, suppression clear, body limit) + E2E (`cargo test -p mcp-server --features test-utils -- --ignored test_live_data_plane_roundtrip` preflight-connects + the `--include-dream` live suite against live containers).
- Exceptions: none.

### Constitution Context
v2.1.0. Principle 1 (local-first) — data plane (Qdrant/Ollama/PG) stays local; T08 only fixes port wiring. Agent Execution Rules "No stubs" → no false-health / unbounded-buffer paths. Deferred-risk guard → MCP/admin surfaces localhost/private-network only; T08 adds a loopback-posture preflight + DefaultBodyLimit (a trust signal, NOT a new auth system — do not expand to auth/public-network). HUMAN GATE: `docker-compose.test.yml` + `scripts/run-e2e-tests.sh` env edits require explicit approval before commit. Waivers: none.

### Architecture Handoff
- Artifact: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
- Feature homes: `crates/infrastructure` (Qdrant adapter REST/gRPC derivation, health.rs labelling), `crates/mcp-server` (suppression isolation, router body limit), `scripts/`+`tests/e2e/` (port/env alignment, live suite).
- Shared / global decisions: Option A (CHOSEN) — Qdrant is the durable write-side CQRS store; online read path is the PG-loaded `RetrievalSnapshot`. `healthy_markers()` must NOT claim `qdrant: "ok"` on the read path (T03 owns the relabel; T08 must keep it honest and may relabel to `qdrant_write_store`).
- Seam: Qdrant transport — one configured base derives REST + gRPC so preflight and operational client never disagree.
- Deletion test: live Qdrant online query (Option B) stays deferred to V2 — do NOT add a `retrieval → infrastructure/qdrant` read-path dependency.
- Drift to avoid: honesty drift (false `qdrant: "ok"`), boundary drift (`retrieval`/`domain` gaining `sqlx`/`redis`/`qdrant` deps), scope drift (no auth/public-network surface).
- Review guidance (for /workflows:review later): confirm preflight connects on REST port; suppression isolation real (UUID namespace + local clear bug fixed); body limit enforced with a test; loopback posture check is a warning signal only.

## Architecture Handoff
- Artifact: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
- Scope-line ownership fence: T08 owns ONLY the port/env lines in `run-e2e-tests.sh` (`QDRANT_URL` ~line 76) + the `docker-compose.test.yml` port lines; T10 owns the CI gate stanza. Do not collide.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T08 — Fix Qdrant port handling + suppression test isolation + compose alignment | hardening | SC-V1.5-E (live suite GREEN); unblocks T09/T10 | completed | 1 | unit-01-t08-qdrant-ports-suppression.md |

## Learnings Brief
_No learnings yet for this batch. Prior-batch learnings consulted: T03 already settled Option A + health relabelling in `vector/qdrant.rs`/`orchestrator.rs`/`health.rs`; T08 builds on the committed labels and must not re-open them._
