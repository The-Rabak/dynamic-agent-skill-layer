---
date: 2026-05-21
topic: compiled-context-layer-skill-rae
status: complete
handoff:
  problem_narrative: true
  user_story: true
  architectural_context: true
  success_criteria: true
---

# Compiled Context Layer for Agentic Harnesses (SkillRAE Implementation)

## Problem Narrative

A developer using multiple coding agent harnesses (Claude Code, OpenCode, Copilot, Codex) faces a triple compound cost every session: manual skill selection wastes 5-10 minutes per task on context setup, skill libraries accumulate but rot unused because maintenance overhead exceeds utility, and each harness operates in a silo — skills built in one never transfer to another. The problem spans two scopes: project-local skills capturing repo-specific patterns, and global machine-wide skills capturing cross-project expertise. Existing approaches force the developer to choose one scope or manually curate both, with no concurrent search or intelligent fusion. The SkillRAE paper (arXiv:2605.10114) proves that multi-level skill graph retrieval + context compilation delivers 11.7% improvement over SOTA on SkillsBench, and weighted RRF + MMR are the right fusion strategies for multi-scope evidence. This is the right time to implement this as local-first Docker-deployed infrastructure that eventually scales to team-wide sharing.

## User Story

As a solo developer using multiple coding agent harnesses,
I need a zero-touch, self-growing skill context layer that searches both project-local and global machine-wide skill scopes concurrently, merges results via weighted RRF + MMR, and at session start compiles relevant skills into a task-specific compact context, while at session end auto-extracts new skills from session activity into the appropriate scope, and offline deduplicates, merges, and retires stale skills across both scopes,
so that every session starts with perfectly scoped, de-duplicated context in under two seconds, and every session grows the right skill graph,
because currently I manually select and scope skills, skills rot unused, and nothing transfers between projects or harnesses,
which causes compounding time loss, dead skill libraries, and zero cross-scope intelligence.

## Success Criteria

- Session start: zero-touch context injection within 2 seconds of first prompt in Claude Code (V1)
- Session end: automatic skill extraction from session transcript into project or global scope, triggered by harness end-hook
- Offline graph maintenance: skill deduplication (merge similar skills across scopes), retirement of stale/unused skills, and community re-detection run as async background tasks
- Cross-scope retrieval: concurrent project-local and global scope search with RRF + MMR fusion produces de-duplicated, relevance-ordered skill set
- Observable behavior: developer types a raw task in Claude Code, relevant skills with task-specific guidance appear in session context without any manual intervention
- V2 readiness: architecture supports adding team-shared remote scope without data layer migration

## Architectural Context

- **Lives in:** New root-level service suite in `dynamic-agent-skill-layer/`, deployed via Docker Compose
- **Feature home:** Three Rust service crates: `skill-graph-builder` (offline async), `skill-mcp-server` (online sync), `session-hook-processor` (post-session async). Shared internal library crate for embeddings, graph operations, and scope resolution.
- **Interacts with:**
  - Claude Code (V1): via MCP protocol. Hooks are configured in `.claude/settings.json` using `type: "mcp_tool"` to invoke our MCP server's tools:
    - `UserPromptSubmit` hook → calls `compile_context` (first prompt only, session-scoped state flag). Returns `additionalContext` injected into session.
    - `SessionEnd` hook → calls `extract_session` with `transcript_path`. Initiates async extraction, returns immediately (<1.5s timeout).
  - Ollama (Docker Compose): embedding generation (`nomic-embed-text`, 768-dim) for skills, communities, and subunits. Called by graph builder and MCP server.
  - Qdrant (Docker Compose): vector storage and hybrid similarity search. Stores skill, community, and subunit embeddings with scope-tag payload filters.
  - PostgreSQL (Docker Compose): graph structure (recursive CTE for multi-hop traversal), community membership, scope tags, session logs, merge/retire audit trail. Normalized schema: `skills`, `subunits`, `communities`, `skill_subunits` (junction), `community_skills` (junction).
  - Redis (Docker Compose): service-to-service event streaming. Events: `skill.file_changed`, `graph.rebuilt`, `skill.extraction_requested`, `skill.approved`.
  - Filesystem: graph builder reads SKILL.md files recursively from git root (project scope) and harness skill directories (global scope). Session hook processor writes approved SKILL.md files.
  - Future harnesses (V2): OpenCode, Copilot, Codex via same MCP interface — configure their equivalent hook systems to call our MCP tools.
- **User entry point:** No UI. Developer types a task in Claude Code. `UserPromptSubmit` MCP hook fires, calls `compile_context`, compiled context appears as `additionalContext` in session. Session end triggers `extract_session`.
- **Data:** Skill embeddings (Qdrant), skill-subunit-community graph (PG with recursive CTEs), compiled context packets (ephemeral string, injected into session). Dual scope: `project` (git root, recursive SKILL.md discovery) and `global` (array of harness skill directories from Docker Compose env vars).
- **Dependencies:** Docker runtime. Five containers: Qdrant, PostgreSQL, Ollama, Redis, plus Rust service containers (MCP server runs continuously; graph-builder and hook-processor are triggered by events/cron). Claude Code depends on MCP server for context injection.
- **Shared / global notes:** MCP protocol is the universal interface boundary — all harnesses connect identically. Redis Streams is the internal event bus. Context compilation logic in `skill-mcp-server` is harness-agnostic. Scope resolution (project vs global) is query-time logic. RRF/MMR fusion is shared across all retrieval paths.

### Community Detection and Graph Construction (Offline)

The offline graph construction pipeline runs as an async process, potentially headless, with these steps:

1. **Subunit extraction:** Parse all SKILL.md files in project and global scope directories. Extract subunits — deterministic procedures, file conventions, constraint-like usage statements, and element references. Normalize, deduplicate by exact match, and store in PG with source skill edges.

2. **Embedding generation:** Send all skill descriptions and subunit texts to Ollama (running in Docker Compose). Generate dense embeddings for both skills and subunits. Store skill and subunit embeddings in Qdrant with scope tags.

3. **Community detection:** Derive compact textual representations for each skill from high-IDF subunits. Embed these representations via Ollama. Apply HDBSCAN (preferred over KMeans for variable-sized communities and noise handling) on skill embeddings. Hard clusters become skill communities. Store community assignments in PG with community centroid embeddings in Qdrant. Community labels derived from top-IDF terms in each cluster.

4. **Cross-scope deduplication:** Compare project-local skill embeddings against global skill embeddings. If cosine similarity exceeds threshold AND the LLM (via Ollama) confirms semantic equivalence, merge candidate pairs. Merged skills retain both scope tags but share one embedding and one graph node.

5. **Skill retirement:** For each scope, compute recency-weighted usage score from session logs. Skills below threshold are flagged for LLM review. Offline LLM (Ollama) decides: keep (low usage but structurally important), retire (mark inactive, remove from online retrieval), or merge (sufficiently similar to another skill).

6. **Graph construction:** Build the multi-level graph in PG: community nodes -> skill nodes -> subunit nodes, with extraction edges between skills and subunits. Subunits can belong to multiple skills (many-to-many). Communities have one-to-many relationship with skills.

## Chosen Approach

**Microservices via Docker Compose (Rust + Qdrant + PostgreSQL + Redis + Ollama)**

Three Rust service crates deployed as separate Docker containers, connected via Redis Streams for event-driven communication, sharing Qdrant (vectors) and PostgreSQL (graph) as the data layer:

- `skill-graph-builder`: Offline async — graph construction, community detection (HDBSCAN + tag augmentation), subunit extraction (hybrid: structural rules + Ollama JSON fallback), cross-scope deduplication, merge (produces consolidated SKILL.md), and retirement (`.retired` extension). Runs triggered by filesystem watcher (incremental updates on `skill.file_changed` Redis events) + cron (periodic full rebuild with merge/retire passes). Publishes `graph.rebuilt` event on completion.

- `skill-mcp-server`: Online sync — MCP server exposed to Claude Code. `UserPromptSubmit` hook fires on first prompt only (state keyed by `{session_id, repo_path}`), calls `compile_context` tool. Concurrent dual-scope retrieval: top-down (community centroid matching via Qdrant) + bottom-up (subunit projection via Qdrant + PG edge lookup), combined skill scoring, relevance-threshold filtering, MMR per-scope deduplication, RRF cross-scope fusion, rescue-aware subunit attachment, template-based context compilation into structured markdown. Returns `additionalContext`. Target <500ms (template-only, no LLM guidance). Graceful degrade on infra failure: returns empty context.

- `session-hook-processor`: Post-session async — `SessionEnd` MCP tool hook calls `extract_session` with `transcript_path` from hook payload. Returns immediately (<1.5s timeout). Async: headless Claude Code reads transcript JSONL, analyzes session context via Ollama, extracts candidate skills/subunits/tags. Writes `.pending` draft SKILL.md files to appropriate scope directory. Filesystem watcher detects `.pending` files for human review — rename to `.md` to approve, delete to reject. Publishes `skill.extraction_requested` event. Human approval triggers `skill.file_changed` → incremental graph rebuild.

This approach fully addresses the user story: zero-touch via MCP hooks (UserPromptSubmit + SessionEnd), dual-scope retrieval with MMR-then-RRF fusion, self-growing via session-end extraction with human-approval gate, offline maintenance for deduplication/merge/retirement. PG from day one avoids V2 migration pain. Docker Compose with 5 infrastructure containers + service crates keeps deployment manageable for solo use while ready for team scale.

## Key Decisions

- **Rust over Python:** Zero-cost latency for the sync MCP server. Embedding calls to Ollama are the only I/O overhead. Rust's async runtime (tokio) handles concurrent scope searches naturally. Python would add 10-50x latency overhead that breaks the 2-second budget.
- **HDBSCAN over KMeans for communities:** HDBSCAN handles variable-sized communities and marks noise skills (skills that don't clearly belong to any community). Better fit for organic skill growth where clusters aren't uniform. Article used KMeans for simplicity; this is a production upgrade.
- **Ollama in Docker Compose:** Unified deployment. All services share one Docker network. No external dependency management. Embedding model: nomic-embed-text (768-dim), configurable.
- **PG from day one (not SQLite):** V2 cross-team sharing requires concurrent write safety and networked access — PG provides this natively. Repository pattern abstraction unnecessary if PG is already the relational store. SQLite would require migration for V2.
- **MCP as harness interface (not custom protocol):** MCP is the emerging standard for agent-tool communication. Claude Code, OpenCode, and Copilot all support MCP natively. No custom protocol to maintain. Single `skill-mcp-server` binary serves all harnesses.
- **Recursive CTEs for graph traversal in PG:** Fully normalized schema (skills, subunits, communities, junction tables). Multi-hop graph queries (e.g., "find communities sharing subunits between selected skills") use recursive CTEs. Clean FK integrity, no JSONB denormalization drift.
- **Redis Streams for service-to-service communication:** Event-driven architecture. Graph builder publishes `skill.file_changed` and `graph.rebuilt` events. MCP server subscribes to `graph.rebuilt` for cache invalidation. Hook processor subscribes to `skill.approved`. Redis added to Docker Compose (5 containers: Qdrant, PG, Ollama, Redis, + service crates).
- **Claude Code native lifecycle hooks (SessionStart, UserPromptSubmit, SessionEnd):**
  - `UserPromptSubmit` hook (first prompt only): MCP tool hook calls our `skill-mcp-server`'s `compile_context` tool with raw prompt text. Server tracks per-session "already compiled" state. First call compiles and returns `additionalContext`; subsequent calls return empty. 30s hook timeout window; target <2s compilation.
  - `SessionEnd` hook: MCP tool hook calls our `session-hook-processor`'s `extract_session` tool with `transcript_path` from hook JSON. Default 1.5s timeout — hook initiates async extraction and returns immediately.
  - `SessionStart` hook (optional V1): Can inject session preamble or bootstrap state. Not used for task-specific compilation (prompt text not yet available).
- **Skill directory discovery:** Recursive scan from git root for SKILL.md files. A skill may be part of a directory with supplementary files (`references/`, `scripts/`, `assets/`); these are stored as reference links on the skill node, not indexed as subunits.

## Approaches Considered

**Approach A: Monolithic Rust service with SQLite:** Rejected because SQLite creates V2 migration debt. Monolith harder to scale components independently (graph building can be memory-heavy while MCP server must be latency-optimized).

**Approach C: Qdrant-only architecture (no relational DB):** Rejected because Qdrant payload-based graph queries are inadequate for multi-hop traversal, community membership queries, and transactional merge/retire operations. The paper's architecture inherently needs both vector search AND graph structure.

## Stakeholder Impact

- **End user (developer):** Every Claude Code session starts with automatically compiled, scope-aware skill context. Zero manual skill selection. Session-end skill extraction preserves institutional knowledge. Cross-project skills transfer automatically.
- **Developers (this codebase):** Three Rust crates, shared internal library for embeddings and graph operations. Docker Compose for local dev. Well-defined service boundaries with PG as integration point.
- **Operations:** Docker Compose for deployment. Four containers: Qdrant, PG, Ollama, and whichever Rust service is active (MCP server runs continuously; graph-builder and hook-processor are triggered/periodic). Ollama requires GPU for acceptable embedding speed; CPU fallback available.
- **Business:** V1: personal productivity multiplier — less context-switching, more coding. V2: team-wide skill sharing creates compounding institutional intelligence.

## Constitution Alignment

- **Relevant project rules:** No constitution.md exists yet. This project is greenfield. This brainstorm will inform the creation of a constitution during planning.
- **No amendment needed because:** Greenfield project.
- **Proposed amendment (if any):** N/A — constitution to be established in planning phase.

## Open Questions

(None — all resolved during Phase 4 dialogue)

## Resolved Questions

- **V1 harness:** Claude Code (not OpenCode). Session integration via `UserPromptSubmit` and `SessionEnd` MCP tool hooks configured in `.claude/settings.json`.
- **Language:** Rust, not Python.
- **Deployment:** Docker Compose, fully local. Five containers: Qdrant, PostgreSQL, Ollama, Redis, + Rust service crates.
- **Existing infrastructure:** No Nucleus dependency. Built from scratch.
- **Data layer:** Qdrant + PostgreSQL. Not SQLite, not Qdrant-only.
- **Community detection:** HDBSCAN with optional tag-based community augmentation. Skills have dual membership (HDBSCAN community + tag-based communities). Many-to-many community_skills junction.
- **Scope merging:** MMR first (per-scope deduplication), then RRF (cross-scope fusion). Relevance-threshold driven (no fixed K).
- **V1 scope:** Solo local machine, two tiers: project (git root) + global (array of harness skill dirs, configurable via env vars).
- **Offline maintenance:** Skill deduplication, merging (creates consolidated SKILL.md), and retirement (`.retired` file extension).
- **Embedding model:** nomic-embed-text (768-dim) via Ollama.
- **Graph rebuild trigger:** Filesystem watcher for incremental updates + cron for periodic full rebuild with merge/retire passes.
- **Skill extraction:** SessionEnd hook → headless Claude Code reads transcript_path → proposes `.pending` draft SKILL.md files → human renames to `.md` to approve, deletes to reject → filesystem watcher triggers graph rebuild.
- **MCP tool surface:** 9 granular tools: `compile_context`, `extract_session`, `rebuild_graph`, `inspect_skill`, `list_communities`, `get_pending_extractions`, `approve_extraction`, `reject_extraction`, `find_skill`.
- **Session end hook format:** Claude Code `SessionEnd` hook provides `transcript_path` (JSONL) in hook payload. MCP tool hook calls `extract_session` with that path. Returns immediately (<1.5s timeout), async extraction starts.
- **Merge/retire thresholds:** Fixed defaults (cosine similarity > 0.85 triggers LLM semantic equivalence check; usage < 1 per month triggers retirement review) with configurable overrides per scope. Retirement uses filesystem `.retired` extension pattern.
- **Scope boundary definition:** Git repo root auto-detected as project scope boundary. Recursive SKILL.md discovery.
- **Skill definition:** Dual existence (file + graph node). May include supplementary directory files stored as reference links on the node.
- **Subunit extraction:** Hybrid — deterministic structural rules for 80% + Ollama fallback (JSON array of `{type, content, source_heading}`). Three types: procedure, convention, asset.
- **Tag system:** Frontmatter tags in SKILL.md. Offline skill creation can suggest new tags in `.pending` files. Human approves alongside skill.
- **Data model:** Fully normalized PG schema with recursive CTEs for multi-hop graph traversal. Tables: `skills`, `subunits`, `communities`, `skill_subunits` (junction), `community_skills` (junction). Subunits deduplicated globally, many-to-many with skills.
- **Service communication:** Redis Streams event-driven. Events: `skill.file_changed`, `graph.rebuilt`, `skill.extraction_requested`, `skill.approved`.
- **Session lifecycle:** `UserPromptSubmit` hook (first prompt only, session-scoped state keyed by `{session_id, repo_path}`) → calls `compile_context` → returns `additionalContext` injected into session. `SessionEnd` hook → calls `extract_session` → async processing.
- **Compilation output:** Structured markdown template with sections. Template-only guidance in V1 (no LLM-synthesized guidance). V2 adds LLM-generated task guidance.
- **Edge cases:** Cold start → return empty context, no-op. Ollama/Qdrant/PG/Redis unreachable → graceful degrade with retry.
- **V2 team scope:** Remote PG instance + shared Qdrant index. Same architecture extended with remote scope config. No migration needed.
- **Ollama in Docker Compose:** One more container in the stack. GPU passthrough recommended for production-speed embeddings.

## Next Steps

-> `/workflows:plan` for implementation phases, service decomposition, and execution slices.
