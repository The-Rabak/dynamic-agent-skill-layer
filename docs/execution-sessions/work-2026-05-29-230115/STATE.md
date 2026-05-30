---
source_type: ticket
plan_file: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
ticket_index: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
tickets:
  - docs/tickets/2026-05-21-skill-layer-v1-1/14-extraction-prompt-review-and-unification.md
  - docs/tickets/2026-05-21-skill-layer-v1-1/15a-live-harness-factory-and-roundtrip-validation.md
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-29T23:01:15Z
completed: 2026-05-29T23:20:00Z
status: completed
execution_shape: vertical-slices
current_unit: 2
total_units: 2
session_id: work-2026-05-29-230115
review_mode: bulk
---

## WHY Context

### Problem Narrative
Developer using multiple coding agent harnesses faces triple compound cost: manual skill selection wastes 5-10 min per task, skill libraries rot unused, each harness operates in a silo. Skills built in one never transfer to another.

### User Story
As a solo developer using multiple coding agent harnesses, I need a zero-touch, self-growing skill context layer that searches project-local and global machine-wide skill scopes concurrently, merges results via weighted RRF + MMR, compiles relevant skills at session start, auto-extracts at session end, and offline deduplicates/merges/retires, so every session starts with perfectly scoped context in under 2 seconds and every session grows the right skill graph.

### Architectural Context
Nine Rust crates with explicit feature homes, Docker Compose with 5 infrastructure containers. MCP protocol is harness boundary. Redis Streams is internal event bus. PG is shared integration point. Filesystem is human-approval UI. Extraction pipeline: SessionExtractor coordinates transcript loading, provider routing (Claude/Ollama), LLM extraction, .pending draft writing, and lifecycle event publishing.

### Success Criteria
- SC-1: Zero-touch context injection <500ms
- SC-2: Dual-scope concurrent retrieval with MMR-then-RRF
- SC-3: Session-end skill extraction with .pending human approval
- SC-4: Offline graph maintenance (merge, retire, cron)
- SC-5: Filesystem-observable state
- SC-6: Subunit-aware compilation
- SC-7: Graceful degrade on any infrastructure failure
- SC-8: V2 readiness

### TDD Contract
- Precedence: plan_overrides_local
- Effective mode: Ralph-driven TDD
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: unit (`cargo test --workspace`) + e2e (`docker compose -f docker-compose.test.yml up --abort-on-container-exit` or `./scripts/run-e2e-tests.sh`)
- Exceptions: None

### Constitution Context
- Version: 1.0.0
- Principles: local-first, zero-touch, human-gate, portable-scope, filesystem-observable
- Waivers: None
- Required approvals: skill creation/retirement, tag creation, schema migrations, model changes, event contract changes, infra config changes

### Architecture Handoff
- Artifact: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
- Feature homes: domain, infrastructure, mcp-server, retrieval, compiler, graph-builder, maintenance, admin, session-extractor
- Shared / global decisions: domain traits (EmbeddingService, TranscriptSkillExtractionService, ScopeResolver, ContextCompiler), infrastructure adapters, Redis event contracts, PG schema
- Seams: extraction adapters (ClaudeExtractor implements TranscriptSkillExtractionService, OllamaExtractor implements same trait)
- Extraction contract: both providers produce identical ExtractionResult schema

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T14 - Extraction prompt review and unification | hardening | SC-3: extraction outputs stay contract-stable and provider-parity-safe | completed | 1 | unit-01-t14-extraction-prompt.md |
| 2 | T15a - Live harness factory and roundtrip validation | hardening | SC-1 + SC-4: live runtime context injection + PG-to-Qdrant durability validation | completed | 1 | unit-02-t15a-harness-factory.md |

## Learnings Brief
- [extraction] Provider asymmetry (Claude external endpoint owns prompting, Ollama needs local prompt) is architectural feature, not bug
- [extraction] Semantic contract (what to extract) shared; syntactic contract (how to prompt) may differ per provider
- [extraction] Ollama `format: "json"` requires embedding schema in prompt text — no `tool_choice`/`strict` support
- [testing] `RetrievalOrchestrator` only works with `SeededGraph` — "live" mode loads PG data into SeededGraph, not bypasses pipeline
- [testing] `PostgresGraphSnapshotStore::list_graph_snapshot()` returns `Vec<PersistedGraphSkillRecord>` usable for live test seeding