---
title: feat: Dynamic Agent Skill Layer V1.1 — SkillRAE Implementation
type: feat
status: active
date: 2026-05-21
topic: skill-layer-v1-1
constitution_version: 1.0.0
constitution_waivers: []
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
source_docs:
  tickets: []
  docs: []
  figma: []
  plans: []
handoff:
  problem_narrative: true
  user_story: true
  architectural_context: true
  success_criteria: true
tdd:
  precedence: plan_overrides_local
  mode: ralph
  loop: red-green-refactor
  evidence:
    unit: required
    e2e: required
  exceptions: []
execution_shape:
  mode: vertical-slices
  rationale: ""
---

## Enhancement Summary

**Deepened on:** 2026-05-21
**Pass:** 3 (v1.1 canonicalization against adversarial assessment)
**Sections enhanced:** 24 (crate ownership, tool contracts, transcript transport, event/state contracts, execution slices, docs deliverables)
**Research agents used:** Rust crate split patterns, Outbox pattern (PG+Qdrant), Redis Streams event catalog, Filesystem watcher reliability, `.pending` lifecycle state machine
**Review inputs applied:** adversarial assessment, performance-oracle, constitution-guardian, data-integrity-guardian, spec-flow-analyzer
**Architecture ref:** docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md

### WHY Integrity Check
- Problem Narrative: preserved
- User Story: preserved
- Architectural Context: preserved
- Success Criteria: preserved
- Execution shape: preserved (vertical-slices)
- Packet tracing: all packets trace to user story or enabling outcome: yes
- Scope posture: original ambitious V1 scope preserved; contradictions removed instead of cutting slices

### Execution Readiness
**Updated:** Full 9-crate structure is now canonical again, with crate names and namespaces simplified by dropping the `skill-` prefix. The slice map stays ambitious, but the contracts that previously drifted between sections are now frozen in one versioned doc set.

Shape fit: all slices still use `vertical-slices`. Slice count remains 10, but ownership, tool boundaries, transcript ingress, result semantics, event flows, and invalidation rules are now explicit and aligned.

### TDD Contract Check
- Precedence: plan_overrides_local
- Effective loop: red-green-refactor
- Evidence: unit required, e2e required
- Exceptions: none

### Key Improvements (Pass 3 — Canonical V1.1)
1. **Full feature-home structure restored and frozen** (serves: SC-4, SC-7, SC-8) — `domain`, `infrastructure`, `mcp-server`, `retrieval`, `compiler`, `graph-builder`, `maintenance`, `admin`, and `session-extractor` are now the canonical V1.1 crate map. Scope was not reduced; ambiguity was removed.
2. **Naming and namespace cleanup applied** (serves: readability, API hygiene) — all crate and namespace references drop the redundant `skill-` prefix. Rust module/import namespaces use snake_case equivalents of the crate names.
3. **`compile_context` result semantics are now explicit** (serves: SC-1, SC-7) — `ok`, `no_match`, `degraded`, and `duplicate_suppressed` replace the old ambiguous empty-string contract. Duplicate suppression is set only after a healthy result (`ok` or `no_match`), never after a degraded attempt.
4. **Transcript ingress contract is resolved** (serves: SC-3, SC-7) — `extract_session` no longer trusts arbitrary host paths. V1.1 uses `transcript_ref` under a read-only mounted transcript root, with optional `transcript_inline` reserved for tests and future harnesses.
5. **One V1 scope persistence model is frozen** (serves: SC-4, SC-8) — V1.1 keeps scalar `scope` plus `merged_from_scopes TEXT[]`; contradictory `skill_scopes` junction-table guidance is removed from the implementation contract and deferred to V2 if real many-to-many scope membership is needed.
6. **One event/state contract is frozen** (serves: SC-5, SC-7) — the 8-event catalog, rename-approval semantics, `graph_version` invalidation rules, and outbox-before-`graph.rebuilt` ordering are now canonical across slices.
7. **Approval-flow reliability is hardened** (serves: SC-3, SC-5) — watcher rename idempotency, periodic reconciliation scans, and audit checkpoints cover missed filesystem events instead of assuming ideal watcher behavior.
8. **Adoption docs are part of scope, not an afterthought** (serves: real day-0 usability) — V1.1 explicitly delivers a 10-minute quickstart, capability catalog, transcript-mount contract guide, and degraded-state runbook without trimming system ambition.

### New Considerations Discovered
- **Performance oracle P1:** Recursive CTE unbounded → 200-800ms at 10K skills. Fix: LIMIT 50 per hop + relevance pruning.
- **Performance oracle P1:** Ollama semaphore priority inversion — offline batch starves online `compile_context`. Fix: reserve 1 slot for sync path.
- **Performance oracle P1:** No compiled context cache — repeated prompts waste 176ms. Fix: keyed cache with graph_version invalidation.
- **Performance oracle P1:** Outbox inconsistency window — `graph.rebuilt` fires before Qdrant ACKed. Fix: emit only after outbox drained.
- **Constitution guardian P1:** `.pending` auto-deletion violates Constitution §3 (human gate). Fix: warning-only at 30d, no auto-delete.
- **Data integrity P1:** Missing outbox table in PG schema. Missing `skill_subunits` composite indexes. `skill_usage ON DELETE CASCADE` → `SET NULL`. UUIDv4 fragmentation risk.
- **Data integrity P2:** No `updated_at` auto-update trigger. `communities.scope` scalar conflicts with dual-membership design. `audit_log` unbounded growth.
- **Adversarial assessment P0:** plan and architecture drifted on crate ownership, tool surfaces, transcript ingress, and suppression semantics. Fixed in this pass.
- **Spec-flow analyzer:** approval and rename behavior needed a reconciliation path when watcher events are missed. Added as a first-class lifecycle concern.

### Scope Warnings
- **Complexity posture:** this remains an intentionally ambitious V1.1. Complexity is accepted where it protects the differentiators (semantic retrieval, self-growth, maintenance loop), but trust-path contracts are now frozen before implementation.

### Simplifications Applied
- **Naming cleanup only, not scope reduction** — crate/module names are shorter, but the 9-crate decomposition stays intact.
- **`.pending` auto-deletion → warning-only** (per constitution guardian P1). TTL 30d log warning. No auto-delete. Files stay until human acts.
- **Scalar scope retained for V1.1** — merged provenance lives in `merged_from_scopes TEXT[]`; true many-to-many scope membership is deferred until a V2 use case requires it.
- **`skill.approved` deleted from the model** — rename approval is represented by `skill.file_changed` plus audit records and filesystem reconciliation.
- **Online surface composed, ownership still split** — `admin` and `session-extractor` stay separate crates, but their routers are composed into the online runtime so deployment remains sane.

### Architecture Handoff Contract (Updated)
- **Source:** Architecture artifact `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md` plus the adversarial assessment and deepening research.
- **Feature Homes (final):** `crates/domain/`, `crates/infrastructure/`, `crates/mcp-server/`, `crates/retrieval/`, `crates/compiler/`, `crates/graph-builder/`, `crates/maintenance/`, `crates/admin/`, `crates/session-extractor/`
- **Canonical contracts frozen:** tool result semantics, transcript transport, event catalog, lifecycle state machine, graph invalidation ordering, and scope persistence.
- **Drift Checks (added):** `cargo tree -p domain --depth 1` CI gate; event/schema ownership checks; watcher reconciliation tests; duplicate-suppression tests for degraded vs healthy outcomes.

---

# feat: Dynamic Agent Skill Layer V1.1 — SkillRAE Implementation

## Problem Narrative

A developer using multiple coding agent harnesses (Claude Code, OpenCode, Copilot, Codex) faces a triple compound cost every session: manual skill selection wastes 5-10 minutes per task on context setup, skill libraries accumulate but rot unused because maintenance overhead exceeds utility, and each harness operates in a silo — skills built in one never transfer to another. The problem spans two scopes: project-local skills capturing repo-specific patterns, and global machine-wide skills capturing cross-project expertise. Existing approaches force the developer to choose one scope or manually curate both, with no concurrent search or intelligent fusion. The SkillRAE paper (arXiv:2605.10114) proves that multi-level skill graph retrieval + context compilation delivers 11.7% improvement over SOTA on SkillsBench, and weighted RRF + MMR are the right fusion strategies for multi-scope evidence. This is the right time to implement this as local-first Docker-deployed infrastructure that eventually scales to team-wide sharing.

## User Story

As a solo developer using multiple coding agent harnesses,
I need a zero-touch, self-growing skill context layer that searches both project-local and global machine-wide skill scopes concurrently, merges results via weighted RRF + MMR, and at session start compiles relevant skills into a task-specific compact context, while at session end auto-extracts new skills from session activity into the appropriate scope, and offline deduplicates, merges, and retires stale skills across both scopes,
so that every session starts with perfectly scoped, de-duplicated context in under two seconds, and every session grows the right skill graph,
because currently I manually select and scope skills, skills rot unused, and nothing transfers between projects or harnesses,
which causes compounding time loss, dead skill libraries, and zero cross-scope intelligence.

### Secondary Story: Offline Maintenance Operator

As a developer with an accumulated skill graph,
I need the system to periodically detect near-duplicate skills for merging and stale skills for retirement, proposing these changes as filesystem-visible drafts I can approve,
so that my skill graph stays clean and relevant without manual curation,
because skill drift over time silently degrades retrieval quality.

## Architectural Context

### Complexity Justification

This plan requires the A LOT detail level because:

1. **Multi-service distributed system:** Nine Rust crates, multiple runtime processes, five Docker containers (Qdrant, PostgreSQL, Redis, Ollama, + Rust services), event-driven communication via Redis Streams, and MCP protocol integration with Claude Code. A simpler plan format would obscure inter-service dependencies and sequencing.
2. **Greenfield with no existing codebase patterns:** No prior code, no conventions, no infrastructure. Every design decision (schema, event contracts, MCP tool surface, embedding model) must be explicit because there is no existing code to reference.
3. **Dual-scope architecture with fusion:** Project-local + global scope concurrent retrieval with MMR-then-RRF fusion is a novel combination not found in existing codebases. The fusion algorithm, scope resolution, and result merging must be explicitly specified.
4. **Constitution-compliant from day one:** Five non-negotiable principles (local-first, zero-touch, human-gate, portable-scope, filesystem-observability) with eight approval boundaries. Without explicit constitution alignment, downstream phases will drift.

### System Placement

- **Lives in:** `dynamic-agent-skill-layer/` root, deployed via Docker Compose
- **Feature homes:** Nine Rust crates with explicit ownership:
  - `crates/domain/` — pure types, traits, config, errors
  - `crates/infrastructure/` — PG/Qdrant/Redis/Ollama adapters, resilience, logging
  - `crates/mcp-server/` — online MCP bootstrap and router composition
  - `crates/retrieval/` — dual-scope retrieval, scoring, MMR, RRF
  - `crates/compiler/` — template compilation and rescue formatting
  - `crates/graph-builder/` — watcher-driven graph construction and rebuild orchestration
  - `crates/maintenance/` — merge, retire, cron, reconciliation policies
  - `crates/admin/` — read/debug/trigger MCP tools
  - `crates/session-extractor/` — transcript ingestion, provider routing, `.pending` generation
- **Interacts with:**
  - Claude Code (V1 harness): via MCP protocol. Hooks configured in `.claude/settings.json` using `type: "mcp_tool"`:
    - `UserPromptSubmit` hook → calls `compile_context` (state keyed by `{session_id, repo_path}`). Hook logic injects only when result status is `ok`; healthy `no_match` suppresses duplicates without injecting filler; `degraded` does not consume the one-shot opportunity.
    - `SessionEnd` hook → calls `extract_session` with `transcript_ref` (relative to a read-only mounted transcript root) plus `{session_id, repo_path}`. Initiates async extraction (<1.5s return).
  - Ollama (Docker Compose): embedding generation (`nomic-embed-text`, 768-dim) and optional extraction fallback. Called by graph-builder, retrieval, and session-extractor.
  - Qdrant (Docker Compose): vector storage, hybrid similarity search. Stores skill, community, and subunit embeddings with scope-tag payload filters.
  - PostgreSQL (Docker Compose): graph structure with recursive CTEs. Normalized schema: `skills`, `subunits`, `communities`, `skill_subunits` (junction), `community_skills` (junction). Session logs, merge/retire audit trail.
  - Redis (Docker Compose): canonical 8-event catalog — `skill.file_changed`, `skill.extraction_requested`, `extraction.completed`, `extraction.failed`, `graph.rebuilt`, `graph.rebuild_failed`, `skill.retired`, `skill.merged`.
  - Filesystem: graph-builder reads `SKILL.md`, `.pending`, and `.retired` files recursively from git root (project scope) and harness skill directories (global scope array from env vars). session-extractor and maintenance write filesystem proposals; reconciliation scans close watcher gaps.
  - V2 harnesses (future): OpenCode, Copilot, Codex via same MCP interface.
- **User entry point:** MCP protocol — no UI. Developer types task in Claude Code → `UserPromptSubmit` hook fires → `compile_context` → structured result envelope returned. Hook injects `additionalContext` only for `ok`. Session end → `SessionEnd` hook → `extract_session` → `.pending` files appear.
- **Data:** Skill embeddings (Qdrant), skill-subunit-community graph (PG with recursive CTEs), compiled-context result envelopes (`status`, `reason_code`, `additional_context`, `health`), draft files (`.pending`, `.retired`, `.rejected` tombstones).
- **Dependencies:** Docker runtime with 5 containers. Claude Code requires MCP server for context injection.
- **Conventions:** Rust 2024 edition, tokio async runtime, SQLx for PG, qdrant-client for Qdrant, redis-rs for Redis, rmcp for MCP server. All configuration via Docker Compose environment variables.
- **Boundary constraints:** Must NOT depend on any cloud services. Must NOT require a web UI. Must NOT trust arbitrary host filesystem paths from hook payloads. Must NOT modify files outside scope directories or `.pending`/`.retired` patterns without human approval.

## Success Criteria

- [ ] SC-1: Zero-touch context injection — developer types a raw task in Claude Code, relevant skills with task-specific guidance appear in session context within 500ms without any manual intervention
- [ ] SC-2: Dual-scope concurrent retrieval — project-local and global scope skills are searched concurrently and merged via MMR-then-RRF fusion into a relevance-ordered, de-duplicated skill set
- [ ] SC-3: Session-end skill extraction — `SessionEnd` hook triggers transcript analysis, producing `.pending` draft SKILL.md files. Developer renames to `.md` to approve, deletes to reject
- [ ] SC-4: Offline graph maintenance — periodic cron-triggered full rebuild detects near-duplicate skills (cosine > 0.85) for LLM-reviewed merge, and stale skills (usage < 1/month) for retirement review, producing `.pending` merge proposals and `.retired` markers
- [ ] SC-5: Filesystem-observable state — all graph mutations (skill creation, retirement, merge) are visible as filesystem changes. No hidden state
- [ ] SC-6: Subunit-aware compilation — compiled context includes task-specific subunit highlights, rescue-aware attached evidence from non-selected skills, and structured markdown guidance
- [ ] SC-7: Graceful degrade — on infrastructure failure, `compile_context` returns explicit `degraded` status with reason/health markers, healthy no-match remains distinct from degraded-empty, and offline services retry with backoff
- [ ] SC-8: V2 readiness — architecture supports adding remote team scope (remote PG + Qdrant URL) with an additive schema migration only, without reworking retrieval/compilation/service boundaries. No SQLite dependency

## TDD & Evidence Contract

- **Precedence:** Plan overrides local. No `compound-engineering.local.md` exists → defaults apply.
- **Effective mode:** Ralph-driven TDD
- **Effective loop:** Failing tests first → minimal implementation → refactor → post-refactor rerun
- **Required evidence:**
  - Unit: `cargo test --workspace` (must show Red → Green → Post-Refactor Green per slice)
  - E2E: `docker compose -f docker-compose.test.yml up --abort-on-container-exit` (must verify end-to-end: MCP tool call → compiled context returned)
- **Exceptions:** None. Unit + e2e evidence required throughout.

## Execution Shape

- **Mode:** vertical-slices
- **Why:** Every phase delivers a user-visible tracer bullet. Phase 1 proves MCP server compiles context end-to-end. Phase 2 adds offline graph construction and session-end extraction. Phase 3 hardens reliability and documentation.

## Constitution Alignment

- **Relevant principles:** All five apply — local-first (Docker Compose, zero cloud), zero-touch (auto context injection via MCP hooks), human-gate (`.pending`/`.retired` approvals), portable-scope (same SKILL.md format across scopes), filesystem-observable (all mutations visible as file changes).
- **Applicable baselines:** Full CI bar (clippy strict, rustfmt, tests, benchmarks), clean code (vertical slice modules, SOLID, DRY, no stubs), Docker Compose deployability.
- **Required approvals:** Constitution amendments, skill mutations (create/retire), tag creation, graph schema migrations, Ollama model changes, Redis event contract changes, infrastructure config changes. All handled by filesystem-based approval pattern.
- **Waivers:** None. All five principles apply without exception.

## Stakeholder Impact

- **End user (developer):** Every Claude Code session starts with automatically compiled, scope-aware skill context. Zero manual skill selection. Session-end skill extraction preserves institutional knowledge. Cross-project skills transfer automatically.
- **Developers (this codebase):** Nine Rust crates with explicit feature homes, plus Docker Compose for local dev. Service boundaries are documented at crate, tool, event, and state-transition level so implementation work can stay aligned.
- **Operations:** `docker compose up` deploys everything. Five containers. Ollama requires GPU for acceptable embedding speed; CPU fallback available. Structured JSON logging to stdout for all services.
- **Business:** V1: personal productivity multiplier — less context-switching, more coding. V2: team-wide skill sharing creates compounding institutional intelligence.

## Overview

The Dynamic Agent Skill Layer V1.1 implements the SkillRAE paper as a production Rust microservice system deployed via Docker Compose. It delivers zero-touch skill context compilation for Claude Code sessions, with an offline skill graph that self-grows from session activity and self-maintains through deduplication, merging, and retirement.

The system uses a split-crate architecture with a thin online MCP surface, a dedicated retrieval/compiler path, offline graph construction, a separate maintenance policy engine, and a session-extraction path that writes human-gated filesystem proposals. Claude Code integrates via native MCP hooks — `UserPromptSubmit` for context injection, `SessionEnd` for skill extraction. All mutations go through a filesystem-based human approval workflow (`.pending`/`.retired`/`.rejected` state).

V1 scope: single developer, two scopes (project/git-root + global/harness-dirs), Claude Code harness, Docker Compose deployment. V2 roadmap: multi-harness (OpenCode, Copilot, Codex) and team scope (remote PG + Qdrant).

## Proposed Solution

### Technical Approach

#### Architecture

### Research Insights — Rust Microservices & Async Patterns

**Best Practices** (serves: SC-1 latency, SC-7 graceful degrade):
- Use `tokio_util::sync::CancellationToken` tree for graceful shutdown across services. One root token, `.child_token()` per spawned task. `tokio::select!` on both signals and task completion.
- Use `tokio::task::JoinSet` for dynamic task spawning with bounded concurrency — replaces raw `tokio::spawn` + manual `Vec<JoinHandle>`.
- Connection pools: `sqlx::PgPoolOptions` with `.max_connections(20)`, `.acquire_timeout(Duration::from_secs(5))`, `.idle_timeout(Duration::from_secs(300))`. Per-service pools, not shared.
- `domain` types MUST NOT depend on infrastructure crates (sqlx, qdrant-client, redis). `domain` = pure types + traits only.
- Workspace `[workspace.dependencies]` for version consistency. `resolver = "2"` for feature unification.
- Rust 2024 edition: async closures stable (`async || {}`). `unsafe_op_in_unsafe_fn` warn-by-default. Use `cargo fix --edition` for migration.

**Docker Build Optimization** (serves: SC-7 deployability):
- `cargo-chef` with pre-built `lukemathwalker/cargo-chef:latest-rust-1` image for build cache separation.
- Multi-stage: planner (chef prepare) → builder (chef cook + cargo build) → runtime (alpine:3.21, minimal).
- `ARG BIN` per service for single Dockerfile, multi-binary builds: `cargo build --release --bin ${BIN}`.
- musl + `tls-rustls` feature for SQLx (avoids OpenSSL C dependency). Alpine produces ~12MB images.
- Cache mounts: `--mount=type=cache,target=/app/target/` for incremental builds.

**Performance Considerations** (serves: SC-1 <500ms):
- Connection pools, not ad-hoc connections. PG `min_connections: 2` for warm pool.
- `tokio::sync::Semaphore` for Ollama concurrency. Max 4 concurrent embedding calls.
- `DashMap<String, CachedContext>` for session-scoped compilation cache — concurrent reads without lock contention.

**Edge Cases** (risk to: SC-7):
- Don't `tokio::spawn` without `JoinHandle` or `AbortHandle` — lose ability to cancel or await.
- Don't rely on `Drop` for cleanup in containers — pools/servers need explicit `.close().await`.
- Don't use `resolver = "1"` — breaks feature unification across workspace members.

**References:**
- Tokio CancellationToken docs: https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html
- cargo-chef: https://github.com/LukeMathWalker/cargo-chef
- Rust 2024 edition guide: https://doc.rust-lang.org/edition-guide/rust-2024/

### Research Insights — MCP Protocol Implementation

**Best Practices** (serves: SC-1, SC-2):
- Use `rmcp` crate (v1.5+) with features: `server`, `macros`, `schemars`, `transport-streamable-http-server`, `transport-io`.
- HTTP transport preferred for Docker Compose (not stdio): MCP server binds port 3001, Claude Code connects via `http://localhost:3001/mcp`.
- Auto-generated JSON schemas from `schemars` derive macros on request/response structs. No manual schema writing.
- Tool registration: `#[tool_router(server_handler)]` generates handler + router. Multi-router composition with `compile_router() + inspect_router()`.
- `compile_context` response must be explicit: `{status, reason_code, additional_context, health, scopes_considered, graph_version}`.
- Graceful degrade: never collapse healthy no-match and infrastructure failure into the same empty response. `status = "no_match"` means the system was healthy; `status = "degraded"` means trust path is impaired.
- Session state: `DashMap<String, SessionState>` keyed by `{session_id}::{repo_path}` with Redis write-through persistence. Suppression is set only after a healthy outcome (`ok` or `no_match`), never after `degraded`.
- Claude Code `.claude/settings.json` hooks: `UserPromptSubmit` → `mcp_tool` hook → `compile_context`. `SessionEnd` → `extract_session`. MCP server registered as `"type": "http", "url": "http://localhost:3001/mcp"`.

**Performance Considerations** (serves: SC-1 <500ms):
- HTTP transport adds ~5-10ms overhead vs stdio. Acceptable trade for Docker compatibility.
- `DashMap` for session cache: O(1) concurrent reads, no lock contention.
- First healthy `compile_context` result per session sets suppression; degraded attempts do not. Subsequent suppressed calls return `duplicate_suppressed` immediately (state check <1ms).

**Edge Cases** (risk to: SC-7):
- If MCP server restarts mid-session, suppression state must survive restart. Fix: persist state to Redis with 24h TTL.
- `SessionEnd` hook timeout default 1.5s — extract_session must return immediately. Async background task handles heavy work.

**References:**
- rmcp crate: https://crates.io/crates/rmcp
- Claude Code hooks: https://docs.anthropic.com/en/docs/claude-code/hooks
- MCP specification: https://modelcontextprotocol.io

### Research Insights — Embedding Service Abstraction

**Best Practices** (serves: SC-1, SC-2):
- **Trait abstraction:** Define `EmbeddingService` trait in `domain`. Concrete `OllamaEmbeddingService` implements it in `infrastructure`.
- **Config-driven routing:** Config fields `provider` (ollama), `model` (nomic-embed-text), `dimensions` (768), `endpoint` (http://ollama:11434) determine which concrete implementation to construct.
- This enables: unit testing with mock/fake embeddings (no Docker needed), future provider swaps (OpenAI embeddings, local ONNX), and clean separation of concerns.
- Ollama integration: POST `/api/embeddings` with `{model, prompt}`. Response: `{embedding: [f32; 768]}`. Use `reqwest` with connection pooling and timeout (500ms hard limit for sync path).
- Embedding cache: LRU cache (e.g., `lru` crate, capacity 1000) for recent prompts → embeddings. Hit rate 30-50% for repeated task types.
- Ollama semaphore: `tokio::sync::Semaphore::new(4)` around embedding calls. Prevents queue explosion under concurrent load.

**Performance Considerations** (serves: SC-1 <500ms):
- Ollama embedding: 15ms (GPU warm) to 100ms (CPU cold). This dominates the 500ms budget.
- Cache hit = 0ms Ollama time. Cache miss = 15-100ms. First priority for latency optimization.
- Cold start short-circuit: Query `SELECT COUNT(*) FROM skills WHERE status = 'active'`. If zero, return `no_match` immediately (skip Ollama + Qdrant + PG).
- Ollama batching: Use `/api/embed` batch endpoint for offline graph builder (sends multiple texts in one call).

**Edge Cases** (risk to: SC-7):
- Ollama unreachable → trait implementation returns `Err(EmbeddingError::ProviderUnavailable)` → MCP server returns `degraded` with `reason_code = "embedding_provider_unavailable"`.
- Model not pulled → health check on startup. If model missing, log warning (cold start still works — healthy `no_match` remains distinct from degraded infra).

### Research Insights — Qdrant Vector Search

**Best Practices** (serves: SC-2):
- Single collection `skills` with payload field `node_type` (skill/subunit/community) + `scope` (project/global). One collection avoids cross-collection joins.
- `CreateFieldIndexCollectionBuilder` for keyword indexes on `scope`, `node_type`, `tags`, `community_id`. Payload indexes critical for filtered search under 500ms.
- Dual-scope search: two concurrent `query()` calls with scope filter via `tokio::join!`. Post-process merge.
- Community-aware: Phase 1 find top-K communities by centroid similarity. Phase 2 search skills+subunits within those communities (community_id IN filter).
- Scalar quantization + `on_disk_payload(true)` for memory efficiency. `hnsw_ef(128)` for accuracy/latency balance.

**Performance Considerations** (serves: SC-1 <500ms):
- Qdrant search latency: ~5-15ms for collections up to 50K points (with payload indexes + scalar quantization).
- Two round-trips per request (project + global). Use `tokio::join!` for concurrent execution — latency = max(project, global) + overhead.
- `score_threshold(0.3)` to reject noise results.
- Optimizers: `indexing_threshold(1_000)` for fast search startup, `memmap_threshold(50_000)` for large segments.

**Edge Cases** (risk to: SC-7):
- Qdrant unreachable → `tokio::time::timeout(Duration::from_millis(400))` on search. On timeout → return partial retrieval outcome and bubble degraded scope markers upward; do not pretend the result was a healthy no-match.
- Empty collection → search returns empty results. Continue normally (cold start).
- Graceful degrade: in-memory cosine-similarity fallback for small skill sets when Qdrant is down.

**References:**
- qdrant-client crate: https://crates.io/crates/qdrant-client
- Qdrant query API: https://qdrant.tech/documentation/concepts/search/

### Research Insights — PostgreSQL Recursive CTEs

**Best Practices** (serves: SC-2 graph traversal, SC-4 merge/retire):
- **Always set depth cap:** `WHERE sg.hop_depth < 3` in recursive CTEs. Prevents infinite loops + query timeout.
- **Cycle guard:** `NOT ss3.skill_id = ANY(sg.visited)` with `ARRAY[$1]::uuid[]` explicit cast.
- **Composite indexes on junction tables:** `skill_subunits(subunit_id, skill_id)` AND `skill_subunits(skill_id, subunit_id)` — both directions needed for recursive join.
- **Partial index:** `CREATE INDEX idx_skills_active_id ON skills(id) WHERE status = 'active'` — smaller, faster for queries filtering active skills.
- **No `sqlx::query!` for recursive CTEs** — macro connects to live DB at compile time, chokes on `WITH RECURSIVE`. Use `sqlx::query_as` (runtime-checked).
- Community membership queries: `COUNT(DISTINCT community_id)` not `COUNT(*)` — HDBSCAN + tag dual membership would double-count otherwise.

**Performance Considerations** (serves: SC-1 <500ms):
- Recursive CTEs with depth limit 3 + composite indexes: <10ms for 1K skills, 10K subunits.
- Without composite indexes: sequential scan on `skill_subunits` per recursive step — 10-50x slower.
- Query-time bound: only traverse from top-K Qdrant results (e.g., 20 skills), not the full graph.

**Edge Cases** (risk to: SC-4):
- Merge transaction wraps multi-table mutations in `pool.begin()` with `SELECT FOR UPDATE` on affected skill rows. Prevents concurrent merge/rebuild conflicts.
- Retirement: `DELETE FROM skill_subunits WHERE skill_id = $1` before status update. Orphaned subunits handled by graph builder cleanup.

**References:**
- PG recursive CTE docs: https://www.postgresql.org/docs/current/queries-with.html
- SQLx query_as: https://docs.rs/sqlx/latest/sqlx/macro.query_as.html

### Research Insights — Redis Streams Event Bus

**Best Practices** (serves: service communication):
- Event envelope: JSON with `{event_id: UUIDv7, event_type: "domain.action", schema_version: 1, timestamp, payload: {...}}`.
- Publisher: `XADD stream * field1 val1 field2 val2`. Consumer: `XREADGROUP GROUP group consumer BLOCK 5000 STREAMS stream >`.
- Dead letter queue: `{stream}:dlq` after 3 delivery attempts. Redis SETEX-based idempotency keys (not in-memory HashSet).
- Backpressure: monitor stream length. Pause publishing if `XLEN > 10_000`. Trim: `MAXLEN ~ 100_000` approximate.

**Performance Considerations:**
- `BATCH_SIZE = 100` (not 10) for throughput. Multiple consumer instances with same group for parallelism.
- Reclaim pending messages every 5s with `XAUTOCLAIM`.

**Edge Cases:**
- `noeviction` + 512MB maxmemory = OOM risk with unbounded streams. Fix: `maxmemory-policy allkeys-lru` or strict `MAXLEN`.
- In-memory `processed` HashSet grows unbounded — memory leak. Fix: Redis SETEX with TTL.

### Research Insights — MMR + RRF Fusion Algorithms

**Best Practices** (serves: SC-2 fusion):
- **MMR** (per-scope): Greedy selection. λ=0.7 balances relevance (70%) and diversity (30%). Cosine distance on 768-dim skill embeddings. O(n²) where n=candidates per scope. Enforce max candidate limit (100).
- **Weighted RRF** (cross-scope): `RRF(d) = Σ w_s / (k + rank_s(d))`. k=60 (TREC standard). Project weight 1.0, global weight 0.7. Same skill in both scopes → scores summed (boosted).
- **Fusion order:** MMR per-scope first (dedup within scope), then RRF cross-scope (merge). Reversing would cause cross-scope duplicates to consume MMR budget.
- Relevance threshold filtering applied before MMR to reduce candidate pool.

**Edge Cases:**
- Empty inputs → empty output. Single candidate → passes through. All identical embeddings → heavily penalized by MMR.
- Orthogonal embeddings → zero penalty, passes through. Same skill both scopes → RRF scores summed.

### Research Insights — HDBSCAN Community Detection

**Best Practices** (serves: SC-4 community detection):
- **Use pure Rust `hdbscan` crate (v0.12)**, not Python subprocess. Zero external deps. `features = ["parallel"]` for rayon-accelerated MST.
- For 768-dim embeddings, `NnAlgorithm::BruteForce` (K-d trees degrade at high dimensionality).
- `min_cluster_size = 3` prevents 1-skill communities. Skills with label -1 remain unclustered (noise) — retrievable, just no community boost.
- Community labels derived from top-IDF terms in cluster members' subunits (not clustering algorithm's job).
- Dual membership: HDBSCAN clusters → insert `community_skills` with `source = 'hdbscan'`. Tags → insert with `source = 'tag'`.
- 1K skills: <50ms runtime, ~6MB memory. 5K skills: ~15-30s (acceptable for cron). 10K+: O(n²) memory becomes concerning.

### Research Insights — Filesystem Watcher

**Best Practices** (serves: SC-5 filesystem observability):
- Use `notify-debouncer-full` (not notify-debouncer-mini). Full debouncer preserves EventKind (create/modify/delete/rename distinction). Mini only emits Any/AnyContinuous.
- `FileIdMap` tracks inode+dev so renames are merged into `RenameMode::Both` events with both paths.
- `.pending` → `.md` rename detection: check `from.extension() == "pending"` && `to.extension() == "md"` in `RenameMode::Both` handler.
- Multi-directory: `watcher.watch(root, RecursiveMode::Recursive)` per scope directory. Skip missing global dirs (user may not have all harnesses).
- `Config::with_follow_symlinks(false)` prevents symlink loops. `with_event_kinds(EventKindMask::CORE)` filters to essential events only.

**Performance Considerations:**
- 2000ms debounce window. Collapses editor temp file sequences (write → rename) into single event.
- Linux: check `fs.inotify.max_user_watches` against estimated directory entry count. Warn if insufficient.

**Edge Cases:**
- Network filesystems (WSL, NFS, Docker macOS): native backends produce no events. Fallback to PollWatcher if no events in first 5 seconds.
- Redis unavailable mid-run: log + continue. Events lost are acceptable (incremental rebuild catches up on next event).

**References:**
- notify crate: https://crates.io/crates/notify
- notify-debouncer-full: https://crates.io/crates/notify-debouncer-full

### Research Insights — Structured Logging

**Best Practices** (serves: all SCs via observability):
- `tracing-subscriber` with `.json()` layer. Format: `tracing_subscriber::registry()` + `fmt::layer().json().with_current_span(true).with_span_list(true)`.
- `#[tracing::instrument]` on public handlers with `skip(db)`. Auto-spans: function name = span name, args = span fields.
- Latency measurement: `Instant::now()` at top, `Span::current().record("latency_ms", ms)` before return. Or `FmtSpan::CLOSE` for auto-timing on span close.
- `RUST_LOG=info,mcp_server=debug,skill_engine=trace` for per-crate filtering. `release_max_level_info` strips DEBUG/TRACE from release binaries entirely.
- Docker stdout: `docker compose logs mcp-server | jq '.'` for structured JSON output. Log driver: `json-file` (default, not journald).
- Disabled spans cost <5ns (LevelFilter compare, compiler-optimized). Enabled spans cost ~200ns per creation.

### Research Insights — Criterion Benchmarking

**Best Practices** (serves: SC-1 latency verification):
- `criterion = { version = "0.7", features = ["async_tokio", "html_reports"] }`. `[[bench]] name = "compile_context"` in Cargo.toml.
- `sample_size(100)`, `warm_up_time(10s)`, `measurement_time(30s)` for latency benchmarks. `noise_threshold(0.03)` for I/O variance tolerance.
- assertion binary parses `target/criterion/<group>/<bench>/new/raw.csv` for p50/p95/p99 from `sample_measured_value` column. Assert p50 <500ms, p95 <800ms.
- Subsystem benchmarks: one `BenchmarkId` per pipeline stage (embed, qdrant, pg, scoring, mmr, rrf, template, e2e). Identifies bottleneck.
- CI: `--baseline main` for regression detection. `--noplot --quick` for faster CI runs. GitHub Actions with `services:` blocks for Docker dependencies.

**Latency Budget** (per compile_context stage, GPU warm / CPU cold):

| Stage | Optimistic (GPU) | Pessimistic (CPU) |
|-------|------------------|-------------------|
| Embed prompt (Ollama) | 15ms | 100ms |
| Qdrant search ×2 | 10ms | 30ms |
| PG graph queries | 5ms | 20ms |
| Scoring (eq.3) | <1ms | 2ms |
| MMR dedup | 2ms | 8ms |
| RRF fusion | <1ms | 1ms |
| Template compilation | 2ms | 5ms |
| Overhead (net/serde) | 5ms | 10ms |
| **Total** | **~40ms** | **~176ms** |

<500ms achievable on GPU. CPU-only at risk for p95. Embedding cache + Ollama semaphore are critical mitigations.

```
┌─────────────────────────────────────────────────────────┐
│                    Docker Compose                        │
│                                                         │
│  ┌──────────┐  ┌──────────┐  ┌───────────────────┐     │
│  │  Ollama  │  │  Qdrant  │  │   PostgreSQL       │     │
│  │ :11434   │  │ :6333    │  │   :5432            │     │
│  │ embed    │  │ vectors  │  │   graph + metadata  │     │
│  └────┬─────┘  └────┬─────┘  └────────┬──────────┘     │
│       │             │                │                  │
│  ┌────┴─────────────┴────────────────┴──────────┐      │
│  │              Redis :6379                       │      │
│  │         Event Streams (pub/sub)               │      │
│  └────┬─────────────┬────────────────┬──────────┘      │
│       │             │                │                  │
│  ┌────┴─────┐  ┌────┴─────┐  ┌──────┴───────────┐     │
│  │ graph-   │  │ mcp-     │  │ hook-            │     │
│  │ builder  │  │ server   │  │ processor        │     │
│  │ :8080    │  │ :3000    │  │ :8081            │     │
│  │ offline  │  │ online   │  │ post-session     │     │
│  │ async    │  │ sync     │  │ async            │     │
│  └──────────┘  └────┬─────┘  └──────────────────┘     │
│                     │ MCP                               │
└─────────────────────┼───────────────────────────────────┘
                      │
              ┌───────┴────────┐
              │  Claude Code    │
              │  hooks:         │
              │  UserPromptSubmit
              │  SessionEnd     │
              └────────────────┘
```

**Data flow — Session start:**
1. Developer types "fix the auth token expiry bug" in Claude Code
2. `UserPromptSubmit` MCP tool hook fires → calls `mcp-server.compile_context({prompt, session_id, repo_path})`
3. MCP server checks session-scoped state (keyed by `{session_id, repo_path}`) — if a prior healthy result exists, return `duplicate_suppressed`; otherwise proceed
4. Generates prompt embedding via Ollama (`nomic-embed-text`, 768-dim)
5. Concurrent dual-scope retrieval:
   - **Project scope** (from git root): top-down community matching (Qdrant community centroids) + bottom-up subunit projection (Qdrant subunit embeddings → PG edge lookup → source skills)
   - **Global scope** (from harness dirs array): same retrieval pattern
6. Combined scoring per SkillRAE paper eq.3: `(α·skill_sim + β·subunit_evidence + γ·name_score) × (1 + λ·community_boost)`
7. Relevance-threshold filtering (skills below threshold excluded)
8. MMR per-scope deduplication (remove near-duplicate results within each scope)
9. RRF cross-scope fusion (merge project + global ranked lists, weighted by scope priority)
10. Evidence export: highlight top-3 task-relevant subunits per selected skill
11. Rescue: scan top-20 non-selected skills for high-relevance subunits, attach to most compatible selected skill
12. Compile structured markdown context (template-only, no LLM guidance generation in V1)
13. Return `{status, reason_code, additional_context, health, scopes_considered, graph_version}`:
    - `ok` with `additional_context` when healthy results exist
    - `no_match` when the system was healthy but nothing relevant matched
    - `degraded` when retrieval/infra trust was impaired
14. Session suppression is recorded only after `ok` or `no_match`. `degraded` leaves the session eligible for a later healthy retry. Target <500ms total for healthy path.

**Structured markdown context template:**

```markdown
## Context for: {task_prompt}

### Relevant Skills
{for each selected skill}
- **{skill.name}** ({skill.scope} scope): {skill.description}
  - Key procedure: {highlighted_procedures}
  - Convention: {highlighted_conventions}
  {if rescue_attached}
  - ⚡ Also see: {rescued_subunit_content} (from related skill {source_skill})
  {/if}

### Task-Specific Guidance
- Apply **{skill.name}**'s {procedure_name} for {specific_task_aspect}
- Follow **{skill.name}**'s {convention_name} when {specific_task_context}
```

**Data flow — Session end:**
1. Claude Code session terminates → `SessionEnd` MCP tool hook fires with `{transcript_ref, session_id, repo_path}`
2. Calls `session-extractor.extract_session({transcript_ref, repo_path, scope_hints})`
3. MCP tool returns immediately (`{status: "processing", job_id}`) while background extraction starts:
   - Validates `transcript_ref` stays inside mounted `CLAUDE_TRANSCRIPT_ROOT`
   - Reads transcript JSONL from the mounted transcript root
   - Invokes headless Claude Code with transcript + extraction prompt
   - Analyzes session for: patterns worth capturing, conventions used, procedures followed
   - Generates `.pending` draft SKILL.md files in appropriate scope directory
   - Includes suggested tags (if no existing tags match) in `.pending` file frontmatter
4. session-extractor publishes `skill.extraction_requested` when the job is accepted, then `extraction.completed` or `extraction.failed` on background completion
5. Developer sees `.pending` files in their scope directory → review → rename to `.md` (approve) or delete / mark `.rejected` (reject)
6. Filesystem watcher detects rename or periodic reconciliation scan notices the state transition → publishes `skill.file_changed` with an idempotency key
7. graph-builder consumes event → incremental rebuild for affected scope

**Data flow — Offline maintenance:**
1. Cron trigger (or filesystem watcher) starts graph builder
2. Subunit extraction: structural rules (headings → procedures, code blocks → assets, lists → conventions) for 80% + Ollama fallback (JSON `{type, content, source_heading}`) for unstructured skills
3. Subunit deduplication: exact-match within scope, cosine-similarity (threshold 0.85) cross-scope
4. Embedding generation: all skill descriptions + subunit texts → Ollama → Qdrant
5. Community detection: HDBSCAN on skill embeddings. Tags augment (dual membership — skill belongs to HDBSCAN community AND tag-based communities). Many-to-many `community_skills`.
6. Cross-scope deduplication: cosine > 0.85 between project and global skill embeddings → LLM semantic equivalence check → merge into consolidated SKILL.md
7. Retirement: recency-weighted usage score from session logs. Below 1/month threshold → `.retired` marker. Human reviews, removes `.retired` to keep or leaves to retire
8. Publishes `graph.rebuilt` only after outbox drain + durable `graph_version` bump → mcp-server invalidates cache by version mismatch

#### Key Decisions Carried Forward

| Decision | Value | Rationale |
|---|---|---|
| Language | Rust (tokio async) | Zero-cost latency for sync MCP server |
| Community detection | HDBSCAN + tag augmentation | Variable clusters + noise handling + human override |
| Embedding model | nomic-embed-text (768-dim) | Code-aware embeddings, configurable via env |
| Fusion order | MMR per-scope first, then RRF cross-scope | Deduplicate within scope before merging |
| Retrieval budget | Relevance-threshold driven | Adapts to graph size, no fixed K |
| Compilation guidance | Template-only V1 | Keeps MCP server under 500ms |
| Human approval | Filesystem-based (.pending/.retired) | Zero UI, portable, observable |
| Community membership | Dual (HDBSCAN + tag-based), many-to-many | Tags don't erase algorithmic clusters |
| Graph queries | Recursive CTEs in PG | Multi-hop traversal without denormalization |
| Service comms | Redis Streams | Decoupled, event-driven, replayable |
| Ollama extraction | Hybrid (rules 80% + JSON fallback) | Fast for structured, robust for unstructured |
| Scope discovery | Project: recursive git root scan. Global: env var path array | Flexible across harnesses |
| Cold start | `no_match`, not fake success | Don't force irrelevant content and don't hide healthy emptiness behind degraded emptiness |
| Failure mode | Explicit degrade + retry with backoff | MCP server never crashes, but degraded trust is surfaced in the tool contract |

#### Database Schema (PostgreSQL)

```sql
-- Core entities
CREATE TABLE skills (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope TEXT NOT NULL CHECK (scope IN ('project', 'global')),
    scope_path TEXT NOT NULL,
    file_path TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    content_hash TEXT NOT NULL,
    tags TEXT[] NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'retired', 'merged', 'pending')),
    embedding_id UUID,
    merged_into UUID REFERENCES skills(id),
    merged_from_scopes TEXT[] NOT NULL DEFAULT '{}',
    reference_paths TEXT[] NOT NULL DEFAULT '{}',
    last_vector_sync_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(scope_path, content_hash)
);

CREATE TABLE subunits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    type TEXT NOT NULL CHECK (type IN ('procedure', 'convention', 'asset')),
    content TEXT NOT NULL,
    content_hash TEXT NOT NULL UNIQUE,
    embedding_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE communities (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    label TEXT NOT NULL,
    centroid_embedding_id UUID,
    scope TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Junction tables
CREATE TABLE skill_subunits (
    skill_id UUID REFERENCES skills(id) ON DELETE CASCADE,
    subunit_id UUID REFERENCES subunits(id) ON DELETE CASCADE,
    is_highlight BOOLEAN NOT NULL DEFAULT false,
    extraction_source TEXT,   -- 'rule' | 'ollama'
    PRIMARY KEY (skill_id, subunit_id)
);

CREATE TABLE community_skills (
    community_id UUID REFERENCES communities(id) ON DELETE CASCADE,
    skill_id UUID REFERENCES skills(id) ON DELETE CASCADE,
    source TEXT NOT NULL CHECK (source IN ('hdbscan', 'tag')),
    PRIMARY KEY (community_id, skill_id, source)
);

-- Session tracking
CREATE TABLE session_logs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id TEXT NOT NULL,
    repo_path TEXT NOT NULL,
    prompt TEXT,
    compiled_context_hash TEXT,
    retrieval_latency_ms INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE skill_usage (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    skill_id UUID REFERENCES skills(id) ON DELETE SET NULL,
    skill_name TEXT,  -- denormalized for analytics when skill FK becomes NULL
    session_log_id UUID REFERENCES session_logs(id) ON DELETE CASCADE,
    scope TEXT NOT NULL,
    score REAL NOT NULL,
    selected BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Outbox for Qdrant-PG dual-write consistency
CREATE TABLE outbox (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type TEXT NOT NULL CHECK (event_type IN ('qdrant_upsert', 'qdrant_delete')),
    payload JSONB NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processed', 'failed')),
    retry_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ
);
CREATE INDEX idx_outbox_pending ON outbox(created_at) WHERE status = 'pending';

-- Graph rebuild locks (prevent concurrent rebuilds on same scope)
CREATE TABLE rebuild_locks (
    scope_path TEXT PRIMARY KEY,
    worker_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'in_progress',
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    heartbeat TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_rebuild_heartbeat ON rebuild_locks(heartbeat) WHERE status = 'in_progress';

-- Audit
CREATE TABLE audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type TEXT NOT NULL,  -- 'skill.created', 'skill.retired', 'skill.merged', 'community.rebuilt'
    entity_type TEXT NOT NULL,
    entity_id UUID NOT NULL,
    before_snapshot JSONB,
    after_snapshot JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Indexes
CREATE INDEX idx_skills_scope_status ON skills(scope, status);
CREATE INDEX idx_skills_active ON skills(id) WHERE status = 'active';  -- partial index for active-only queries
CREATE INDEX idx_skills_embedding ON skills(embedding_id) WHERE embedding_id IS NOT NULL;
CREATE INDEX idx_skills_merged_scopes ON skills USING GIN(merged_from_scopes);  -- for maintenance cross-scope dedup
CREATE INDEX idx_subunits_type ON subunits(type);
CREATE INDEX idx_community_skills_source ON community_skills(source);
CREATE INDEX idx_skill_usage_skill ON skill_usage(skill_id, created_at);
CREATE INDEX idx_skill_subunits_skill_subunit ON skill_subunits(skill_id, subunit_id);  -- forward traversal
CREATE INDEX idx_skill_subunits_subunit_skill ON skill_subunits(subunit_id, skill_id);  -- reverse (rescue) traversal

-- Auto-update trigger on skills.updated_at
CREATE OR REPLACE FUNCTION update_updated_at()
RETURNS TRIGGER AS $$ BEGIN NEW.updated_at = now(); RETURN NEW; END; $$ LANGUAGE plpgsql;
CREATE TRIGGER skills_updated_at BEFORE UPDATE ON skills
FOR EACH ROW EXECUTE FUNCTION update_updated_at();

-- Event catalog (8 events, single-owner emission)
-- skill.file_changed     | graph-builder watcher  | any filesystem change on SKILL.md/.pending/.retired
-- skill.extraction_requested | session-extractor  | extract_session called
-- extraction.completed   | session-extractor     | .pending file written to disk
-- extraction.failed      | session-extractor     | LLM provider failed after retries
-- graph.rebuilt          | graph-builder         | incremental/full rebuild success, outbox drained
-- graph.rebuild_failed   | graph-builder         | rebuild failed after retries
-- skill.retired          | maintenance           | .retired marker confirmed (audit)
-- skill.merged           | maintenance           | merge approved (audit)

-- Event envelope schema:
-- { event_id: UUIDv7, event_type: string, correlation_id: UUIDv7, idempotency_key: string,
--   schema_version: 1, timestamp: RFC3339, source_service: string, payload: {...} }

-- Idempotency: Redis SETEX key = "idempotency:{idempotency_key}", TTL per event class:
--   Sync events: 24h. Session events: 7d. Maintenance events: 30d. Failed events: 90d.
```

### Research Insights — PG Schema Hardening

**Best Practices** (serves: SC-4 merge/retire, SC-8 V2 readiness):

**P0: Freeze one V1 scope model and keep it consistent.** V1.1 uses scalar `skills.scope` plus `merged_from_scopes TEXT[]` for provenance and maintenance logic. Do not introduce a `skill_scopes` junction table in V1. Team scope remains a V2 additive migration if true many-to-many membership is earned by real usage.

**P0: Use UUIDv7 for all PKs.** UUIDv4 (random) causes severe B-tree index page splits on high-write tables (`session_logs`, `skill_usage`, `audit_log`). UUIDv7 provides time-ordered index locality + embedded timestamp. Rust: `uuid` crate with `uuid7` feature.

**P0: Composite PKs on junction tables.** `PRIMARY KEY (skill_id, subunit_id)` on `skill_subunits`. `PRIMARY KEY (community_id, skill_id, source)` on `community_skills`. Without explicit composite PKs, duplicate edges can be inserted.

**P1: Composite indexes for CTE performance.** The recursive CTE queries join `skill_subunits` in both directions. Need both: `(subunit_id, skill_id)` AND `(skill_id, subunit_id)`. Without these, every recursive step does sequential scan. Add: `CREATE INDEX idx_skill_subunits_subunit_skill ON skill_subunits(subunit_id, skill_id)` and `CREATE INDEX idx_skill_subunits_skill_subunit ON skill_subunits(skill_id, subunit_id)`.

**P1: `TIMESTAMPTZ` on all timestamp columns.** Schema already uses it — ensure no `TIMESTAMP` (without zone) anywhere. Add `updated_at` auto-update trigger on `skills`.

**P1: Content hash uniqueness.** `UNIQUE(content_hash)` on `subunits` is correct. On `skills`, use `UNIQUE(scope_path, content_hash)` for the frozen scalar-scope design.

**P1: `ON DELETE SET NULL` on `skill_usage.skill_id`.** Preserves usage analytics when skills are retired. `ON DELETE CASCADE` on junction FKs prevents orphans.

**P2: Partition `audit_log` by `created_at` range (monthly).** JSONB snapshots bloat rapidly. TOAST compression helps but partitioning prevents unbounded table growth.

**P2: Add `NOT NULL` constraints on all non-optional columns.** At minimum: `skills.name`, `skills.status`, `skills.content_hash`, `subunits.type`, `subunits.content`, `subunits.content_hash`, `communities.label`, junction FKs.

**Anti-patterns to avoid:**
- Don't use PG ENUM types — use `TEXT` with CHECK constraints or lookup tables. ENUMs require `ALTER TYPE` DDL to extend.
- Don't reference Qdrant point IDs with FK constraints — treat Qdrant as eventually consistent. Run reconciliation job.
- Don't query `community_skills` without `DISTINCT` on `community_id` — HDBSCAN + tag dual membership double-counts without it.

#### Redis Event Streams

| Event | Publisher | Subscribers | Payload |
|---|---|---|---|
| `skill.file_changed` | graph-builder watcher | graph-builder | `{scope, file_path, change_type, content_hash, source: "direct"\|"pending_approval"}` |
| `skill.extraction_requested` | session-extractor | (log, audit) | `{session_id, transcript_hash, correlation_id}` |
| `extraction.completed` | session-extractor | (log, audit) | `{session_id, file_path, candidate_count, correlation_id}` |
| `extraction.failed` | session-extractor | (log) | `{session_id, error_code, retry_count, correlation_id}` |
| `graph.rebuilt` | graph-builder | mcp-server | `{scope, skills_count, communities_count, duration_ms, graph_version}` |
| `graph.rebuild_failed` | graph-builder | (log) | `{scope, error_msg, retry_count}` |
| `skill.retired` | maintenance | (audit) | `{skill_id, file_path, retirement_reason}` |
| `skill.merged` | maintenance | (audit) | `{target_skill_id, source_skill_ids[], file_path}` |

**Event envelope:** All events use `{event_id: UUIDv7, event_type: string, correlation_id: UUIDv7, idempotency_key: string, schema_version: 1, timestamp: RFC3339, source_service: string, payload: {...}}`. Idempotency tracked via Redis SETEX with event-class TTL (24h sync, 7d session, 30d maintenance). Consumer groups with ACK + XAUTOCLAIM every 5s for orphaned messages. Dead letter queue `{stream}:dlq` after 3 delivery attempts.

**`.pending`→`.md` rename resolution:** `skill.file_changed` emitted by filesystem watcher with `source: "pending_approval"` in payload. No separate `skill.approved` event — the rename IS the approval per constitution §5. Session-extractor extractions and maintenance merge proposals both use `.pending` extension; origin (`session_extraction` vs `merge_proposal`) is frontmatter metadata, not a separate event type.

**Canonical invalidation contract:** `graph.rebuilt` is emitted only after PG writes commit, outbox relay drains all vectors for that rebuild, and the new `graph_version` is durable. `mcp-server` caches compiled context by `(prompt_hash, scope_fingerprint, graph_version)`, so old cache entries naturally fall out on version mismatch. No other event invalidates compiled context.

#### MCP Tool Surface

| Tool | Called by | Purpose |
|---|---|---|
| `compile_context` | UserPromptSubmit hook | Receives prompt text → returns structured compile result envelope |
| `extract_session` | SessionEnd hook | Receives `transcript_ref` under mounted transcript root → initiates async extraction |
| `find_skill` | Developer / debug | Natural language + tag search across scopes |
| `rebuild_graph` | Developer / cron | Triggers full graph rebuild for a scope |
| `inspect_skill` | Developer / debug | Returns skill details, subunits, communities |
| `list_communities` | Developer / debug | Lists all communities with skill counts |
| `get_pending_extractions` | Developer / debug | Lists pending extraction proposals and lifecycle metadata |

#### `compile_context` result contract

- `status: "ok"` — healthy retrieval produced non-empty compiled context; hook injects `additional_context`; session suppression is set
- `status: "no_match"` — healthy retrieval found no relevant skills; hook injects nothing; session suppression is still set because the attempt was trustworthy
- `status: "degraded"` — one or more dependencies/scopes failed; hook injects nothing unless partial context policy is explicitly enabled; session suppression is **not** set
- `status: "duplicate_suppressed"` — a prior healthy result already consumed the first-prompt opportunity for `{session_id, repo_path}`
- `reason_code` is mandatory for non-`ok` outcomes
- `health` carries per-dependency markers (`ollama`, `qdrant`, `postgres`, `redis`, `filesystem_index`)

#### `extract_session` transcript contract

- Primary V1.1 input: `transcript_ref`, a relative path rooted under `CLAUDE_TRANSCRIPT_ROOT`
- `CLAUDE_TRANSCRIPT_ROOT` is bind-mounted into the container read-only; the service rejects traversal, absolute paths, and refs outside that root
- Optional `transcript_inline` is supported for tests and future harnesses that can stream transcript content directly
- Raw host `transcript_path` is **not** a valid service contract in V1.1

### Execution Slices

#### Phase 1: Tracer Bullet — Compile Context End-to-End
**Purpose:** Prove that a Claude Code session can receive auto-compiled skill context from the MCP server. This is the thinnest vertical slice: Docker Compose infra → MCP server → retrieval → compilation → Claude Code integration.
**Rationale:** Until this works, nothing else matters. The tracer bullet validates all infrastructure, the MCP protocol, the retrieval pipeline, and the compilation output format.

##### Slice 1.1a: Docker Compose Infrastructure + Domain Crate
**Slice type:** tracer-bullet (infra foundation necessary before service code)
**Serves:** SC-7 (graceful degrade — containers must start before anything runs), SC-8 (V2 readiness)
**Demo scenario:** `docker compose up` starts all 5 containers. `cargo test --workspace` passes with domain having ZERO infrastructure deps. `cargo tree -p domain --depth 1` shows only serde/strum/thiserror/async-trait/tokio.
**Feature home:** Root `docker-compose.yml`, `crates/domain/`
**Files:**
- `docker-compose.yml` — 5 services: ollama (nomic-embed-text), qdrant, postgres, redis, placeholder
- `docker-compose.test.yml` — test overrides
- `docker-compose.override.yml` — dev overrides (hot reload)
- `.env.example` — all configurable env vars with defaults
- `crates/domain/Cargo.toml` — ZERO deps on sqlx/qdrant-client/redis/reqwest
- `crates/domain/src/lib.rs` — re-exports
- `crates/domain/src/types.rs` — Skill, Subunit, Community, Scope, SkillStatus, SubunitType, ExtractionResult, ScoredSkill, ScopeDescriptor
- `crates/domain/src/traits.rs` — EmbeddingService, TranscriptSkillExtractionService, ScopeResolver, ContextCompiler
- `crates/domain/src/errors.rs` — DomainError enum (thiserror)
- `crates/domain/src/config.rs` — typed config structs (no env-var parsing — that's infrastructure)
**Depends on:** None
**Dependency type:** parallel-safe
**Blast radius:** low (greenfield)

##### Slice 1.1b: Infrastructure Crate + PG Schema + Outbox
**Slice type:** tracer-bullet (infrastructure unlock for all downstream services)
**Serves:** SC-7 (graceful degrade), SC-4 (merge/retire data integrity), SC-5 (filesystem observability)
**Demo scenario:** `docker compose up`. PG migration runs creating full schema including outbox table. `infrastructure` tests connect to Docker containers, verify schema, test Ollama/Redis/Qdrant connectivity.
**Feature home:** `crates/infrastructure/`
**Files:**
- `crates/infrastructure/Cargo.toml` — depends on domain + sqlx/qdrant-client/redis/reqwest
- `crates/infrastructure/src/lib.rs`
- `crates/infrastructure/src/embeddings/ollama.rs` — OllamaEmbeddingService (implements EmbeddingService)
- `crates/infrastructure/src/extraction/claude.rs` — ClaudeExtractor (implements TranscriptSkillExtractionService)
- `crates/infrastructure/src/extraction/ollama.rs` — OllamaExtractor (optional, config-gated)
- `crates/infrastructure/src/persistence/postgres.rs` — PG pool setup, migration runner
- `crates/infrastructure/src/persistence/outbox.rs` — GraphWriteCoordinator (atomic PG+outbox writes)
- `crates/infrastructure/src/persistence/rebuild.rs` — RebuildCoordinator (lock acquire/heartbeat/complete/fail)
- `crates/infrastructure/src/streaming/redis.rs` — Redis Streams publisher/subscriber with DLQ
- `crates/infrastructure/src/scope.rs` — GitRootProjectResolver, EnvPathGlobalResolver (implement ScopeResolver)
- `crates/infrastructure/src/resilience.rs` — retry with backoff, circuit breaker
- `crates/infrastructure/src/health.rs` — health check endpoint (per-component status)
- `crates/infrastructure/src/logging.rs` — structured JSON logging (tracing-subscriber)
- `crates/infrastructure/migrations/001_initial_schema.sql` — full PG schema with outbox, rebuild_locks, composite indexes, UUIDv7 PKs
**Depends on:** Slice 1.1a (domain types/traits)
**Dependency type:** real

###### What to build (Slice 1.1a)
`domain` crate with pure domain types (Skill, Subunit, Community, Scope, SkillStatus, SubunitType), traits (EmbeddingService, TranscriptSkillExtractionService, ScopeResolver, ContextCompiler), configuration structs, and domain errors. ZERO infrastructure dependencies — verified by `cargo tree -p domain --depth 1` showing only serde/strum/thiserror/async-trait/tokio.

Docker Compose configuration for all 5 infrastructure containers.

###### Scope (Slice 1.1a)
- **Owns:** Docker Compose configuration, domain crate (pure types + traits + config), PG schema migration file
- **Non-goals:** Any infrastructure code. Any service logic. No concrete Ollama/PG/Redis/Qdrant implementations
- **Scope fence:** domain must not import sqlx, qdrant-client, redis, or reqwest

###### What to build (Slice 1.1b)
`infrastructure` crate with all concrete adapters: OllamaEmbeddingService, ClaudeExtractor, PG pool, Redis streams, GraphWriteCoordinator (outbox), RebuildCoordinator, scope resolvers, resilience utilities, structured logging. Full PG schema migration with outbox table, composite indexes, auto-update trigger.

###### Scope (Slice 1.1b)
- **Owns:** All concrete external-system adapters. PG pool + migrations. Redis pub/sub. Ollama/Claude clients. Outbox pattern. Rebuild locks. Resilience. Health checks. Logging.
- **Non-goals:** Service-level orchestration, MCP protocol, retrieval pipeline, compilation, graph construction
- **Scope fence:** Must re-export domain types for downstream consumers. Must NOT contain business logic — only adapter implementations

###### Acceptance criteria
- [ ] `docker compose up` starts all 5 containers without errors
- [ ] `cargo tree -p domain --depth 1` shows ZERO sqlx/qdrant-client/redis/reqwest deps (CI gate)
- [ ] PG schema migration runs on first start (tables + outbox + indexes + trigger exist after `docker compose up`)
- [ ] UUIDv7 generation verified on all PKs via `pg_uuidv7` extension or application-side UUIDv7
- [ ] outbox table with correct schema, `idx_outbox_pending` partial index
- [ ] `skill_subunits` bidirectional composite indexes exist
- [ ] `skills_updated_at` trigger fires on UPDATE
- [ ] `skill_usage.skill_id` uses `ON DELETE SET NULL` (not CASCADE)
- [ ] Ollama container serves embeddings (test: `curl ollama:11434/api/embeddings`)
- [ ] Qdrant accepts vector writes (test: create collection, upsert point, search)
- [ ] Redis Streams accepts events (test: publish → consumer group read → ACK)
- [ ] `cargo build --workspace` succeeds
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --check --all` passes

###### Evidence
- **Test command:** `cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit`
- **Evidence focus:** Proves infrastructure containers start, PG migrations apply, Ollama serves embeddings, Qdrant accepts vectors, Redis streams events

##### Slice 1.2: MCP Server — Compile Context (Single Scope, No Graph)
**Slice type:** tracer-bullet (first user-visible behavior)
**Serves:** SC-1 (zero-touch context injection), SC-2 (retrieval pipeline — single scope proof)
**Demo scenario:** Developer runs `docker compose up`. Opens Claude Code (pointed at local MCP server). Types "how do I write a rust function that reads a file?" → MCP server receives the prompt, performs retrieval from global scope, compiles context, returns structured markdown — all within 500ms.
**Feature home:** `crates/mcp-server/`, `crates/retrieval/`, `crates/compiler/`
**Files:**
- `crates/mcp-server/Cargo.toml`
- `crates/mcp-server/src/main.rs` — MCP server bootstrap, tool registration, router composition
- `crates/mcp-server/src/tools/compile_context.rs` — thin handler → delegates to retrieval + compiler crates
- `crates/mcp-server/src/tools/find_skill.rs` — thin handler
- `crates/mcp-server/src/state.rs` — session-scoped state (DashMap + Redis SETEX write-through)
- `crates/retrieval/Cargo.toml`
- `crates/retrieval/src/lib.rs`
- `crates/retrieval/src/orchestrator.rs` — single-scope retrieval orchestration
- `crates/retrieval/src/scoring.rs` — skill scoring (paper eq.3)
- `crates/retrieval/src/qdrant_search.rs` — Qdrant vector search
- `crates/retrieval/src/graph_search.rs` — PG recursive CTE with LIMIT 50 + relevance pruning
- `crates/retrieval/src/fusion.rs` — MMR primitives used before cross-scope RRF
- `crates/compiler/Cargo.toml`
- `crates/compiler/src/lib.rs`
- `crates/compiler/src/template.rs` — structured markdown template
- `crates/compiler/src/rescue.rs` — rescue-aware subunit attachment
- `tests/integration/test_compile_context.rs` — e2e: prompt in, context out
- `tests/fixtures/test-skills/` — test SKILL.md files for retrieval testing
**Depends on:** Slice 1.1b (infrastructure)
**Dependency type:** real
**Blast radius:** low
**Shared state changes:** None (reads from PG/Qdrant)
**Rollback path:** Stop MCP server container, no data written

###### What to build
Complete MCP server (`rmcp` crate) with thin tool handlers delegating into `retrieval` and `compiler` crates. Single-scope retrieval (global scope only). Retrieval pipeline: prompt embedding via Ollama → top-down community matching (Qdrant) + bottom-up subunit projection (Qdrant + PG CTE with LIMIT 50 + relevance pruning) → combined scoring per SkillRAE paper eq.3 → relevance-threshold filtering → MMR deduplication (hard cap 50 Qdrant results per scope) → evidence export (top-3 subunit highlights) → rescue → template compilation.

`compile_context` returns the canonical result envelope:
- `ok` with `additional_context`
- `no_match` when healthy but empty
- `degraded` with `reason_code` + `health`
- `duplicate_suppressed` after a prior healthy outcome for `{session_id, repo_path}`

Session state: dual-tier DashMap (hot path) + Redis SETEX async write-through (crash recovery, 24h TTL). Compiled context cache: key `(prompt_hash, scope_fingerprint, graph_version)`, TTL 5min, skips full pipeline for repeated prompts. Cold-start short-circuit: `SELECT COUNT(*) WHERE status='active'` → zero → return `no_match` immediately.

For this slice, the graph is seeded manually (test SKILL.md files loaded by migration). No filesystem watcher or graph builder yet.

###### Scope
- **Owns:** MCP server binary + tool contract, retrieval crate, compiler crate, single-scope retrieval pipeline, scoring formula, MMR deduplication, rescue attachment, template compilation, session state tracking
- **Non-goals:** Dual-scope retrieval (global only), project scope detection, filesystem watcher, graph builder integration, session-end hook processing, admin tools (rebuild/inspect/list/approve)
- **Scope fence:** Do not add project scope retrieval. Do not add hooks for UserPromptSubmit/SessionEnd — those are configuration in `.claude/settings.json` (documented, not implemented in Rust). Do not add LLM-synthesized guidance (template-only). Tool handlers must remain thin; business logic lives in `retrieval` and `compiler`

###### Acceptance criteria
- [ ] MCP server starts and registers `compile_context` and `find_skill` tools
- [ ] `compile_context("how do I read a file in rust")` returns `status = "ok"` and structured markdown context from seeded test data
- [ ] Retrieval uses Ollama embeddings (verified: embedding dimension = 768)
- [ ] Skill scoring follows paper eq.3: `(α·ℓ₁ + β·ℓ₀ + γ·p) × (1 + λ·community_boost)`
- [ ] MMR deduplication runs before RRF (within single scope in this slice)
- [ ] Rescue attaches non-selected-skill subunits to selected skills when relevance > threshold
- [ ] Compiled context follows template format (skill name, description, highlighted subunits, rescue cues)
- [ ] Second `compile_context` call for same `{session_id, repo_path}` returns `duplicate_suppressed` after a healthy first outcome
- [ ] Target latency <500ms (measured by embedded timer in test)
- [ ] Graceful degrade: if Ollama unreachable, returns `degraded` with reason/health markers (doesn't crash)
- [ ] Healthy empty path: if PG/Qdrant contain no skills, returns `no_match`
- [ ] Degraded first attempt does not set suppression; later healthy retry still runs
- [ ] `cargo test --workspace` passes with Red→Green→Post-Refactor-Green evidence
- [ ] `cargo clippy --workspace -- -D warnings` passes

###### Evidence
- **Test command:** `cargo test --workspace`
- **Evidence focus:** Unit tests for scoring formula, MMR, rescue, template, and result-status transitions. Integration test: seed test skills → call compile_context → verify structured markdown output + envelope semantics. Latency benchmark test. Graceful degrade tests (mock unavailable Ollama).

##### Slice 1.3: Claude Code Hook Integration + Dual Scope Retrieval
**Slice type:** expansion
**Serves:** SC-2 (dual-scope concurrent retrieval with MMR-then-RRF), SC-1 (actual Claude Code integration)
**Demo scenario:** Developer configures `.claude/settings.json` with UserPromptSubmit hook pointing to MCP server. Opens Claude Code in project `/home/rabak/projects/foo`. Types "fix auth bug." MCP server receives prompt, concurrently searches project scope (from git root cwd) AND global scope (from env var paths), applies MMR per-scope then RRF cross-scope fusion, returns compiled context. Developer sees skill context injected in Claude Code session.
**Feature home:** `crates/retrieval/`, `crates/mcp-server/`, `.claude/settings.json` (example)
**Files:**
- `crates/retrieval/src/dual_scope.rs` — concurrent scope search (tokio::join!)
- `crates/retrieval/src/scope_resolution.rs` — delegates to ScopeResolver impls from infrastructure
- `crates/retrieval/src/fusion.rs` — extend with RRF cross-scope (after per-scope MMR)
- `crates/infrastructure/src/scope.rs` — git root detection and global path resolution impls
- `crates/mcp-server/src/tools/compile_context.rs` — enforce healthy-result-only suppression semantics
- `config/claude-code/hooks.example.json` — example `.claude/settings.json` hook config
- `tests/integration/test_dual_scope.rs` — dual-scope retrieval + cross-scope fusion tests
**Depends on:** Slice 1.2
**Dependency type:** real
**Blast radius:** low
**Shared state changes:** None
**Rollback path:** Remove hook config from `.claude/settings.json`

###### What to build
Extend retrieval from single-scope to dual-scope. Concurrent retrieval: `tokio::join!` for project and global scope searches. Project scope detected from git root (cwd from hook payload). Global scope from `SKILL_GLOBAL_PATHS` env var (comma-separated path array). RRF cross-scope fusion applied after per-scope MMR deduplication. Weighted by scope priority (project scope weight 1.0, global scope weight configurable, default 0.7). Example Claude Code hook configuration documented. The hook example must honor status semantics: inject only on `ok`, suppress only after healthy result, retry later after `degraded`.

###### Scope
- **Owns:** Dual-scope concurrent retrieval, scope resolution, RRF cross-scope fusion, Claude Code hook configuration docs
- **Non-goals:** SessionEnd hook (Phase 2), filesystem watcher, graph builder integration
- **Scope fence:** Do not implement project scope skill extraction or graph construction. Skills must be manually seeded for this slice

###### Acceptance criteria
- [ ] Dual-scope retrieval runs project + global searches concurrently (verified: total time ≤ max(project_latency, global_latency) + overhead)
- [ ] Project scope detected from git root (cwd from hook payload or env var)
- [ ] Global scope paths read from `SKILL_GLOBAL_PATHS` env var
- [ ] RRF cross-scope fusion correctly merges two ranked lists (point: same skill ranks high in both scopes → boosted)
- [ ] Scope weighting applied (project skills weighted higher than global)
- [ ] Claude Code `.claude/settings.json` hook example works end-to-end (manual verification)
- [ ] Different session_id produces independent state (two repos, two sessions, both get first-prompt context)
- [ ] Same session_id + same repo_path: second prompt returns `duplicate_suppressed` after a healthy first outcome
- [ ] If the first prompt returns `degraded`, the second prompt still performs retrieval instead of being suppressed
- [ ] `cargo test --workspace` passes

###### Evidence
- **Test command:** `cargo test --workspace`
- **Evidence focus:** Unit tests for RRF fusion with weighted lists. Integration test: seed project + global test skills → call compile_context → verify both scopes searched, RRF merged, correct ordering. Session state isolation test.

#### Phase 2: Core Widening — Graph Builder + Session Extraction
**Purpose:** Complete the self-growing loop. Phase 1 proved context injection works with manually seeded skills. Phase 2 builds the offline graph construction (so skills are discovered automatically) and session-end extraction (so skills are created automatically with human approval).
**Rationale:** These two services together close the feedback loop: sessions consume context from the graph, sessions produce new skills that grow the graph. The tracer bullet proved the online path; Phase 2 widens to the offline and post-session paths.

##### Slice 2.1: Filesystem Watcher + Incremental Graph Rebuild
**Slice type:** expansion
**Serves:** SC-4 (offline graph maintenance — incremental rebuild on file change), SC-5 (filesystem-observable state)
**Demo scenario:** Developer writes a new SKILL.md in `.skills/rust-file-io/`. Filesystem watcher detects the change, publishes `skill.file_changed`. Graph builder consumes event, acquires rebuild lock, runs incremental rebuild: extracts subunits, generates embedding, writes to PG+outbox (Qdrant relayed asynchronously), runs HDBSCAN, publishes `graph.rebuilt` only after outbox drained. MCP server invalidates cache on `graph.rebuilt`.
**Feature home:** `crates/graph-builder/`
**Files:**
- `crates/graph-builder/Cargo.toml`
- `crates/graph-builder/src/main.rs` — graph builder bootstrap, event loop, build_orchestrator (delegates to RebuildCoordinator for lock + GraphWriteCoordinator for outbox writes)
- `crates/graph-builder/src/watcher.rs` — filesystem watcher (notify-debouncer-full + FileIdMap, PollWatcher fallback for WSL/Docker/NFS)
- `crates/graph-builder/src/watcher_recovery.rs` — startup recovery: scan rebuild_locks, claim stale Redis messages, rescan missed files
- `crates/graph-builder/src/extraction/mod.rs` — subunit extraction orchestrator
- `crates/graph-builder/src/extraction/rules.rs` — deterministic structural rules
- `crates/graph-builder/src/extraction/ollama_fallback.rs` — Ollama JSON extraction fallback
- `crates/graph-builder/src/extraction/dedup.rs` — subunit deduplication
- `crates/graph-builder/src/graph/build.rs` — graph construction (skill → subunit edges, community assignment)
- `crates/graph-builder/src/graph/communities.rs` — HDBSCAN clustering + tag augmentation
- `crates/graph-builder/src/graph/embeddings.rs` — batch embedding generation (non-blocking semaphore: 3 slots, 1 reserved for sync)
- `crates/graph-builder/src/graph/rebuild.rs` — incremental vs full rebuild logic
- `tests/integration/test_watcher_rebuild.rs` — file change → rebuild → graph updated
- `tests/fixtures/test-skills/` — test SKILL.md files for extraction testing
**Depends on:** Slice 1.3 (infrastructure + MCP server already running)
**Dependency type:** real
**Blast radius:** medium (writes to PG/Qdrant/filesystem)
**Shared state changes:** PG skill/subunit/community tables, Qdrant vectors, SKILL.md.retired files
**Rollback path:** Stop graph builder, revert generated files by running a clean rebuild

###### What to build
Complete `graph-builder` service. Filesystem watcher using `notify` crate: watches project scope (git root recursively) and global scope (all paths from env var). Detects new/modified/deleted SKILL.md and `.pending`→`.md` renames. Publishes `skill.file_changed` to Redis.

Incremental rebuild: on file change, only reprocess affected files. Subunit extraction: deterministic rules (headings → procedures, code blocks → assets, numbered/bullet lists under procedural headings → conventions). For skills where structural rules produce <2 subunits OR skill has no clear heading structure, Ollama fallback generates JSON `[{type, content, source_heading}]`. Subunits deduplicated by content hash within scope, then cross-scope cosine check.

Embedding generation: batch send all new/modified skill descriptions + subunit texts to Ollama. Store in Qdrant with scope payload tag.

Community detection: HDBSCAN on skill embeddings (using `linfa-clustering` crate or calling Python script via subprocess). Tags from skill frontmatter create additional `community_skills` entries (source: 'tag'). Skills can belong to multiple communities via dual membership.

###### Scope
- **Owns:** Filesystem watcher, subunit extraction (rules + Ollama fallback), embedding generation, community detection (HDBSCAN + tags), incremental graph rebuild, `skill.file_changed` + `graph.rebuilt` event publishing
- **Non-goals:** Cross-scope merge, retirement, full cron rebuild (Slice 2.3), session-end hook processing
- **Scope fence:** Do not implement merge/retire logic. Do not implement periodic cron trigger. Incremental rebuild only on file change events, plus reconciliation scans that recover missed watcher transitions

###### Acceptance criteria
- [ ] Filesystem watcher detects new SKILL.md in project scope (git root) within 2 seconds
- [ ] Filesystem watcher detects `.pending` → `.md` rename as skill creation
- [ ] Reconciliation scan detects missed rename/delete transitions and emits equivalent `skill.file_changed` events idempotently
- [ ] Structural rules extract subunits from skills with headings, code blocks, and lists
- [ ] Ollama fallback produces valid JSON `[{type, content, source_heading}]` for unstructured skills
- [ ] Subunits deduplicated by content hash (exact match) within scope
- [ ] Embeddings generated and stored in Qdrant with scope payload filter
- [ ] HDBSCAN forms communities from skill embeddings
- [ ] Tags from frontmatter create additional community memberships (dual membership)
- [ ] `skill.file_changed` event published on file change
- [ ] `graph.rebuilt` event published on rebuild completion
- [ ] `graph.rebuilt` emitted only after outbox drain + durable `graph_version` bump
- [ ] MCP server receives `graph.rebuilt` and invalidates cache
- [ ] All graph mutations (skill creation, subunit extraction, community assignment) produce `audit_log` entries with before/after JSONB snapshots (constitution §74)
- [ ] `cargo test --workspace` passes

###### Evidence
- **Test command:** `cargo test --workspace`
- **Evidence focus:** Unit tests for rule extraction, JSON extraction parsing, dedup logic, HDBSCAN clustering. Integration test: write test SKILL.md → wait for watcher → verify graph contains skill + subunits + community assignment. Verify Qdrant contains embedding. Verify `graph.rebuilt` published.

##### Slice 2.2: Session-End Extraction + Human Approval Workflow
**Slice type:** expansion
**Serves:** SC-3 (session-end skill extraction with human approval)
**Demo scenario:** Developer finishes a Claude Code session. Session ends. `SessionEnd` MCP hook fires → `extract_session` called. Minutes later, `.skills/setup-rust-project.md.pending` appears. Developer reviews, renames to `.md`. File appears as active skill in next session's context.
**Feature home:** `crates/session-extractor/`, `crates/mcp-server/`
**Files:**
- `crates/session-extractor/Cargo.toml`
- `crates/session-extractor/src/lib.rs`
- `crates/session-extractor/src/transcripts.rs` — transcript_ref validation + JSONL transcript reader/parser
- `crates/session-extractor/src/providers/claude.rs` — default extraction provider
- `crates/session-extractor/src/providers/ollama.rs` — optional extraction provider
- `crates/session-extractor/src/writer.rs` — .pending file writer with YAML frontmatter (includes origin, created_at, expires_at, tags)
- `crates/mcp-server/src/tools/extract_session.rs` — thin MCP tool handler (returns immediately)
- `tests/integration/test_extract_session.rs` — transcript → .pending file
- `tests/fixtures/sample-transcript.jsonl` — sample Claude Code session transcript
**Depends on:** Slice 2.1 (graph builder + watcher must exist to consume .pending → .md renames)
**Dependency type:** real
**Blast radius:** medium (writes .pending files to filesystem)
**Shared state changes:** Writes .pending files. Graph builder processes them after rename
**Rollback path:** Delete .pending files. Approved skills (renamed to .md) are handled by normal graph lifecycle

###### What to build
`session-extractor` crate exposing extraction logic behind the `TranscriptSkillExtractionService` trait defined in `domain`, with two concrete implementations routed by config (`provider` field): **headless Claude Code** (default, `claude --print --output-format json`) and **Ollama** (optional, gated by `provider: ollama`). Both produce the same JSON output schema `{name, description, tags, procedures[], conventions[], assets[]}`. Config fields: `provider` (claude|ollama), `model` (for Ollama), `endpoint` (for Ollama).

On call: returns immediately (`{status: "processing", job_id}`). Background task validates `transcript_ref`, resolves it under `CLAUDE_TRANSCRIPT_ROOT`, reads transcript JSONL, invokes configured extraction provider, parses output, generates `.pending` SKILL.md draft files with YAML frontmatter (name, description, tags, source: session extraction). Writes to appropriate scope directory (project scope if repo context detected, global otherwise). Suggested tags from extraction included in frontmatter. Publishes `skill.extraction_requested` on accept and `extraction.completed` / `extraction.failed` on background completion.

Filesystem IS the approval UI: `.pending` files are reviewed by developer. Rename to `.md` = approve. Delete = reject. No MCP tools for approval — constitution §5 mandates filesystem-observable state as the approval mechanism. `get_pending_extractions` MCP tool remains as a read-only convenience for listing pending files.

###### Scope
- **Owns:** extract_session tool contract, transcript parsing, transcript-root trust boundary, TranscriptSkillExtractionService implementations, .pending file generation
- **Non-goals:** Transcript analysis quality tuning, multi-harness extraction (Claude Code only V1), automated approval
- **Scope fence:** Do not auto-approve. Do not analyze transcripts in real-time during session. Do not handle non-Claude-Code transcripts

###### Acceptance criteria
- [ ] `extract_session(transcript_ref)` returns immediately with job_id
- [ ] Raw absolute host paths are rejected by the service contract
- [ ] Background task reads JSONL transcript successfully
- [ ] Config `provider: claude` routes to headless Claude Code (default)
- [ ] Config `provider: ollama` routes to Ollama extraction
- [ ] Both providers produce valid JSON `{name, description, tags, procedures[], conventions[], assets[]}`
- [ ] `.pending` SKILL.md file generated with correct YAML frontmatter
- [ ] Suggested tags included in frontmatter (from extraction analysis)
- [ ] `.pending` file written to correct scope directory
- [ ] `get_pending_extractions()` lists all `.pending` files with metadata (read-only)
- [ ] `skill.extraction_requested` emitted on job acceptance; `extraction.completed` or `extraction.failed` emitted on background completion
- [ ] Rename `.pending` → `.md` triggers filesystem watcher → incremental graph rebuild
- [ ] Delete `.pending` rejects the extraction (filesystem-based approval)
- [ ] `cargo test --workspace` passes

###### Evidence
- **Test command:** `cargo test --workspace`
- **Evidence focus:** Unit tests for JSONL parsing, .pending file generation, approval flow. Integration test: feed sample transcript → verify .pending file created with correct frontmatter → rename → verify watcher triggered → verify graph builder indexed skill.

##### Slice 2.3: Offline Maintenance — Merge, Retire, Cron Rebuild
**Slice type:** expansion
**Serves:** SC-4 (offline graph maintenance — merge + retire), SC-5 (filesystem-observable retirement)
**Demo scenario:** Three months in, global scope has 150 skills. Cron triggers full maintenance pass. HDBSCAN detects `rust-cli-parser-setup` and `cargo-init-config` are 0.88 cosine similar. LLM confirms semantic equivalence. Produces merged `.pending` file and `.retired` markers on source files. Also detects `legacy-python2-patterns` hasn't been used in 4 months. Marks `.retired` as proposal. Developer reviews, confirms retirements, deletes py2 pattern, keeps merged skill.
**Feature home:** `crates/maintenance/`, `crates/admin/`
**Files:**
- `crates/maintenance/Cargo.toml`
- `crates/maintenance/src/lib.rs`
- `crates/maintenance/src/merge.rs` — cross-scope dedup + LLM semantic check + merged `.pending` generation
- `crates/maintenance/src/retire.rs` — usage scoring + `.retired` proposal generation
- `crates/maintenance/src/cron.rs` — periodic full maintenance pass (default: daily at 3am)
- `crates/maintenance/src/cleanup.rs` — `.pending` warning scan + reconciliation support
- `crates/admin/Cargo.toml`
- `crates/admin/src/lib.rs`
- `crates/admin/src/tools.rs` — rebuild_graph, inspect_skill, list_communities, get_pending_extractions
- `tests/integration/test_merge_workflow.rs`
- `tests/integration/test_retire_workflow.rs`
**Depends on:** Slice 2.1 (graph builder exists, incremental rebuild works)
**Dependency type:** real
**Blast radius:** medium (modifies SKILL.md files with .retired markers, creates merged files)
**Shared state changes:** .retired markers on files, merged SKILL.md files, PG status updates, audit log entries
**Rollback path:** Remove .retired markers, delete merged files, rebuild from clean

###### What to build
Cross-scope merge detection: during full rebuild, compare all project skill embeddings against global skill embeddings. Cosine similarity > 0.85 → LLM semantic equivalence check (Ollama: "are these two skills describing the same capability?"). If yes → generate consolidated SKILL.md as `.pending` file (not `.md` directly — constitution §3, §5 require human gate). Merged `.pending` contains union of subunits from both source skills. Developer reviews and renames to `.md` to approve. On approval, source skills are marked `.retired`. The merged skill keeps one canonical V1 scope chosen by policy, while `merged_from_scopes TEXT[]` and frontmatter provenance preserve its source history.

Retirement: compute recency-weighted usage score from `skill_usage` table. Below 1/month threshold → flagged. LLM review (Ollama: "is this skill still relevant?") → generates `.retired` marker as a PROPOSAL. Human must explicitly confirm retirement by leaving `.retired` in place (constitution §3: "retirement decision MUST be human-approved"). Remove `.retired` to keep skill active. `.retired` files are excluded from online retrieval but kept in graph.

Cron trigger: configurable interval (default: daily at 3am). Runs full rebuild with merge detection + retirement pass. Both operations produce filesystem-observable proposals (`.pending` for merges, `.retired` for retirements) requiring human confirmation. Also expose `rebuild_graph` MCP tool for manual triggers.

Admin tools: `inspect_skill` (skill details + subunits + communities), `list_communities` (all communities with counts), `rebuild_graph` (manual trigger).

###### Scope
- **Owns:** Merge detection + LLM verification + merged `.pending` file generation, retirement scoring + `.retired` proposal generation, cron scheduler, admin MCP tools
- **Non-goals:** Auto-merge without human review (merge produces `.pending`; human renames to `.md` to approve), auto-retire without human confirmation (`.retired` is a proposal; human leaves in place to confirm)
- **Scope fence:** Do not auto-delete files. Do not merge across scopes without creating a new unified `.pending` file

###### Acceptance criteria
- [ ] Merge detection correctly identifies skill pairs with cosine similarity > 0.85
- [ ] LLM semantic equivalence check returns confirm/reject for merge candidates
- [ ] Merged `.pending` file generated with union of subunits from both source skills
- [ ] Human renames `.pending` → `.md` to approve merge; source skills marked `.retired` on approval
- [ ] Approved merged skill records canonical `scope` plus `merged_from_scopes` provenance (no `skill_scopes` junction table)
- [ ] Retirement scoring correctly computes recency-weighted usage
- [ ] Skills below 1/month threshold flagged for LLM review
- [ ] `.retired` markers created as PROPOSALS for human confirmation
- [ ] Human confirms retirement by leaving `.retired` in place; removes `.retired` to keep active
- [ ] Retired skills excluded from Qdrant retrieval index
- [ ] Cron runs full rebuild at configured interval (merge + retirement as proposals only)
- [ ] Retirement scoring correctly computes recency-weighted usage
- [ ] Skills below 1/month threshold flagged for LLM review
- [ ] `.retired` markers created for confirmed retirements
- [ ] Retired skills excluded from Qdrant retrieval index
- [ ] Cron runs full rebuild at configured interval
- [ ] `rebuild_graph` MCP tool triggers full rebuild on demand
- [ ] `inspect_skill` returns complete skill graph neighborhood
- [ ] `list_communities` returns all communities with member counts
- [ ] All mutations produce audit log entries with before/after snapshots
- [ ] `cargo test --workspace` passes

###### Evidence
- **Test command:** `cargo test --workspace`
- **Evidence focus:** Unit tests for similarity threshold, LLM response parsing, usage scoring. Integration test: seed two similar skills → run merge detection → verify merged file + .retired markers. Seed stale skill → Run retirement pass → verify .retired marker. Verify audit log entries.

##### Slice 2.4: Outbox Relay Worker + Qdrant-PG Consistency
**Slice type:** hardening
**Serves:** SC-4 (merge/retire data integrity), SC-7 (graceful degrade — consistency under failure)
**Demo scenario:** Graph builder writes 50 skills during a full rebuild. PG transaction commits with outbox entries. Async relay worker polls outbox, writes to Qdrant, marks processed. Mid-write, Qdrant container restarts. Worker retries with exponential backoff. After Qdrant recovers, worker picks up pending entries and completes. Reconciliation job runs daily, finds zero orphans. Developer queries skills — all have correct embeddings.
**Feature home:** `crates/infrastructure/` (relay worker), `crates/graph-builder/` (reconciliation job)
**Files:**
- `crates/infrastructure/src/persistence/outbox.rs` — add relay worker loop (poll `status='pending'`, write to Qdrant, mark processed)
- `crates/infrastructure/src/persistence/outbox_reconciler.rs` — reconciliation job (daily scan for consistency gaps)
- `tests/integration/test_outbox_consistency.rs` — simulated partial failure scenarios
**Depends on:** Slice 2.1 (outbox table + GraphWriteCoordinator exist)
**Dependency type:** real
**Blast radius:** low (offline, no user-visible impact during development)

###### What to build
Async relay worker in `infrastructure`: polls `outbox` table (`WHERE status='pending'`) every 1s, claims up to 10 entries with `FOR UPDATE SKIP LOCKED`, writes to Qdrant (upsert with content-hash point ID for idempotency), marks `status='processed'`. On Qdrant failure: increment `retry_count`, set `status='failed'` + `last_error`. Worker emits `graph.rebuilt` only after ALL pending outbox entries for current rebuild are processed (prevents stale cache issue).

Reconciliation job: runs daily (cron in `maintenance`). Scans `skills WHERE last_vector_sync_at < updated_at` → re-enqueues as outbox entries. Scans Qdrant collection against known content hashes → deletes orphaned points.

###### Scope
- **Owns:** Outbox relay polling worker, reconciliation job, Qdrant idempotent upsert, consistency gap detection
- **Non-goals:** Distributed transactions, real-time sync guarantee (eventual consistency is acceptable for offline path)
- **Scope fence:** Outbox is for Qdrant consistency only. PG-PG writes are already transactional.

###### Acceptance criteria
- [ ] Relay worker polls outbox and processes pending entries
- [ ] Qdrant write failure → entry stays in `failed` state with incremented retry_count
- [ ] Qdrant write success → entry marked `processed`
- [ ] Idempotent upsert: same content_hash → same Qdrant point ID, no duplicates
- [ ] Reconciliation job detects skills with stale/missing Qdrant vectors
- [ ] `graph.rebuilt` event emitted only after all outbox entries for current rebuild processed
- [ ] `cargo test --workspace` passes

###### Evidence
- **Test command:** `cargo test --workspace`
- **Evidence focus:** Unit tests for outbox polling, retry logic, idempotent upsert. Integration test: write to PG+outbox → relay worker processes → verify Qdrant has point. Simulate Qdrant failure → verify retry + eventual consistency. Reconciliation scan test.

##### Slice 2.5: .pending File Lifecycle State Machine
**Slice type:** hardening
**Serves:** SC-3 (human approval with proper lifecycle), SC-5 (filesystem-observable state machine), constitution §3 (human gate)
**Demo scenario:** Developer has 15 `.pending` files from sessions over 2 months. Some are 45 days old. Cleanup cron runs — logs warnings for files >30 days old: "WARNING: setup-rust-project.md.pending has been pending for 45 days". No files deleted. Developer reviews warnings, approves some (rename to `.md`), rejects others (delete, optionally create `.rejected` tombstone).
**Feature home:** `crates/graph-builder/src/maintenance/`
**Files:**
- `crates/graph-builder/src/maintenance/cleanup.rs` — TTL warning scan + .rejected tombstone pruning
- `crates/mcp-server/src/session_extractor/writer.rs` — add `expires_at`, `warning_at`, `origin`, `source_session_id` to .pending frontmatter
- `crates/domain/src/types.rs` — add SkillLifecycleState enum: Draft, Active, Retired, Rejected, Deleted
- `tests/integration/test_pending_lifecycle.rs` — .pending → .md, .pending → .rejected, TTL warning
**Depends on:** Slice 2.2 (session extraction produces .pending), Slice 2.3 (maintenance produces merge .pending)
**Dependency type:** real

###### What to build
Complete filesystem-based lifecycle state machine with 5 states:
- **draft** (`.pending`): Proposed skill. YAML frontmatter includes `origin`, `created_at`, `expires_at` (90d), `warning_at` (30d), `source_session_id`/`merged_from`. TTL tracked by cleanup cron.
- **active** (`.md`): Approved skill. Rename from `.pending` is the approval action per constitution §3.
- **retired** (`.retired`): Deprecated skill. Human leaves marker in place to confirm retirement proposal.
- **rejected** (`.rejected` or deleted): Rejected proposal. `.rejected` tombstone prevents re-proposal. `is_tombstone: true` frontmatter for pure markers.
- **deleted**: Removed from filesystem. No observable state.

Cleanup cron (runs with maintenance cron): scans all `.pending` files, logs warning if `now > warning_at` AND not yet warned. Sets `warning_logged_at` in frontmatter. NO auto-deletion — constitution §3 requires human action for all mutations. `.rejected` tombstones with `is_tombstone: true` pruned after 30 days (tombstones are observation records, not mutations).

YAML frontmatter schema for `.pending`:
```yaml
origin: session_extraction | merge_proposal | manual
created_at: RFC3339
expires_at: RFC3339 (created + 90d)
warning_at: RFC3339 (created + 30d)
source_session_id: string (if origin=session_extraction)
merged_from: [path, ...] (if origin=merge_proposal)
warning_logged_at: RFC3339 (set on first warning)
modification_count: int (incremented on user edits)
```

###### Scope
- **Owns:** Lifecycle state machine, .pending YAML frontmatter, TTL warning scan, .rejected tombstone
- **Non-goals:** Auto-deletion (constitution violation), notification service (V2), user-facing cleanup UI
- **Scope fence:** No auto-delete. No auto-approve. No auto-retire. All mutations require human filesystem action

###### Acceptance criteria
- [ ] .pending files include `origin`, `created_at`, `expires_at`, `warning_at` in YAML frontmatter
- [ ] Cleanup cron logs warning for .pending files >30 days old (no deletion)
- [ ] Rejected extractions produce `.rejected` tombstone with `is_tombstone: true`
- [ ] `.rejected` tombstones >30 days old are pruned (tombstone cleanup, not skill deletion)
- [ ] `skill.file_changed` event with `source: "pending_approval"` on `.pending` → `.md` rename
- [ ] `skill.file_changed` event with `change_type: "retired"` on `.md` → `.retired` rename
- [ ] All state transitions produce audit_log entries
- [ ] `cargo test --workspace` passes

###### Evidence
- **Test command:** `cargo test --workspace`
- **Evidence focus:** Unit tests for frontmatter parsing, state transition detection, TTL calculation. Integration test: create .pending → 31 days → verify warning log. Rename .pending → .md → verify graph update. Delete .pending → verify .rejected tombstone.

#### Phase 3: Hardening — Resilience, Observability, Documentation
**Purpose:** Ensure production readiness. Infrastructure resilience, structured logging, developer documentation, and constitution compliance verification.
**Rationale:** Phase 1-2 prove the feature works. Phase 3 proves it works reliably and can be operated and contributed to.

##### Slice 3.1: Graceful Degrade, Retry, Health Checks
**Slice type:** hardening
**Serves:** SC-7 (graceful degrade on any infra failure)
**Demo scenario:** Developer starts Docker Compose but Ollama container fails to pull the model. MCP server starts anyway. When `compile_context` is called, it logs the outage and returns `{status: "degraded", reason_code: "embedding_provider_unavailable"}`. No crash. graph-builder retries failed operations with exponential backoff (1s, 2s, 4s, 8s, max 60s). Redis connection loss → reconnect with backoff. PG connection loss → pool retry.
**Feature home:** `crates/infrastructure/`, `crates/mcp-server/`, `crates/graph-builder/`, `crates/session-extractor/`
**Files:**
- `crates/infrastructure/src/resilience.rs` — retry with backoff, circuit breaker pattern
- `crates/infrastructure/src/health.rs` — health check endpoint
- `crates/mcp-server/src/main.rs` — add health check endpoint, wrap compile_context in degrade guard
- `crates/graph-builder/src/main.rs` — wrap all ops in retry guards
- `crates/session-extractor/src/lib.rs` — retry on Claude Code invocation failure
- `docker-compose.yml` — add healthcheck definitions for all containers
- `tests/integration/test_resilience.rs` — forced failure scenarios
**Depends on:** Slice 2.3
**Dependency type:** real
**Blast radius:** low (adds resilience, no functional changes)
**Shared state changes:** None
**Rollback path:** No state changes to roll back

###### What to build
Resilience utilities in infrastructure: retry with exponential backoff config, circuit breaker (open after N consecutive failures, half-open after timeout). Health check endpoint on each service (`/health` returning JSON with component status: ollama, qdrant, pg, redis). MCP server wraps `compile_context` in degrade guard: any infra failure → log warning → return explicit degraded result, not a healthy-looking empty success. graph-builder wraps all Ollama/Qdrant/PG writes in retry. session-extractor retries headless Claude Code invocation on failure.

Docker Compose healthcheck definitions with appropriate intervals and retries. Dependency ordering: PG starts before services, Redis starts before services, Ollama + Qdrant start in parallel (services start without them if needed).

###### Scope
- **Owns:** Retry with backoff, circuit breaker, health checks, degrade guards, Docker healthchecks
- **Non-goals:** Metrics/monitoring dashboard (V2), alerting (V2), performance profiling
- **Scope fence:** Do not add Prometheus/Grafana. Health check is JSON endpoint only

###### Acceptance criteria
- [ ] MCP server returns `degraded` when Ollama is unreachable (doesn't crash)
- [ ] MCP server returns `degraded` when Qdrant is unreachable
- [ ] MCP server returns `degraded` when PG is unreachable
- [ ] Graph builder retries with exponential backoff on transient failures
- [ ] Circuit breaker opens after N consecutive failures and resets after timeout
- [ ] All services expose `/health` endpoint returning JSON status per dependency
- [ ] Docker Compose health checks report accurate container status
- [ ] Services start in correct order (PG → Redis → Ollama/Qdrant → Rust services)
- [ ] `cargo test --workspace` passes (including forced-failure resilience tests)

###### Evidence
- **Test command:** `cargo test --workspace`
- **Evidence focus:** Unit tests for retry backoff, circuit breaker state machine. Integration test: stop Ollama container → call compile_context → verify `degraded` returned. Restart Ollama → verify normal context returned. Verify health endpoint reports degraded status during outage. Verify circuit breaker opens/closes correctly.

##### Slice 3.3: Session State Persistence + Compiled Context Cache
**Slice type:** hardening
**Serves:** SC-1 (zero-touch — no duplicate injection on restart), SC-7 (graceful degrade — state survives infra failure)
**Demo scenario:** Developer has been working in a Claude Code session for 30 minutes. Docker Compose needs a restart. `docker compose restart`. Next prompt — MCP server reads session state from Redis and returns `duplicate_suppressed` instead of re-injecting context. Developer doesn't notice anything.
**Feature home:** `crates/mcp-server/src/state.rs`
**Files:**
- `crates/mcp-server/src/state.rs` — dual-tier: DashMap (hot path, <100ns) + Redis SETEX write-through (24h TTL, crash recovery)
- `crates/mcp-server/src/state.rs` — add compiled context cache: key `(prompt_hash, scope_fingerprint, graph_version)`, TTL 5min
- `tests/integration/test_session_persistence.rs` — restart MCP server → verify no duplicate injection
**Depends on:** Slice 3.1 (resilience + health checks already in place)
**Dependency type:** real

###### What to build
Dual-tier session state: on healthy `compile_context` outcome per `{session_id, repo_path}`, set suppression state in DashMap AND Redis SETEX (write-through, 24h TTL with key `session:{session_id}:{repo_path}`). On MCP server startup, pre-load all `session:*` keys from Redis into DashMap to avoid Redis round-trip per prompt. On `SessionEnd` hook: explicitly `DEL session:{session_id}:{repo_path}` (not TTL expiry). Redis unreachable → fall back to DashMap-only (documented behavior: survives process lifetime only). `degraded` outcomes must not write suppression state.

Compiled context cache: on `compile_context` success, cache result with key `(blake3(prompt), scope_fingerprint, graph_version)`. On `graph.rebuilt` event: increment `graph_version` (Redis INCR) → cache auto-invalidated by version mismatch. TTL 5min for same prompt + same graph state. Skips full pipeline (embedding, Qdrant, PG, MMR, RRF, template) on cache hit.

###### Scope
- **Owns:** Dual-tier session state, compiled context cache, graph_version counter
- **Non-goals:** Full-blown caching layer, cache eviction policies beyond TTL
- **Scope fence:** Session state is operational caching, not a graph mutation. Redis is write-through, not source of truth

###### Acceptance criteria
- [ ] MCP server restart → session state loaded from Redis → no duplicate context injection
- [ ] Healthy first outcome persists suppression state; degraded first outcome does not
- [ ] Same prompt + same graph version → compiled context returned from cache (verified: no Ollama/Qdrant/PG calls)
- [ ] `graph.rebuilt` event → cache invalidated by graph_version mismatch
- [ ] Redis unavailable → falls back to DashMap-only (documented behavior)
- [ ] `SessionEnd` hook → session state explicitly deleted from Redis
- [ ] `cargo test --workspace` passes

###### Evidence
- **Test command:** `cargo test --workspace`
- **Evidence focus:** Unit tests for dual-tier state read/write, cache key computation, graph_version invalidation, and suppression-on-healthy-only behavior. Integration test: compile context → restart MCP server container → compile context again → verify `duplicate_suppressed`. Stop Redis → verify DashMap fallback. Publish `graph.rebuilt` → verify cache miss.

##### Slice 3.2: Structured Logging, Benchmarking, Documentation
**Slice type:** hardening
**Serves:** All success criteria (observability for debugging), SC-1 (latency benchmark verification)
**Demo scenario:** Developer runs `docker compose up`. All services log structured JSON to stdout (Docker logs). `docker compose logs mcp-server | grep compile_context` shows: `{"ts":"...","level":"INFO","msg":"compile_context","session_id":"abc","repo_path":"/home/rabak/projects/foo","latency_ms":342,"skills_retrieved":7,"scopes":["project","global"]}`. Benchmark test verifies <500ms p50 latency under load. README walks a new developer through setup in 5 minutes.
**Feature home:** `crates/infrastructure/`, `docs/`
**Files:**
- `crates/infrastructure/src/logging.rs` — structured JSON logger setup (tracing + tracing-subscriber)
- `crates/mcp-server/src/main.rs` — initialize structured logging
- `crates/graph-builder/src/main.rs` — initialize structured logging
- `crates/session-extractor/src/lib.rs` — initialize structured logging
- `tests/bench/compile_context_bench.rs` — criterion benchmark for compile_context latency
- `README.md` — project overview, 10-minute quickstart, architecture diagram
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md` — detailed architecture doc
- `docs/reference/capability-catalog.md` — tool surface, lifecycle states, event catalog, scope model
- `docs/runbooks/degraded-state.md` — degraded-mode meanings, reason codes, operator actions
- `docs/reference/transcript-ingress.md` — transcript root mount contract and hook examples
- `CONTRIBUTING.md` — dev setup, testing, conventions
**Depends on:** Slice 3.1
**Dependency type:** real
**Blast radius:** none (docs + logging only)
**Shared state changes:** None
**Rollback path:** No state changes

###### What to build
Structured JSON logging via `tracing` + `tracing-subscriber` with JSON formatter. All services log: startup/shutdown, MCP tool invocations (with latency), graph rebuild events (duration, counts), extraction requests (session_id, candidate count), errors (with context, no stack traces in production). Log level configurable via `RUST_LOG` env var.

Criterion benchmarks: `compile_context` latency under varying load (1 concurrent, 5 concurrent, 10 concurrent). Must prove p50 <500ms, p95 <800ms.

Documentation: README with project overview, 10-minute quickstart (`docker compose up` + `.claude/settings.json` snippet), architecture diagram (ASCII or Mermaid), and transcript-mount setup. Architecture doc in `docs/architecture/` with data flow diagrams, component descriptions, configuration reference. Capability catalog documents tools, events, lifecycle states, and result semantics. Degraded-state runbook documents reason codes and operator response. CONTRIBUTING.md covers dev setup, test conventions, and code style.

###### Scope
- **Owns:** Structured JSON logging, criterion benchmarks, README, architecture doc, contributing guide
- **Non-goals:** Metric dashboards, alert rules, SLO definitions, performance tuning beyond <500ms target
- **Scope fence:** Do not add OpenTelemetry. Do not add Grafana dashboards. Logging is stdout only (Docker handles aggregation)

###### Acceptance criteria
- [ ] All services log structured JSON to stdout (verify: `docker compose logs mcp-server | jq .`)
- [ ] compile_context invocation logged with latency_ms, skills_retrieved, scopes
- [ ] Graph rebuild logged with duration_ms, skills_count, communities_count
- [ ] Errors logged with context (no silent failures anywhere)
- [ ] `RUST_LOG` env var controls log level
- [ ] Criterion benchmark: `compile_context` p50 <500ms, p95 <800ms
- [ ] README enables a new developer to `docker compose up` and see context in Claude Code within 10 minutes
- [ ] Capability catalog documents tool surface, event catalog, lifecycle states, scope model, and result semantics
- [ ] Degraded-state runbook documents `reason_code` values and operational responses
- [ ] Architecture doc covers: container topology, data flow, retrieval pipeline, compilation format, event streams, invalidation contract
- [ ] CONTRIBUTING.md covers: dev setup, `cargo test --workspace`, `cargo clippy`, benchmark commands, PR process
- [ ] `cargo test --workspace` passes
- [ ] `cargo bench` runs and produces latency numbers

###### Evidence
- **Test command:** `cargo test --workspace && cargo bench`
- **Evidence focus:** Integration test verifies JSON log output format. Benchmark verifies latency targets. Manual: README quickstart walkthrough.

### Slice-to-Story Traceability

| Success Criterion | Delivered by Slice(s) | Demo scenarios |
|---|---|---|---|
| SC-1: Zero-touch context injection | Slice 1.2, 1.3, 3.3 | Developer types task → context appears in Claude Code. Restart → `duplicate_suppressed` instead of reinjection |
| SC-2: Dual-scope retrieval + fusion | Slice 1.3 | Project + global skills searched concurrently, RRF merged |
| SC-3: Session-end extraction + approval | Slice 2.2, 2.5 | Session ends → .pending file appears → rename approves. Stale .pending → warning |
| SC-4: Offline maintenance (merge/retire) | Slice 2.3, 2.4 | Cron detects duplicates, proposes merge; detects stale, proposes retire. Outbox ensures consistent vectors |
| SC-5: Filesystem-observable state | Slice 2.1, 2.2, 2.3, 2.5 | .pending, .retired, .rejected, SKILL.md — all mutations visible as files. Full lifecycle state machine |
| SC-6: Subunit-aware compilation | Slice 1.2 | Context includes subunit highlights + rescue-attached evidence |
| SC-7: Graceful degrade | Slice 3.1, 3.3, 2.4 | Ollama down → MCP server returns `degraded`. State survives restart. Outbox ensures eventual consistency |
| SC-8: V2 readiness | Slice 1.1a, 1.1b | PG schema from day one. domain/infrastructure split. ScopeResolver trait. remote config additive |

## Alternative Approaches Considered

**Approach A: Monolithic Rust + SQLite** — Rejected because SQLite creates V2 migration debt for team sharing. Monolith harder to scale components independently (graph building is memory-intensive, MCP server must be latency-optimized). Does not serve SC-8 (V2 readiness).

**Approach C: Qdrant-only (no relational DB)** — Rejected because Qdrant payload-based graph queries are inadequate for multi-hop traversal, community membership queries, and transactional merge/retire operations. The SkillRAE paper inherently needs both vector search AND graph structure. Does not serve SC-4 (reliable merge/retire).

**Approach D: Python-based services** — Rejected because Python adds 10-50x latency overhead vs Rust for the sync MCP server, breaking the <500ms target (SC-1). Constitution principle "local-first" favors Rust's zero-dependency deployment. Python would require virtualenv management in Docker.

## Acceptance Criteria

### Functional Requirements

- [ ] FR-1: Developer receives auto-compiled skill context in Claude Code session on first prompt without any manual action
- [ ] FR-2: Context includes skills from both project scope (git root) and global scope (harness skill dirs), merged by relevance
- [ ] FR-3: Context includes subunit highlights (procedures, conventions) and rescue-attached evidence from related skills
- [ ] FR-4: Session-end extraction produces `.pending` draft SKILL.md files from transcript analysis
- [ ] FR-5: `.pending` → `.md` rename triggers automatic graph indexing
- [ ] FR-6: Offline maintenance detects near-duplicate skills and proposes merges
- [ ] FR-7: Offline maintenance detects stale skills and proposes retirement via `.retired` markers
- [ ] FR-8: Graph rebuild updates Qdrant embeddings and PG graph structure incrementally on file change
- [ ] FR-9: Community detection (HDBSCAN + tag augmentation) runs on full rebuild, supporting dual skill membership

### Non-Functional Requirements

- [ ] NFR-1: `compile_context` completes in <500ms p50 (measured by benchmark)
- [ ] NFR-2: MCP server never crashes on infrastructure failure (returns explicit `degraded` or healthy `no_match`, stays alive)
- [ ] NFR-3: All services log structured JSON to stdout
- [ ] NFR-4: All containers start via `docker compose up` without manual setup beyond env vars
- [ ] NFR-5: PG migrations run automatically on first start

### Quality Gates

- [ ] QG-1: `cargo test --workspace` passes with Red→Green→Post-Refactor-Green evidence per slice
- [ ] QG-2: `cargo clippy --workspace -- -D warnings` passes
- [ ] QG-3: `cargo fmt --check --all` passes
- [ ] QG-4: `cargo bench` passes with latency targets met
- [ ] QG-5: Docker Compose integration tests pass (`docker compose -f docker-compose.test.yml up --abort-on-container-exit`)
- [ ] QG-6: Architecture doc written and reviewed

## Success Metrics

- **Context relevance:** Measured by developer feedback — do the skills surfaced actually help with the task? Tracked informally in V1.
- **Latency:** `compile_context` p50 <500ms, p95 <800ms (benchmark-verified)
- **Graph growth rate:** Number of skills in graph increases over time (session-end extraction adds skills)
- **Approval rate:** Percentage of `.pending` extractions that are approved (signals extraction quality)
- **Graph health:** Number of merges and retirements per month (signals maintenance effectiveness)

## Dependencies & Prerequisites

- Docker and Docker Compose installed on developer machine
- Rust toolchain (rustup, cargo, clippy, rustfmt) for development
- Claude Code installed (V1 harness target)
- NVIDIA GPU recommended for Ollama embedding speed; CPU fallback available (slower)
- ~10GB disk for Docker images + Ollama model (nomic-embed-text ~274MB)
- No external network dependencies after initial Docker pull

## Risk Analysis & Mitigation

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|---|---|
| Ollama embedding latency exceeds budget | Medium | High | GPU passthrough. LRU embedding cache + Ollama semaphore with 1 reserved sync slot. Compiled context cache (5min TTL). Cold-start short-circuit |
| HDBSCAN not available in pure Rust | Low | Medium | Pure Rust `hdbscan` crate v0.12. `features = ["parallel"]`. Fallback: linfa-clustering DBSCAN |
| Claude Code hook format changes | Low | Medium | MCP protocol is a standard. Pin Claude Code version in docs |
| Too many skills → retrieval slows down | Medium | Medium | Relevance-threshold pruning. MMR hard cap 50 per scope. CTE LIMIT 50 per hop + statement_timeout 400ms |
| Ollama semaphore priority inversion | Medium | High | Reserved 1 semaphore slot for sync `compile_context` path. Offline batch gets 3 slots |
| PostgreSQL schema migration conflicts | Low | Medium | TEXT CHECK constraints (not ENUM). `scope` scalar with `merged_from_scopes TEXT[]` for V1 |
| Redis OOM on unbounded streams | Low | Medium | `allkeys-lru` maxmemory-policy. `MAXLEN ~100_000`. Periodic background XTRIM |
| Embedding cache miss under high load | Medium | High | LRU cache (capacity 1000). Compiled context cache (5min TTL). Graph version check for cache hit |
| PG recursive CTE performance at scale | Low | Medium | LIMIT 50 per hop + relevance pruning. Composite junction indexes both directions. Benchmark at 1K/5K/10K |
| Qdrant-PG dual-write inconsistency | Medium | High | Outbox pattern: PG outbox table → async relay worker → Qdrant. Content-hash idempotency. Reconciliation job daily |
| Filesystem watcher missed events | Medium | Medium | notify-debouncer-full + FileIdMap. PollWatcher fallback for WSL/Docker/NFS. Startup recovery scan. Idempotency key |
| Redis consumer crash → lost event | Low | Medium | Consumer groups + XAUTOCLAIM every 5s. DLQ after 3 attempts. Startup claim of stale pending |
| .pending files accumulate indefinitely | High | Low | TTL warning at 30d (log only). No auto-delete (constitution §3). `.rejected` tombstone prefix on deletion |
| Session state lost on MCP restart | Medium | Medium | Dual-tier: DashMap (hot path) + Redis SETEX write-through (24h TTL). Startup pre-load from Redis |

## Resource Requirements

- **Development:** 1 developer (solo), estimated 4-6 weeks for all slices
- **Infrastructure:** Developer machine with Docker, Rust, 16GB+ RAM, GPU recommended
- **Dependencies:** Docker images (Qdrant, PostgreSQL, Redis, Ollama), Rust crates (tokio, sqlx, qdrant-client, redis-rs, rmcp, notify, tracing, linfa-clustering or HDBSCAN subprocess)

## Future Considerations

- **V2: Multi-harness support** — OpenCode, Copilot, Codex integration via same MCP protocol. Their hook systems differ but MCP tool surface is identical.
- **V2: Team scope** — Remote PG instance + shared Qdrant index. Add `team` scope value via additive schema migration and new resolver/config. No retrieval-pipeline rewrite needed.
- **V2: LLM-generated task guidance** — Replace template-only compilation with Ollama-synthesized guidance. Trade latency for quality. Deferred because template-based is sufficient for V1.
- **V2: Skill versioning** — Track skill versions through content hash diffs. Allow rollback to previous skill version.
- **V2: Feedback loop** — Developer rates skill relevance post-session. Feedback improves retrieval scoring weights.
- **V2: Skill dependency graph** — Track that skill A's procedure references skill B's convention. Use for retrieval boosting (if A is relevant, B might be too).

## Documentation Plan

- `README.md` — Project overview, 10-minute quickstart, architecture diagram, transcript mount setup
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md` — Container topology, data flow, retrieval pipeline, compilation format, event streams, PG schema ERD, invalidation contract
- `docs/reference/capability-catalog.md` — Tool surface, result semantics, lifecycle states, event catalog
- `docs/runbooks/degraded-state.md` — Reason codes, degraded meanings, operator actions
- `docs/reference/transcript-ingress.md` — Mounted transcript-root contract and hook examples
- `CONTRIBUTING.md` — Dev setup, `cargo test`, `cargo clippy`, `cargo bench`, PR conventions
- `config/claude-code/hooks.example.json` — Example `.claude/settings.json` with MCP hook configuration
- Inline code documentation: Rust doc comments on all public APIs

## WHY Reassessment

This V1.1 file is the canonicalized follow-up to the original plan. The assessment findings below are incorporated here rather than left as pending contradictions.

### Constitution Violations — Resolved

**BLOCKING-1: Headless Claude Code extraction — RESOLVED.** Headless Claude Code remains default. Ollama stays optional behind `TranscriptSkillExtractionService`. Config field `provider` (claude|ollama) routes to the concrete implementation.

**BLOCKING-2/3: Merge bypassed human gate — RESOLVED.** All edits and merges go through `.pending` state. Merge detection generates `.pending` merged file. Human renames to `.md` to approve. Source skills marked `.retired` only after merge approval. This resolves both BLOCKING-2 and BLOCKING-3.

**P1-1: Retirement delegated to LLM — RESOLVED.** Retirement produces `.retired` marker as PROPOSAL. Human confirms by leaving `.retired` in place (remove to keep active). LLM evaluates but does not finalize.

**P1-2: Missing audit for Slice 2.1 — RESOLVED.** Audit AC added: "All graph mutations produce `audit_log` entries with before/after JSONB snapshots."

**P1-3: Transcript trust boundary — RESOLVED.** `extract_session` now accepts `transcript_ref` under a read-only mounted transcript root. Raw host paths are not part of the V1.1 contract. Optional `transcript_inline` is reserved for tests and future harnesses.

### Other Decisions Applied

- **Renamed crates/namespaces to drop `skill-` prefix** — `domain`, `infrastructure`, `mcp-server`, `retrieval`, `compiler`, `graph-builder`, `maintenance`, `admin`, `session-extractor`
- **Removed `approve_extraction`/`reject_extraction` from MCP surface** (filesystem IS approval UI per constitution §5)
- **Embedding service trait abstraction** added to plan: `EmbeddingService` trait + `OllamaEmbeddingService` concrete impl + config-driven provider routing
- **Extraction service trait abstraction** added to plan: `TranscriptSkillExtractionService` trait + `ClaudeExtractor` (default) + `OllamaExtractor` (optional) + config-driven routing
- **Architecture complexity:** Kept as designed (full multi-crate, dual-scope, full lifecycle pipeline). Scope was not reduced.
- **`compile_context` contract hardened** — `ok`, `no_match`, `degraded`, and `duplicate_suppressed` replace ambiguous empties
- **Scope persistence frozen** — scalar `scope` + `merged_from_scopes`; no V1 `skill_scopes` junction table

### Other Quality Issues (P2/P3)

**Per-stage latency SLOs missing (P1 performance).** No budgets assigned per pipeline stage. Cannot diagnose which stage regresses. Fix: add per-stage budgets per the latency research table.

**Embedding cache missing (P1 performance).** Every compile_context call embeds prompt via Ollama. No cache. 10x concurrent load = Ollama queue explosion, 1s+ latency. Fix: LRU cache (capacity 1000) + Ollama Semaphore(4).

**Cold-start short-circuit missing (P1 performance).** Full pipeline runs even with zero skills, wasting 20-150ms. Fix: `SELECT COUNT(*) FROM skills WHERE status = 'active'`. If zero, return healthy `no_match` immediately.

**Redis `processed` HashSet unbounded (P1 memory leak).** Consumer stores every message ID forever. Fix: Redis SETEX with TTL for idempotency, not in-memory HashSet.

**QP-Consistency gap (P2 architecture).** Graph builder writes to Qdrant + PG in separate calls. No distributed transaction. If Qdrant fails after PG commit, vectors orphan. Fix: outbox pattern in PG → relay publishes to Qdrant.

**Skill lifecycle missing states (P2 spec-flow).** No `.rejected` tombstone for rejected extractions. No `.merge_proposal` for merge candidates. No `.pending` TTL/expiration policy. No skill deletion flow (only retirement). Fix: define complete lifecycle state machine per spec-flow analysis.

**Session state volatile (P2).** `{session_id, repo_path}` compilation flag is in-memory. MCP server restart → lost → re-injects context mid-session. Fix: persist to Redis with 24h TTL.

## References & Research

### Internal References

- Brainstorm: `docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md` (28 resolved decisions)
- Constitution: `docs/constitution.md` v1.0.0 (5 principles, 8 approval boundaries)
- Glossary: `CONTEXT.md` (Skill, Skill Graph, Subunit, Skill Community, Tag, Scope)

### External References

- SkillRAE paper: arXiv:2605.10114 (Meng, Wang, Fang — CUHK Shenzhen, May 2026)
- nomic-embed-text: Ollama model, 768-dim embeddings, code-aware
- MCP protocol: Model Context Protocol specification (Anthropic)
- Claude Code hooks: 26 lifecycle events, `type: "mcp_tool"` integration
- HDBSCAN: Campello et al., density-based hierarchical clustering

### Related Work

- SkillsBench: execution-centric agent skill benchmark with deterministic verifiers
- AgentSkillOS: ecosystem-scale skill organization and orchestration
- SkillRouter: full-text skill routing for LLM agents
