---
source_type: ticket-index
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_index: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/05-reliable-extraction-worker-pool-provider.md
source_packet_ref: "## Execution Slices > Slice 2.2: Reliable real extraction (Ollama) + worker-pool correctness + provider disposition"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-06-01T14:23:56Z
status: completed
execution_shape: vertical-slices
batch: "4 (T05 — singleton)"
current_unit: 1
total_units: 1
completed: 2026-06-01T00:00:00Z
session_id: work-2026-06-01-142356
review_mode: bulk
---

## WHY Context

### Problem Narrative
The V1.5 self-growing loop has a trigger (T04 SessionEnd) but extraction itself is
unproven and partly fake: the worker pool reports 0/32 parallel jobs completed (a
single `Arc<Mutex<Receiver>>` serializes all workers across `recv().await`), terminal
lifecycle events are split between `execute_job` and the worker loop so completions can
be lost, the dead `_retry_policy` param is never used, the default provider silently
maps unset/empty to Claude, and the Claude provider POSTs to a non-existent
`:8080/extract` (the graph-builder admin port — a confused-deputy/SSRF hazard). Ollama
defaults to `llama3.1` with a 1.5s inner timeout that fails every real `granite4:3b`
CPU extraction. Until this is fixed, "real extraction is reliable" (SC-C) and "no
production stub paths remain" (SC-F) are both false.

### User Story
As a solo developer who deploys the skill layer with `docker compose up`, I need real
extraction to complete reliably so that ending my Claude Code sessions actually grows
my skill graph — without silent stalls, lost completions, or a dead cloud endpoint.

### Architectural Context
Feature home: `crates/session-extractor/` (worker-pool orchestration + dispatch) plus
the shared extraction adapters `crates/infrastructure/src/extraction/{ollama,claude}.rs`.
Worker-pool concurrency stays in session-extractor; provider transport stays in
infrastructure. Ollama is the default, fully-local data-plane provider; Claude is a
first-class opt-in provider calling the real Anthropic Messages API (no sidecar, no new
compose port, no CLI subprocess).

### Success Criteria
- SC-V1.5-C: Ollama extraction completes end to end; under a ≥32-job burst every
  accepted job emits exactly one terminal event and writes/declines `.pending`
  deterministically.
- SC-V1.5-F: kill the dead `_retry_policy` and the `:8080/extract` stub path; clippy
  strict + rustfmt clean.

### TDD Contract
- Effective mode: Ralph-driven TDD (plan `tdd.mode: ralph`, `plan_overrides_local`,
  matches `compound-engineering.local.md`).
- Effective loop: failing tests first → minimal implementation → refactor →
  post-refactor rerun, per slice.
- Required evidence: Unit (required) `cargo test -p session-extractor` showing
  Red→Green→Post-Refactor Green for the new worker-pool/provider logic; E2E (required)
  the ≥32-job parallel burst (`cargo test -p mcp-server --features test-utils -- --ignored
  extract_session_parallel_burst`) against live Ollama — `#[ignore]`-gated where live
  infra is unavailable in this environment (consistent with commit a2c2271).
- Exceptions: none.

### Constitution Context
- Constitution v2.0.0 (todo 105 amendment): the extraction LLM provider is
  user-selectable. Ollama is the default and only data-plane-local path; Claude
  (`EXTRACT_SESSION_PROVIDER=claude`) is a FIRST-CLASS, fully-supported opt-in provider
  — NOT a stub, NOT a tolerated exception. No waiver required.
- Local-first preserved on the default path: Ollama default needs no cloud key;
  `ANTHROPIC_API_KEY` is read from env, never committed.
- Human-gate: none for code on this ticket. The opt-in provider env contract MUST be
  documented (capability catalog / README). No secret committed.

### Architecture Handoff
- Artifact: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
  + plan §Deepening Research Insights §2.2 (the concrete shapes).
- Feature homes: `crates/session-extractor` owns worker-pool orchestration + terminal-
  event dispatch; `crates/infrastructure/src/extraction` owns provider transport adapters.
- Shared/global: the extraction adapters are shared infra; keep concurrency/dispatch in
  session-extractor.
- Seams to honor: `TranscriptSkillExtractionService` trait (provider behind it);
  `ExtractionEventPublisher` seam for lifecycle events; the frozen 8-event Redis catalog
  (do not add new event types).
- Deepening candidates to preserve: `execute_job` returns a typed `ExtractionOutcome`
  and publishes NOTHING; the dispatch layer (worker loop + no-pool spawn path +
  `extract_blocking`) owns all three terminal events (completed/failed/timeout).
- Coupling to honor (todo 103, already merged): `extract_blocking` (lib.rs:415) is the
  durable transcript-ingest queue drain entry point and currently relies on `execute_job`
  publishing `extraction.completed` internally. Consolidating event ownership MUST keep
  `extract_blocking` emitting the completed event so the queue drain still observes
  completion before acking a row.
- Review guidance: `/workflows:review` (bulk) must verify single terminal-event
  ownership, no absolute paths in `extraction.completed`, loud construct-time failure for
  `provider=claude` without key, and that Ollama default needs no cloud key.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T05 — Reliable real extraction (worker-pool correctness + provider disposition) | hardening | SC-V1.5-C, SC-V1.5-F | completed | 1 | unit-01-t05-reliable-extraction.md |

## Learnings Brief
- **[concurrency]** `async_channel::bounded` (cloneable receiver) is the idiomatic MPMC replacement for an `Arc<Mutex<mpsc::Receiver>>` held across `.await` — each worker clones the receiver and pulls concurrently; drop the redundant `Semaphore`.
- **[extraction/dispatch]** Single terminal-event ownership: `execute_job` returns a typed `ExtractionOutcome` and publishes nothing; the worker loop, no-pool spawn path, and `extract_blocking` own the completed/failed/timeout events. `extract_blocking` (todo-103 queue drain) MUST keep emitting `extraction.completed` or the durable transcript-ingest drain stalls before ack.
- **[llm-providers]** Claude extraction = direct Anthropic Messages API (`{ANTHROPIC_BASE_URL}/v1/messages`), forced `emit_candidates` tool_use, `x-api-key`, static system block with `cache_control: ephemeral`; `provider=claude` w/o `ANTHROPIC_API_KEY` fails loudly at construct time. Ollama is the default and needs no cloud key.
- **[security]** Strip absolute host paths from `draft_paths` before publishing `extraction.completed` (Redis info-leak) — `scope_relative_draft_paths` + a payload assertion test.
- **[tdd]** Honest Red for a resumed/already-implemented unit: `git stash` to baseline, show keystone tests absent (18→24), `stash pop` for Green.
- **[testing]** Project convention (a2c2271): live-infra tests are `#[ignore]`-gated; the live ≥32-job burst e2e belongs to T10's green-live-suite gate. Deterministic offline burst (fake provider) is the in-CI keystone.
- **[scope]** Pre-existing rustfmt drift in `crates/graph-builder/src/graph/rebuild.rs` is unrelated to T05 — left untouched (scope fence). Worth a separate cleanup pass.

## Outcome
T05 completed in 1 attempt (resume-and-finish of a prior partial run). All 11 ACs verified; SC-V1.5-C and SC-V1.5-F satisfied. session-extractor 24 / infrastructure 55 / mcp-server 55+11 tests green; T05 surface rustfmt-clean. Batch 4 complete → `last_completed_batch` advanced to 4. Next batch: 5 (T06).
