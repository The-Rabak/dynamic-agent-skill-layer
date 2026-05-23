---
date: 2026-05-21
topic: skill-layer-v1-1
status: complete
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
reviewers:
  - architecture-strategist
  - uncle-bob
handoff:
  deepen_plan: true
  work: true
  review: true
---

# Dynamic Agent Skill Layer V1.1 — Canonical Architecture

## Purpose Linkage

- **Problem Narrative:** Developer using multiple coding agent harnesses faces triple compound cost: manual skill selection wastes 5-10 min per task, skill libraries rot unused, each harness operates in a silo. Skills built in one never transfer to another. Existing approaches force one scope or manual curation of both.
- **User Story:** Solo developer needs zero-touch, self-growing skill context layer that searches project-local and global machine-wide skill scopes concurrently, merges via weighted RRF+MMR, compiles at session start, auto-extracts at session end, and offline deduplicates/merges/retires.
- **Success Criteria this artifact protects:**
  - SC-1: Zero-touch context injection <500ms
  - SC-2: Dual-scope concurrent retrieval with MMR-then-RRF
  - SC-3: Session-end skill extraction with .pending human approval
  - SC-4: Offline graph maintenance (merge, retire, cron)
  - SC-5: Filesystem-observable state
  - SC-6: Subunit-aware compilation
  - SC-7: Graceful degrade on any infrastructure failure
  - SC-8: V2 readiness (team scope with additive migration, not architectural rewrite)
- **Architectural Context:** Nine Rust crates with explicit feature homes, Docker Compose with 5 infrastructure containers (Qdrant, PostgreSQL, Redis, Ollama, + service binaries). MCP protocol is harness boundary. Redis Streams is internal event bus. PG is shared integration point. Filesystem is human-approval UI.

## Feature Homes and Ownership

### Post-improvement crate structure

```
crates/
├── domain/           # Pure domain: types, traits, config (ZERO infra deps)
├── infrastructure/   # Concrete impls: Ollama clients, PG pool, Redis, resilience
├── mcp-server/       # Thin MCP transport: bootstrap, tool handlers, session state
├── retrieval/        # Retrieval pipeline: Qdrant search, PG graph, scoring, MMR+RRF
├── compiler/         # Context compilation: template, rescue, formatting
├── graph-builder/    # Offline graph construction: watcher, extraction, embeddings, HDBSCAN
├── maintenance/      # Policy workflows: merge detection, retirement, cron trigger
├── admin/            # Online admin/debug MCP tools (rebuild_graph, inspect_skill, list_communities)
├── session-extractor/ # Post-session: transcript analysis, skill extraction, .pending files
```

### Feature-home ownership

- **Feature home: `crates/domain/`**
  - Owns: `Skill`, `Subunit`, `Community`, `Scope`, `SkillStatus`, `SubunitType` types; `EmbeddingService` trait; `TranscriptSkillExtractionService` trait; `ScopeResolver` trait; `ContextCompiler` trait; configuration structs; shared domain errors
  - Crosses into: nothing — zero infrastructure dependencies
  - Notes: This is the inner-most layer. All other crates depend on it. It depends on nothing. Validated by `cargo tree -p domain --depth 1` showing no sqlx/qdrant-client/redis deps.

- **Feature home: `crates/infrastructure/`**
  - Owns: `OllamaEmbeddingService` (implements `EmbeddingService`); `ClaudeExtractor` + `OllamaExtractor` (implement `TranscriptSkillExtractionService`); PG connection pool + migration runner; Redis Streams publisher/subscriber; scope resolver implementations (`GitRootProjectResolver`, `EnvPathGlobalResolver`); resilience utilities (retry, circuit breaker, health checks); structured logging setup
  - Crosses into: `domain` (depends on traits and types)
  - Notes: All concrete external-system adapters live here. Service crates never instantiate `reqwest` or `sqlx::PgPool` directly — they get them from `infrastructure`.

- **Feature home: `crates/retrieval/`**
  - Owns: Qdrant vector search (per-scope); PG recursive CTE graph traversal; combined scoring per SkillRAE paper eq.3; MMR per-scope deduplication; RRF cross-scope fusion; dual-scope concurrent orchestrator; relevance-threshold filtering; result ranking
  - Crosses into: `domain` (types, traits), `infrastructure` (concrete clients)
  - Notes: Pure retrieval logic. No MCP transport concerns. No compilation. No session tracking. Testable in isolation with mock Qdrant/PG.

- **Feature home: `crates/compiler/`**
  - Owns: Structured markdown template compilation; rescue-aware subunit attachment; context formatting for Claude Code `additionalContext`
  - Crosses into: `domain` (types)
  - Notes: Takes scored/ranked skills → returns formatted string. Single responsibility. Different harnesses add new compilers, not new retrieval logic.

- **Feature home: `crates/mcp-server/`**
  - Owns: MCP server bootstrap (rmcp); tool handler registration (`compile_context`, `find_skill`, `extract_session`); session-scoped state tracking (keyed by `{session_id, repo_path}`); healthy-result suppression state
  - Crosses into: `domain`, `infrastructure`, `retrieval`, `compiler`, `session-extractor`
  - Notes: Thin transport adapter. Tool handlers are delegation-only — they compose retrieval + compilation, not implement them. This crate is the "orchestration" layer, not the "logic" layer.

- **Feature home: `crates/graph-builder/`**
  - Owns: Filesystem watcher (notify — detects new/modified/deleted SKILL.md and `.pending`→`.md` renames); subunit extraction (deterministic structural rules + Ollama JSON fallback); batch embedding generation; HDBSCAN community detection; incremental + full graph rebuild; `skill.file_changed` and `graph.rebuilt`/`graph.rebuild_failed` event publishing
  - Crosses into: `domain`, `infrastructure`
  - Notes: Offline construction only. No merge/retire policy. No admin tools. No online concerns. When a file changes, this crate rebuilds the graph — nothing more.

- **Feature home: `crates/maintenance/`**
  - Owns: Cross-scope skill deduplication (cosine similarity + LLM semantic check); `.pending` merged-file generation; recency-weighted usage scoring from `skill_usage` table; `.retired` proposal generation; cron trigger for periodic full maintenance pass
  - Crosses into: `domain`, `infrastructure`
  - Notes: Policy workflows. Consumes graph state, produces filesystem proposals. Does not build the graph. Does not serve online requests. Human gate enforced: merges produce `.pending`, not `.md`.

- **Feature home: `crates/admin/`**
  - Owns: Admin MCP tools: `rebuild_graph`, `inspect_skill`, `list_communities`, `get_pending_extractions`
  - Crosses into: `domain`, `infrastructure`, `graph-builder`
  - Notes: Read-only + trigger-only. Online debug surface. No mutation authority beyond triggering rebuilds.

- **Feature home: `crates/session-extractor/`**
  - Owns: `extract_session` tool handler (returns immediately, background task does work); JSONL transcript parsing; extraction provider routing (config `provider` field → Claude or Ollama); `.pending` SKILL.md file generation with YAML frontmatter; `skill.extraction_requested` and `extraction.completed` event publishing
  - Crosses into: `domain`, `infrastructure`
  - Notes: Name now reflects the action (extract) and the input (session transcript). The crate remains distinct even when its router is composed into the online binary.

## Shared / Global Decisions

| Candidate | Keep in feature home / Move to shared | Why |
|-----------|----------------------------------------|-----|
| `EmbeddingService` trait | `domain` (shared) | Used by retrieval, graph-builder, and session-extractor. Stable interface — input text, output embedding vector. Trait-only in domain; concrete impls in `infrastructure` |
| `TranscriptSkillExtractionService` trait | `domain` (shared) | Used by session-extractor and (future) graph-builder for Ollama subunit fallback. Renamed from `ExtractionService` to avoid collision with subunit extraction in graph-builder |
| `ScopeResolver` trait | `domain` (shared) | V2 adds team scope. Trait avoids hardcoded scope logic in retrieval and graph-builder. `GitRootProjectResolver` + `EnvPathGlobalResolver` in `infrastructure`. V2 adds `RemoteTeamScopeResolver` |
| `ContextCompiler` trait | `domain` (shared) | V2 adds LLM-synthesized guidance compiler. V1: `TemplateOnlyCompiler` in `compiler`. V2: `OllamaGuidanceCompiler` alongside. MCP server selects compiler by config |
| PG connection pool + migration runner | `infrastructure` (shared) | Every service crate needs PG. Single setup, shared pool config. Per-service pool instances, not a shared pool |
| Redis Streams pub/sub | `infrastructure` (shared) | Standard event envelope. Every service publishes or subscribes. Idempotency key management here |
| Resilience utilities (retry, circuit breaker, health checks) | `infrastructure` (shared) | Used by every service. Generic retry with backoff, circuit breaker state machine, health check JSON endpoint |
| MCP protocol surface | `mcp-server` (feature home) + `admin` (admin surface) | MCP transport layer stays thin; admin tools get their own crate so they don't bloat the online server |
| Retrieval + scoring logic | `retrieval` (feature home) | Pure pipeline. No MCP deps. No compilation. No session state. Testable with mock clients |
| Compilation logic | `compiler` (feature home) | Pure transformation: scored skills → markdown. No I/O. Testable with fixture skill arrays |
| Graph construction | `graph-builder` (feature home) | Offline. Watcher-driven. No policy workflows |
| Merge/retire policy | `maintenance` (feature home) | Policy workflows. Consumes graph, produces filesystem proposals. Separate reason to change from graph construction |
| Admin MCP tools | `admin` (feature home) | Online debug surface. Read-only + trigger-only. Separate binary or mounted in MCP server via router composition |
| Pending file generation | `session-extractor` (feature home) | Single concern: transcript → skill drafts. Name matches action |

## Canonical V1.1 Contracts

- **`compile_context` result contract:** `ok`, `no_match`, `degraded`, and `duplicate_suppressed` are the only legal top-level outcomes. Healthy no-match and degraded-empty are intentionally different states. Session suppression is written only after `ok` or `no_match`.

- **Transcript ingress contract:** `extract_session` accepts `transcript_ref` rooted under `CLAUDE_TRANSCRIPT_ROOT`, which is mounted read-only into the container. Optional `transcript_inline` exists for tests and future harnesses. Raw host `transcript_path` is not part of the V1.1 trust boundary.

- **State and invalidation contract:** `graph.rebuilt` is emitted only after PG writes commit, outbox relay drains, and the new `graph_version` is durable. `mcp-server` cache keys include `graph_version`, so invalidation happens by version mismatch rather than ad hoc cache clears.

- **Watcher reconciliation contract:** filesystem changes are observed by watcher first, but a reconciliation scan is mandatory on startup and periodically thereafter. Missed rename/delete transitions must emit idempotent `skill.file_changed` equivalents and corresponding audit records.

- **Scope persistence contract:** V1.1 keeps scalar `skills.scope` plus `merged_from_scopes TEXT[]`. This preserves V1 simplicity while keeping the retrieval/compiler architecture ready for a V2 additive migration if many-to-many scope membership becomes real.

- **Event catalog contract:** the canonical event set is `skill.file_changed`, `skill.extraction_requested`, `extraction.completed`, `extraction.failed`, `graph.rebuilt`, `graph.rebuild_failed`, `skill.retired`, and `skill.merged`. There is no `skill.approved` event in V1.1.

## Deletion Test

| Candidate | Keep/Delete/Delay | Why |
|-----------|-------------------|-----|
| `skill_scopes` junction table (many-to-many scope membership) | **Delay — use scalar `scope` column** | V1.1 freezes scalar `scope` plus `merged_from_scopes TEXT[]`. Team scope can arrive via additive migration when a real many-to-many access pattern appears. Easier to add later than remove now |
| `retrieval` and `compiler` as separate crates | **Keep** | Retrieval scoring changes when tuning relevance. Template format changes when adding harnesses. Different failure modes, different test surfaces, different change frequencies. The split costs nothing now (greenfield) and prevents a god module |
| `maintenance` and `graph-builder` as separate crates | **Keep** | Graph construction is offline/file-driven. Merge/retire are policy workflows with different triggers and change cadence. If the user story didn't include offline maintenance, builder would still exist. Separate reasons to change |
| `admin` as separate crate | **Keep** | Admin tools are online/debug surface in a system where the primary online path is MCP transport. Mixing them into `graph-builder` (offline) or `mcp-server` (production path) is a boundary violation. Thin crate, justified by separation of concerns |
| `ScopeResolver` trait | **Keep** | V2 team scope is a stated success criterion (SC-8). Hardcoding scope resolution in retrieval and graph-builder means adding team scope touches N crates. Trait costs nothing now and prevents that churn. Two V1 impls: `GitRootProjectResolver`, `EnvPathGlobalResolver` |
| `ContextCompiler` trait | **Keep** | V2 adds LLM-synthesized guidance. Without a trait, adding V2 compilation means refactoring MCP server internals. With trait, it's a new implementation in `compiler` + config toggle. Keeps the MCP server thin |
| Redis `processed` HashSet (in-memory dedup) | **Delete — use Redis SETEX** | Unbounded memory growth. Every processed message ID stored forever. Fix: `SETEX` with TTL for idempotency, configurable TTL (default 24h). No in-memory tracking |
| LLM guidance generation at compile time | **Delete from V1** | Template-only compilation keeps MCP server under 500ms. LLM generation deferred to V2 behind `ContextCompiler` trait without requiring architectural changes |
| Cross-scope merge auto-approval | **Delete permanently** | Constitution §3 requires human gate for all mutations. Merges produce `.pending` files, human renames to `.md`. Never auto-approve |
| Retirement auto-execution | **Delete permanently** | Constitution §3 requires human gate. Retirement scoring + LLM review runs offline, but `.retired` marker is always a PROPOSAL. Human confirms by leaving in place |
| `infrastructure` crate | **Keep** | Both reviewers independently flagged the old `skill-core` boundary violation. Without this crate, concrete impls either leak into domain (breaking purity) or duplicate across service crates (DRY violation). Single shared home for adapters is the correct split |
| legacy `skill-core` idea from the original plan | **Delete — split into `domain` + `infrastructure`** | The original concept said "core MUST NOT depend on infrastructure crates" but also bundled `db.rs`, `embedding.rs`, `events.rs`, `resilience.rs`, `logging.rs`. Those files introduce sqlx/redis/reqwest deps. The stated purity rule and the planned file list contradicted each other. Split resolves this |

## Interfaces as Test Surfaces

- **Interface: `EmbeddingService` (in `domain`)**
  - Callers/tests rely on: `embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>` and `embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError>`
  - Must not leak: Ollama HTTP details, connection pooling, semaphore state, model name resolution
  - Evidence needed later: Unit tests with mock `EmbeddingService` that returns deterministic vectors. Integration tests with containerized Ollama for real embedding dimensions (768). Benchmark: cache hit vs cache miss latency

- **Interface: `TranscriptSkillExtractionService` (in `domain`)**
  - Callers/tests rely on: `extract(&self, transcript: &SessionTranscript) -> Result<ExtractionResult, ExtractionError>`
  - Must not leak: Claude Code CLI invocation details, Ollama prompt engineering, output format parsing
  - Evidence needed later: Mock extraction returning known `ExtractionResult`. Integration: real transcript → valid `.pending` file with correct YAML frontmatter

- **Interface: `ScopeResolver` (in `domain`)**
  - Callers/tests rely on: `resolve(&self) -> Result<Vec<ScopeDescriptor>, ScopeError>` returning `{scope_id, scope_type, paths, config}`
  - Must not leak: git binary invocation, filesystem traversal details, env var parsing
  - Evidence needed later: Mock resolver returning known paths. Integration: real git root detection, real env var path array parsing

- **Interface: `ContextCompiler` (in `domain`)**
  - Callers/tests rely on: `compile(&self, skills: &[ScoredSkill], prompt: &str) -> String`
  - Must not leak: Markdown formatting details, rescue algorithm internals, template structure
  - Evidence needed later: Fixture skills → known markdown output. Regression: adding new compiler impl doesn't change existing output

- **Interface: MCP tool surface (`compile_context`, `extract_session`)**
  - Callers/tests rely on: JSON-RPC request/response with schemars-generated schemas
  - Must not leak: Internal retrieval pipeline structure, Ollama availability, PG schema details
  - Evidence needed later: E2E: MCP client → `compile_context` → structured markdown. Graceful degrade: Ollama down → `degraded` with reason/health markers, not a fake healthy empty success

- **Interface: Redis event contracts**
  - Callers/tests rely on: `{event_id: UUIDv7, event_type: string, correlation_id: UUIDv7, idempotency_key: string, schema_version: u32, timestamp: RFC3339, payload: {...}}`
  - Must not leak: Redis connection details, consumer group internals, stream key naming
  - Evidence needed later: Schema validation tests per event type. End-to-end: file change → `skill.file_changed` published → graph builder consumes → `graph.rebuilt` published → MCP server invalidates cache

- **Interface: PG schema as contract**
  - Callers/tests rely on: Tables (`skills`, `subunits`, `communities`, `skill_subunits`, `community_skills`, `session_logs`, `skill_usage`, `audit_log`), CHECK constraints, composite PKs, indexes
  - Must not leak: Migration tooling details, connection pool config, recursive CTE implementation
  - Evidence needed later: Migration idempotency (run twice, no errors). Schema validation: CHECK constraints reject invalid data. Performance: recursive CTE with 1K/5K/10K skills

## Seams, Adapters, and Contracts

- **Seam: Embedding generation**
  - **Adapter:** `OllamaEmbeddingService` (in `infrastructure`, implements `EmbeddingService`)
  - **Contract:** Input = text or batch of texts. Output = `Vec<f32>` of configured dimension (768). Error = `EmbeddingError::ProviderUnavailable` on Ollama failure. Must not block caller longer than timeout (500ms for sync path, 5s for batch). Must not exceed concurrency semaphore (4 concurrent calls)

- **Seam: Session transcript → skill extraction**
  - **Adapter:** `ClaudeExtractor` (default, in `infrastructure`, implements `TranscriptSkillExtractionService`) + `OllamaExtractor` (optional, config-gated, in `infrastructure`)
  - **Contract:** Input = `SessionTranscript` (parsed JSONL). Output = `ExtractionResult { name, description, tags, procedures, conventions, assets }`. Error = `ExtractionError::ProviderUnavailable`. Both providers produce identical JSON output schema. Config `provider` field routes to concrete implementation

- **Seam: Scope resolution**
  - **Adapter:** `GitRootProjectResolver` + `EnvPathGlobalResolver` (both in `infrastructure`, implement `ScopeResolver`)
  - **Contract:** `resolve()` returns path array per scope. Project scope = git root directory. Global scope = paths from `SKILL_GLOBAL_PATHS` env var constrained by required `SKILL_GLOBAL_ALLOWED_ROOTS` absolute allowlist (no implicit fallback). V2 adds `RemoteTeamScopeResolver` implementing same trait

- **Seam: Retrieval pipeline**
  - **Adapter:** `DualScopeRetriever` (in `retrieval`) — orchestrates concurrent scope searches, scoring, MMR, RRF
  - **Contract:** Input = prompt text + optional scope filter. Output = `RetrievalOutcome { skills, degraded_scopes, reason_codes }`. Timeout = 400ms per scope search. Must not depend on MCP transport. Must not depend on compilation

- **Seam: Context compilation**
  - **Adapter:** `TemplateOnlyCompiler` (V1, in `compiler`, implements `ContextCompiler`) + `OllamaGuidanceCompiler` (V2, same trait)
  - **Contract:** Input = `Vec<ScoredSkill>` + prompt text. Output = structured markdown string. Must not depend on retrieval pipeline internals. Must not make I/O calls. Template format is stable contract for Claude Code `additionalContext`

- **Seam: Service-to-service communication**
  - **Adapter:** Redis Streams (in `infrastructure`) with event envelope contract
  - **Contract:** Events are idempotent (keyed by `idempotency_key`). Consumers use Redis consumer groups with ACK. Dead letter queue after 3 delivery attempts. Stream trimmed at `MAXLEN ~100_000`. Idempotency tracked via Redis SETEX with 24h TTL (not in-memory `HashSet`)

- **Seam: Qdrant-PG dual persistence**
  - **Adapter:** `GraphWriteCoordinator` (in `infrastructure` or `graph-builder`) — wraps PG+Qdrant writes with outbox pattern
  - **Contract:** Writes mutation intent to PG outbox first (transactional). Async worker reads outbox, writes to Qdrant, marks complete. Reconciliation job finds orphaned vectors and missing embeddings. Not a distributed transaction — eventual consistency with idempotent replay

## Design-It-Twice Options

- **Option A: Single legacy `skill-core` crate containing both domain and infrastructure (original plan)**
  - Pros: Fewer crates, simpler workspace, no trait duplication risk
  - Cons: Violates plan's own stated purity rule. Tests pull in sqlx/qdrant-client/redis deps. Cannot import pure domain types without heavy deps. Both reviewers independently flagged this as P1
- **Option B: Split `domain` + `infrastructure` (chosen)**
  - Pros: Domain crate compile time is instant (no heavy deps). Domain crate importable everywhere. Trait-only boundary enforced by compiler. Infrastructure crate isolates adapter churn
  - Cons: One more crate to manage. Must ensure infrastructure crate re-exports domain types so consumers import from one place
- **Chosen for now:** Option B. The cost of adding one crate at greenfield is zero. The cost of splitting after code exists is high. Both reviewers converged on this independently

- **Option A: `graph-builder` as a monolith (original plan — watcher + extraction + embeddings + HDBSCAN + merge + retire + cron + admin tools)**
  - Pros: One crate to understand, simpler dependency graph
  - Cons: 10+ responsibilities. Merge/retire are policy workflows, not graph construction. Admin tools are online/debug in an offline crate. When V2 adds team scope, this crate explodes
- **Option B: Split into `graph-builder` + `maintenance` + `admin` (chosen)**
  - Pros: Graph construction = one job. Maintenance = policy evaluation. Admin = online debug. Each crate has a single reason to change
  - Cons: Three crates instead of one. Maintenance depends on graph-builder output, admin depends on both
- **Chosen for now:** Option B. The split aligns with different change frequencies: graph construction changes with extraction improvements, maintenance changes with policy tuning, admin changes with debug needs. If this proves too fine-grained during Phase 1, merge `admin` into `mcp-server` (it's already an MCP surface) and defer `maintenance` until Phase 2.3

## Context Tiers

- **Global context:** Constitution v1.0.0 (5 principles, 8 approval boundaries). `domain` types and traits (stable vocabulary). Config defaults (`SKILL_GLOBAL_PATHS`, `SKILL_GLOBAL_ALLOWED_ROOTS`, `RUST_LOG`, provider settings). PG schema contracts. Redis event envelope schema. Docker Compose topology. Crate dependency direction rules (domain ← infrastructure ← service crates)
- **On-demand context:** This architecture artifact (deepening candidates, design-it-twice, drift checks). Vertical-slice architecture contract. SkillRAE paper (arXiv:2605.10114) for scoring formula details. PG schema ERD. Qdrant collection config. Latency budget table. Event catalog with payload schemas
- **Ticket-local context:** Exact feature home (which crate). Files list from execution slice. Scope fence and non-goals. Acceptance criteria. Evidence command (`cargo test --workspace` or `docker compose -f docker-compose.test.yml`). Problem narrative + user story linkage. Any slice-specific architectural decisions

## Recommendations for `/deepen-plan`

- Keep the 9-crate feature-home split in every slice; do not collapse it back into `mcp-server` or `graph-builder` modules.
- Ensure Slice 1.2/1.3 show the explicit `mcp-server` → `retrieval`/`compiler` delegation rather than embedding retrieval logic in transport handlers.
- Ensure Slice 2.2 uses `transcript_ref` + mounted transcript root, not raw host paths.
- Ensure Slice 2.1/2.4/3.3 all point at the same invalidation rule: PG commit → outbox drain → `graph_version` bump → `graph.rebuilt`.
- Keep `cargo tree -p domain --depth 1` as a CI gate to enforce zero infrastructure deps.

## Recommendations for `/workflows:work`

- Build `domain` first and verify zero infrastructure deps before adding any service logic.
- Build `infrastructure` second: `OllamaEmbeddingService`, PG pool, Redis client, scope resolvers, resilience utilities. All with integration tests against Docker Compose containers.
- Service crates must never import `reqwest`, `sqlx`, or `redis` directly — always through `infrastructure` re-exports.
- Graph builder must use `GraphWriteCoordinator` (outbox pattern) for all PG+Qdrant writes — never write to both stores independently.
- Filesystem watcher events must include idempotency key (`{file_path}:{mtime_hash}`) or use file content hash.
- Session state in MCP server must use Redis SETEX, not in-memory `DashMap`, from Slice 1.2 onward.
- Tool handlers in `mcp-server` must be thin delegations to `retrieval`, `compiler`, and `session-extractor`. No business logic in tool handler code.
- `.pending` file TTL defaults: 30 days before cleanup warning, 90 days before expiry metadata. No auto-deletion. Include `warning_at` and `expires_at` in frontmatter.

## Recommendations for `/workflows:review`

- Verify `domain` has zero dependencies on `sqlx`, `qdrant-client`, `redis`, `reqwest`, `tokio` (only `tokio` allowed for async trait methods).
- Verify `infrastructure` is the only crate that directly instantiates Ollama/Qdrant/PG/Redis clients.
- Verify `mcp-server` tool handlers are thin delegation, not business logic.
- Verify all Redis events include `correlation_id` and `idempotency_key`.
- Verify filesystem watcher uses `notify-debouncer-full` with `FileIdMap` for rename detection.
- Verify graph builder uses `GraphWriteCoordinator` (outbox) for all PG+Qdrant dual writes.
- Verify merge/retire workflows produce filesystem proposals (`.pending`/`.retired`), never auto-apply mutations.
- Verify graceful degrade: every infrastructure call in MCP server has a timeout and explicit `degraded`/`no_match` semantics.
- Verify constitution compliance: local-first, zero-touch, human gate, portable scope, filesystem-observable.
- Verify `cargo tree -p domain --depth 1` shows only std/core deps (CI gate).

## Drift Checks

- **Feature-home drift:** Retrieval logic leaking into `mcp-server` tool handlers. Compilation logic leaking into retrieval modules. Admin tools in graph-builder. Merge/retire policy in graph-builder. Any of these = refactoring to fix would require splitting code that's already coupled.
- **Shared/global drift:** `domain` accumulating infrastructure imports (sqlx, qdrant-client, redis). `infrastructure` accumulating business rules. Concrete impls duplicated across service crates instead of living in infrastructure.
- **Horizontal scattering:** Scoring formula appearing in both retrieval and graph-builder. Scope resolution logic duplicated in MCP server and graph-builder. Template formatting duplicated in compilation and admin tools.
- **Unearned abstractions:** Adding a trait before a concrete use case exists. Adding a junction table before the access pattern justifies it. Adding a cache layer before measuring actual latency.

## Resolved Operational Choices

- **`admin` stays a separate crate but is composed into the online binary via router composition.** Split ownership, single deployment surface.
- **`session-extractor` stays a separate crate but its MCP router is also composed into the online binary for V1.1.** If extraction load becomes disruptive later, it can graduate to its own process without changing the tool contract.
- **Transcript ingress uses `transcript_ref` under a mounted transcript root.** This closes the Docker trust boundary without inventing a new host-path exception.
