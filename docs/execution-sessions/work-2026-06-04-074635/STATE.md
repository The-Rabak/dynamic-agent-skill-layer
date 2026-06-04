---
source_type: ticket-index
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_index: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/10b-first-run-activation-proof.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
source_packet_ref: "Post-T10 adoption addendum: first-run activation proof"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-06-04T07:46:35+03:00
status: in_progress
execution_shape: vertical-slices
current_unit: 0
total_units: 1
session_id: work-2026-06-04-074635
---

## WHY Context

### Problem Narrative
V1.5 closes the gap between "correct in the test bench" and "works on `docker compose up`". After T10 turned the live suite GREEN, a fresh user still has to infer the happy path from Docker Compose, fixtures, ignored tests, and reports. Correctness makes the system safe; activation makes people want it. T10b is the smallest activation proof that preserves the no-dashboard, filesystem-first philosophy.

### User Story
As a solo developer who just cloned the repo and ran `docker compose up`, I want a doctor script that verifies my local stack and a demo script that seeds realistic skills, shows context injection with deterministic "why this matched" reasons, captures a transcript through the shipped ingest path, and proves a `.pending` draft lands on disk — so that I can experience the product promise (compiled context in, self-grown skill out) in under 10 minutes without reading the test harness.

### Architectural Context
9-crate Rust workspace, Docker Compose local deployment (PG + Redis + Qdrant + Ollama + MCP server). Online retrieval = Option A (refresh-on-rebuild PG snapshot; Qdrant = durable write-side CQRS store). T10b is a SCRIPTS+DOCS activation layer that sits ON TOP of the already-green live stack. It must not change retrieval/extraction behavior, thresholds, ports, env defaults, or the fixture corpus. It consumes T09's `tests/fixtures/retrieval_corpus.json` and T10's report shape (`scripts/generate-e2e-summary.py`, `tests/e2e/reports/latest-summary.md`).

### Success Criteria (T10b acceptance)
- `scripts/doctor.sh`: checks Docker/Compose, env vars, PG/Redis/Qdrant REST, Qdrant gRPC (when applicable), Ollama model, MCP `/health`, transcript-ingest secret posture, graph_version readability, Claude Code hook config presence. Clear ok|warn|fail lines + actionable next steps; exits non-zero ONLY for demo-blocking failures.
- `scripts/run-demo.sh`: reuses T09 corpus, seeds ≥2 skills, calls `compile_context`, prints injected skill names + deterministic "why this matched" reasons; captures a transcript through the SHIPPED command-hook/ingest-queue path (no hand-built relative `transcript_ref`) and proves a `.pending` draft lands; explicitly states `cloud_calls: none` on the default path; writes `tests/e2e/reports/activation-demo.md` (stack health, seeded skills, compile_context status, graph_version, extraction/queue status, pending draft paths, elapsed time, warnings).
- README quickstart points at `doctor.sh` + `run-demo.sh` before deeper E2E commands.
- Time-to-wow target recorded (<10 min fresh clone → first injected context, excluding model download); script reports elapsed time.
- `docs/runbooks/degraded-state.md` documents failure modes: missing Ollama model, wrong Qdrant port, no ingest secret, MCP down, no matching skills.

### TDD Contract
- Effective mode: Ralph-driven TDD (plan `tdd.mode: ralph`, `plan_overrides_local`)
- Effective loop: failing check first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: unit + e2e (required). For this scripts/docs ticket the "tests" are the scripts themselves exercised against the live stack (and/or `bash -n`/shellcheck + dry-run when the full Docker stack cannot be brought up in-agent). The downstream full e2e suite (`scripts/run-e2e-tests.sh --include-dream`, todo 4) provides the authoritative live-stack proof.
- Exceptions: none

### Constitution Context (v2.1.0)
- Local-First (P1): default `docker compose up` MUST NOT reach cloud. Demo default path must report `cloud_calls: none`. Ollama is default provider.
- Zero-Touch (P2): cold start (no matching skills) returns empty context silently.
- Human Gate (P3): extraction MUST produce `.pending` draft files requiring human rename-to-approve. Demo proves a `.pending` draft lands; it must NOT auto-approve.
- Deferred-risk guard: admin MCP tools unauthenticated → localhost/private only; doctor checks transcript-ingest secret posture and loopback posture.
- **Human gate for THIS ticket:** No human-gated infra config change is expected. If implementation touches compose/env defaults, STOP and request approval (orchestrator will treat as follow-up, not a blocker).

### Architecture Handoff
- Artifact: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
- Feature home: `scripts` (+ docs). Singleton post-gate ticket.
- Shared / global decisions: corpus owned by T09 (consume, don't redefine); report shape owned by T10 (consume `generate-e2e-summary.py` patterns / `latest-summary.md`); ports/env owned by T08 (do NOT edit).
- Scope fence: scripts are local developer aids only; must NOT become a second production control plane. No new production Rust unless doctor/demo discovers a missing public API — if so, STOP and file a follow-up rather than expanding the ticket.
- Deletion test: doctor/demo are concrete helper scripts; no speculative abstraction.
- Interfaces consumed: shipped command-hook/ingest-queue endpoint + queue drain (same path the e2e suite uses), `compile_context` MCP tool, `/health`, graph_version readout.
- Review guidance (for /workflows:review, todo 2): verify no port/env/threshold/corpus edits; verify shipped ingest path (no relative transcript_ref shortcut); verify cloud_calls:none on default; verify `.pending` proof is real; verify no production control-plane creep.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T10b — First-run activation proof (doctor + demo + time-to-wow) | hardening | SC-V1.5-A/B/E adoption proof | completed (2 concerns → review/triage) | 1 | unit-01-first-run-activation-proof.md |

## Learnings Brief
- [scripts] doctor.sh/run-demo.sh must agree with `run-e2e-tests.sh` on ports/env: Qdrant REST 6333, gRPC 6334; MCP `/health`; `TRANSCRIPT_INGEST_SECRET` posture.
- [mcp] `compile_context` returns status directly in `result` (not `result.content[].text`).
- [pg] `transcript_ingest_queue` orders by `updated_at`; query via `docker compose exec -T postgres psql`.
- [maintenance] `MAINTENANCE_RUN_ONCE=1` runs one production cron cycle (merge + drain) then exits.
- [ingest] `capture-transcript.sh` detaches its POST via `setsid`; augment with a synchronous idempotent POST for deterministic proof in WSL.
- [OPEN-A] Seeding SKILL.md files ≠ building the graph: compile_context degrades and graph_version stays 0 unless a rebuild/embedding is triggered. The demo must drive real retrieval, not print static corpus reasons.
- [OPEN-B] `.pending` discovery must be scoped to the demo's own sandbox dir, not globbed across all of `target/` (stale drafts from prior runs inflate the count).
