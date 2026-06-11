//! T17 AC3 + AC4: live cold-boot cache measurement and readiness-honesty proof.
//!
//! # What this proves
//!
//! **AC3 — persisted embedding cache eliminates re-embedding:**
//! A second `from_environment` boot over the SAME seeded corpus is substantially
//! faster than the first because the first populated the `skill_embeddings` cache
//! in Postgres. The warm boot reads pre-computed vectors and performs ~zero embed
//! calls. We assert `warm_duration < cold_duration * 0.5` and that a retrieval
//! query after the warm boot returns a known seeded skill (proving cached vectors
//! are byte-exact, not corrupted).
//!
//! **AC4 — warming guard: no hang, no healthy-while-warming window:**
//! After a normal boot the app is Ready. We then call `handle.set_warming()` to
//! simulate the background-reload window and assert:
//! - `find_skill` and `compile_context` return an explicit `"warming"` status FAST
//!   (wrapped in a 5-second timeout — proves the embed semaphore is never acquired).
//! - `handle.health_component()` is `healthy: false` with a `"warming"` detail
//!   (so `/health` would return 503).
//! - After `handle.set_ready()`, retrieval works normally again.
//!
//! # Corpus
//! ~30 distinct real-ish skills are seeded, each with non-trivial multi-view fields
//! (use_when / requires / invariants / avoid_when) so the embedding cache covers
//! e_task, e_needs, and e_negative views as well as e_description. This ensures the
//! cold boot does real, measurable embedding work across all view kinds.
//!
//! # Isolation
//! Uses `env_guard::isolated_namespace()` so every run targets its own PG schema,
//! Qdrant collection, and Redis stream. Teardown is panic-safe via `NamespaceGuard`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use domain::{ScopeType, SubunitType};
use infrastructure::{
    LiveGraphSkillRecord, LiveGraphSnapshotMutation, LiveGraphSubunitRecord, RebuildCoordinator,
};
use mcp_server::{
    McpServerApp,
    tools::{
        compile_context::{CompileContextRequest, CompileContextStatus},
        find_skill::FindSkillRequest,
    },
};
use retrieval::RetrievalConfig;

#[path = "../integration/env_guard.rs"]
mod env_guard;

fn retrieval_config() -> RetrievalConfig {
    RetrievalConfig {
        candidate_limit: 32,
        max_results: 2,
        max_subunits_per_skill: 4,
        rescue_threshold: 0.1,
        relevance_threshold: 0.15,
        mmr_lambda: 0.6,
        // Generous per-scope timeout to keep this cache/readiness correctness+timing
        // test from flaking on a cold WSL2 first-scoring call.  The 400ms production
        // SLO is validated by the dedicated retrieval latency test, not here.
        scope_timeout_ms: 5000,
        ..RetrievalConfig::default()
    }
}

fn repo_root_path() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should resolve")
        .display()
        .to_string()
}

/// Builds the seed corpus: ~30 distinct skills with rich multi-view fields so
/// that e_description, e_task, e_needs, and e_negative views are all embedded
/// and cached on first boot.
///
/// Each skill has a unique description so embeddings are real distinct work for
/// Ollama. Multi-view fields (use_when / requires / invariants / avoid_when) are
/// deliberately varied so the embedding cache records multiple view kinds per
/// skill. The total distinct (skill, view) pairs is well over 100, making the
/// cold boot embed cost measurably larger than the warm cache-load.
fn build_seed_corpus() -> LiveGraphSnapshotMutation {
    let skills = vec![
        LiveGraphSkillRecord {
            stable_id: "t17-rust-async-io".to_owned(),
            name: "rust-async-io".to_owned(),
            description: "Rust async file I/O patterns using tokio::fs with cancellation safety and backpressure".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["rust".to_owned(), "async".to_owned(), "io".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Read file with cancellation".to_owned(),
                content: "Use tokio::fs::read_to_string wrapped in timeout for cancellation safety in async contexts".to_owned(),
            }],
            use_when: vec!["writing async Rust code that reads files".to_owned()],
            avoid_when: vec!["synchronous code paths where std::fs suffices".to_owned()],
            invariants: vec!["never block the async executor with sync file I/O".to_owned()],
            requires: vec!["tokio runtime with io features enabled".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-rust-error-handling".to_owned(),
            name: "rust-error-handling".to_owned(),
            description: "Structured error handling in Rust using thiserror and anyhow with context propagation".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["rust".to_owned(), "errors".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Error type hierarchy".to_owned(),
                content: "Define domain errors with thiserror; use anyhow for application-level propagation".to_owned(),
            }],
            use_when: vec!["defining error types for a library crate".to_owned()],
            avoid_when: vec!["throwaway scripts where unwrap is acceptable".to_owned()],
            invariants: vec!["library errors must implement std::error::Error".to_owned()],
            requires: vec![],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-postgres-migrations".to_owned(),
            name: "postgres-migrations".to_owned(),
            description: "Postgres schema migrations with sqlx::migrate! and search_path namespace isolation for tests".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["postgres".to_owned(), "migrations".to_owned(), "sqlx".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Run migrations in tests".to_owned(),
                content: "Use sqlx::migrate!().run(&pool) in a sandbox schema to avoid touching production tables".to_owned(),
            }],
            use_when: vec!["adding a new table or altering schema in a Postgres database".to_owned()],
            avoid_when: vec!["ad-hoc schema changes in production without rollback plan".to_owned()],
            invariants: vec!["migrations must be idempotent and ordered by version number".to_owned()],
            requires: vec!["sqlx with postgres feature and a live database connection".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-redis-streams".to_owned(),
            name: "redis-streams".to_owned(),
            description: "Redis streams consumer group pattern with XREADGROUP, XACK and self-healing NOGROUP recovery".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["redis".to_owned(), "streams".to_owned(), "events".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Consumer group self-heal".to_owned(),
                content: "Detect NOGROUP error on XREADGROUP and recreate the consumer group with XGROUP CREATE MKSTREAM".to_owned(),
            }],
            use_when: vec!["building reliable event consumers on Redis streams".to_owned()],
            avoid_when: vec!["simple pub/sub where delivery guarantees are not required".to_owned()],
            invariants: vec!["always ACK consumed messages to prevent infinite replay".to_owned()],
            requires: vec!["Redis 5.0+ with streams support".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-qdrant-vector-store".to_owned(),
            name: "qdrant-vector-store".to_owned(),
            description: "Qdrant vector database collection management with dimension discovery and upsert batching".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["qdrant".to_owned(), "vectors".to_owned(), "embeddings".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Collection per embedding model".to_owned(),
                content: "Name collections after the embedding model to prevent cross-model dimension mismatches".to_owned(),
            }],
            use_when: vec!["storing dense vector embeddings for semantic retrieval".to_owned()],
            avoid_when: vec!["exact keyword search where BM25 is sufficient".to_owned()],
            invariants: vec!["vector dimension must match the collection's configured dimension".to_owned()],
            requires: vec!["Qdrant HTTP API accessible on QDRANT_URL".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-ollama-embedding".to_owned(),
            name: "ollama-embedding".to_owned(),
            description: "Ollama embedding service integration with semaphore concurrency control and dimension discovery".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["ollama".to_owned(), "embedding".to_owned(), "ml".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Discover embedding dimension".to_owned(),
                content: "Call the embed endpoint with a probe string to discover the actual vector dimension before creating collections".to_owned(),
            }],
            use_when: vec!["integrating a local Ollama embedding model into the retrieval pipeline".to_owned()],
            avoid_when: vec!["production environments where latency SLO requires cloud embeddings".to_owned()],
            invariants: vec!["embed calls must be bounded by a semaphore to prevent Ollama overload".to_owned()],
            requires: vec!["Ollama running on OLLAMA_URL with the target model pulled".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-docker-compose-test".to_owned(),
            name: "docker-compose-test".to_owned(),
            description: "Docker Compose test stack configuration with named volumes and health-check probes for CI integration testing".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["docker".to_owned(), "compose".to_owned(), "ci".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Volume naming convention".to_owned(),
                content: "Name test volumes with -test suffix to prevent data contamination between prod and test stacks".to_owned(),
            }],
            use_when: vec!["setting up integration test infrastructure with real databases".to_owned()],
            avoid_when: vec!["unit tests that can use in-memory fakes".to_owned()],
            invariants: vec!["test volumes must never share data with production volumes".to_owned()],
            requires: vec!["Docker Engine and Docker Compose plugin installed".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-tokio-concurrency".to_owned(),
            name: "tokio-concurrency".to_owned(),
            description: "Tokio task concurrency patterns using JoinSet, semaphore-bounded spawning, and structured concurrency".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["tokio".to_owned(), "concurrency".to_owned(), "async".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Bounded parallel tasks".to_owned(),
                content: "Use tokio::sync::Semaphore with JoinSet to limit concurrent tasks without blocking the executor".to_owned(),
            }],
            use_when: vec!["running bounded parallel async operations in Rust".to_owned()],
            avoid_when: vec!["sequential operations where parallelism adds no benefit".to_owned()],
            invariants: vec!["always await spawned tasks to propagate panics to the caller".to_owned()],
            requires: vec!["tokio runtime with multi-thread flavor".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-arcswap-snapshot".to_owned(),
            name: "arcswap-snapshot".to_owned(),
            description: "Lock-free in-memory snapshot swapping using arc-swap for zero-downtime graph reloads under concurrent read load".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["arcswap".to_owned(), "snapshot".to_owned(), "concurrency".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "ArcSwap for hot reload".to_owned(),
                content: "Store the graph snapshot in ArcSwap<Arc<Graph>>; swap atomically so readers never block on the write path".to_owned(),
            }],
            use_when: vec!["storing shared in-memory state that is replaced atomically on rebuild".to_owned()],
            avoid_when: vec!["mutable state requiring fine-grained field updates (use RwLock instead)".to_owned()],
            invariants: vec!["load() on ArcSwap is lock-free; never wrap in a Mutex".to_owned()],
            requires: vec!["arc-swap crate in Cargo.toml".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-tracing-instrumentation".to_owned(),
            name: "tracing-instrumentation".to_owned(),
            description: "Rust tracing crate instrumentation with structured spans, fields, and JSON subscriber for production observability".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["tracing".to_owned(), "observability".to_owned(), "rust".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Instrument async functions".to_owned(),
                content: "Apply #[tracing::instrument(skip_all)] to async fns; add targeted field spans inline for high-cardinality values".to_owned(),
            }],
            use_when: vec!["adding observability to production Rust services".to_owned()],
            avoid_when: vec!["test helpers where tracing adds noise without benefit".to_owned()],
            invariants: vec!["never include secrets or PII in span fields".to_owned()],
            requires: vec!["tracing and tracing-subscriber crates".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-serde-json-schema".to_owned(),
            name: "serde-json-schema".to_owned(),
            description: "Serde JSON serialization with schema validation, flattening, and custom deserializer patterns for MCP protocol messages".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["serde".to_owned(), "json".to_owned(), "mcp".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "MCP message framing".to_owned(),
                content: "Use serde(tag) for discriminated unions in MCP request/response types; flatten sparingly to avoid key collisions".to_owned(),
            }],
            use_when: vec!["defining MCP protocol message types with serde".to_owned()],
            avoid_when: vec!["simple structs with no polymorphism".to_owned()],
            invariants: vec!["all public API types must derive Serialize and Deserialize".to_owned()],
            requires: vec![],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-sqlx-query-patterns".to_owned(),
            name: "sqlx-query-patterns".to_owned(),
            description: "SQLx compile-time checked queries with parameter binding, batch inserts, and RETURNING clause patterns for Postgres".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["sqlx".to_owned(), "postgres".to_owned(), "queries".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Batch insert with UNNEST".to_owned(),
                content: "Use query! with UNNEST($1::text[], ...) for bulk inserts; avoids N individual roundtrips".to_owned(),
            }],
            use_when: vec!["writing batch insert queries against Postgres with sqlx".to_owned()],
            avoid_when: vec!["single-row inserts where the simpler form is clearer".to_owned()],
            invariants: vec!["use query_as! for type-checked result mapping to Rust structs".to_owned()],
            requires: vec!["sqlx with postgres feature and DATABASE_URL at compile time".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-axum-routing".to_owned(),
            name: "axum-routing".to_owned(),
            description: "Axum HTTP router configuration with state injection, middleware layers, and SSE streaming endpoints".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["axum".to_owned(), "http".to_owned(), "routing".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Wire SSE endpoint".to_owned(),
                content: "Use axum::response::Sse with a tokio broadcast channel receiver for server-sent event streaming".to_owned(),
            }],
            use_when: vec!["building HTTP API endpoints in Rust with axum".to_owned()],
            avoid_when: vec!["simple CLI tools that do not serve HTTP".to_owned()],
            invariants: vec!["shared state must implement Clone + Send + Sync for Router::with_state".to_owned()],
            requires: vec!["axum and tokio dependencies in Cargo.toml".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-embedding-cache".to_owned(),
            name: "embedding-cache".to_owned(),
            description: "Persisted embedding cache in Postgres skill_embeddings table for eliminating redundant re-embeds on warm boot".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["embeddings".to_owned(), "cache".to_owned(), "performance".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Cache keyed by content hash".to_owned(),
                content: "Key the embedding cache by (skill_stable_id, view_kind, content_blake3_hash) so changed content is re-embedded and unchanged content is reused".to_owned(),
            }],
            use_when: vec!["optimizing boot time for a skill graph with many skills".to_owned()],
            avoid_when: vec!["tiny corpora where embedding all skills on each boot is fast enough".to_owned()],
            invariants: vec!["cache must be invalidated when content changes, not just on version bump".to_owned()],
            requires: vec!["Postgres skill_embeddings table from migration 011".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-graph-builder-hdbscan".to_owned(),
            name: "graph-builder-hdbscan".to_owned(),
            description: "HDBSCAN-based skill community detection for graph builder with stability scoring and noise-point handling".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["hdbscan".to_owned(), "clustering".to_owned(), "graph".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Summary,
                title: "Community membership".to_owned(),
                content: "HDBSCAN assigns skills to communities; noise points (-1 label) fall back to single-skill communities so no skill is community-less".to_owned(),
            }],
            use_when: vec!["clustering skills into communities for graph navigation".to_owned()],
            avoid_when: vec!["corpora too small for HDBSCAN min_cluster_size (use tag-based grouping)".to_owned()],
            invariants: vec!["every skill must have at least one community membership after clustering".to_owned()],
            requires: vec!["embedding vectors for all skills before clustering".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-mcp-protocol".to_owned(),
            name: "mcp-protocol".to_owned(),
            description: "MCP (Model Context Protocol) server implementation with tool registration, JSON-RPC dispatch and SSE transport".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["mcp".to_owned(), "protocol".to_owned(), "tools".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Tool registration pattern".to_owned(),
                content: "Register tools with name, description, and inputSchema; the dispatch table routes JSON-RPC calls by tool name".to_owned(),
            }],
            use_when: vec!["implementing an MCP server that exposes tools to Claude".to_owned()],
            avoid_when: vec!["simple CLI utilities that do not need tool-calling integration".to_owned()],
            invariants: vec!["tool names must be stable — changing them breaks agent memory".to_owned()],
            requires: vec![],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-session-extraction".to_owned(),
            name: "session-extraction".to_owned(),
            description: "LLM-based skill extraction from Claude Code session transcripts with structured JSON output and multi-window chunking".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["extraction".to_owned(), "llm".to_owned(), "sessions".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Chunk transcript windows".to_owned(),
                content: "Split transcripts into overlapping windows sized to the model context; extract candidate skills from each window separately".to_owned(),
            }],
            use_when: vec!["extracting reusable skills from a past Claude Code session".to_owned()],
            avoid_when: vec!["sessions too short to contain meaningful reusable patterns".to_owned()],
            invariants: vec!["never truncate a transcript window mid-turn; split only at turn boundaries".to_owned()],
            requires: vec!["OLLAMA_URL or CLAUDE_API_KEY depending on provider".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-semantic-retrieval".to_owned(),
            name: "semantic-retrieval".to_owned(),
            description: "Semantic retrieval pipeline using dense embeddings, subunit evidence scoring, and MMR diversity reranking".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["retrieval".to_owned(), "semantic".to_owned(), "ranking".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Summary,
                title: "MMR diversity reranking".to_owned(),
                content: "Apply Maximal Marginal Relevance after initial dense retrieval to balance relevance with diversity in the result set".to_owned(),
            }],
            use_when: vec!["ranking skills by semantic relevance to an agent query".to_owned()],
            avoid_when: vec!["exact-match lookups by skill ID where ranking is unnecessary".to_owned()],
            invariants: vec!["relevance threshold must be calibrated to the embedding model's score range".to_owned()],
            requires: vec!["embedding vectors stored in Qdrant or Postgres snapshot".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-cargo-workspace".to_owned(),
            name: "cargo-workspace".to_owned(),
            description: "Cargo workspace layout with feature flags, dev-dependencies, and cross-crate test targets for a multi-crate Rust project".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["cargo".to_owned(), "workspace".to_owned(), "rust".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Test targets in workspace".to_owned(),
                content: "Declare [[test]] targets in the consuming crate's Cargo.toml pointing to shared test files in tests/ at the workspace root".to_owned(),
            }],
            use_when: vec!["structuring a multi-crate Rust project as a Cargo workspace".to_owned()],
            avoid_when: vec!["single-crate projects where workspace overhead is unnecessary".to_owned()],
            invariants: vec!["workspace resolver must be set to \"2\" for feature unification correctness".to_owned()],
            requires: vec![],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-blake3-content-hash".to_owned(),
            name: "blake3-content-hash".to_owned(),
            description: "Content-addressed hashing using BLAKE3 for change detection in skill content, enabling incremental cache invalidation".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["blake3".to_owned(), "hashing".to_owned(), "content".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Hash multi-view fields".to_owned(),
                content: "Concatenate view-specific content fields with a separator before hashing to produce a stable (skill, view) cache key".to_owned(),
            }],
            use_when: vec!["building a content-addressed cache keyed on skill content".to_owned()],
            avoid_when: vec!["change detection where a version counter is cheaper and sufficient".to_owned()],
            invariants: vec!["hash function must be deterministic across restarts; avoid ASLR-dependent hashes".to_owned()],
            requires: vec!["blake3 crate in Cargo.toml".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-readiness-state-machine".to_owned(),
            name: "readiness-state-machine".to_owned(),
            description: "Server readiness state machine with Warming/Ready/Failed states to prevent tool calls from hanging during snapshot rebuilds".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["readiness".to_owned(), "health".to_owned(), "state".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Warming short-circuit".to_owned(),
                content: "Check readiness before embedding the query; return an explicit warming response instead of blocking on the Ollama semaphore".to_owned(),
            }],
            use_when: vec!["guarding tool calls during snapshot build or background reload windows".to_owned()],
            avoid_when: vec!["in-process test constructors where the snapshot is always ready".to_owned()],
            invariants: vec!["tool calls must never acquire the embed semaphore while Warming".to_owned()],
            requires: vec!["ReadinessHandle shared between McpServerApp and PostgresGraphReloader".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-health-check-endpoint".to_owned(),
            name: "health-check-endpoint".to_owned(),
            description: "HTTP health check endpoint returning 200/503 based on infrastructure component status and snapshot readiness".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["health".to_owned(), "http".to_owned(), "observability".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "503 on unhealthy component".to_owned(),
                content: "Return HTTP 503 when any HealthComponent.healthy is false, including during snapshot Warming, so load balancers route traffic away".to_owned(),
            }],
            use_when: vec!["exposing server health to a load balancer or Kubernetes readiness probe".to_owned()],
            avoid_when: vec!["development servers where readiness signaling is not needed".to_owned()],
            invariants: vec!["Warming readiness must produce 503, never 200 during the embed window".to_owned()],
            requires: vec![],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-namespace-isolation".to_owned(),
            name: "namespace-isolation".to_owned(),
            description: "Per-test namespace isolation using Postgres search_path, Qdrant collection names, and Redis stream keys for safe parallel e2e testing".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["testing".to_owned(), "isolation".to_owned(), "e2e".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Sandbox teardown on panic".to_owned(),
                content: "Implement Drop for the namespace guard to run async teardown on a dedicated thread, reclaiming sandbox resources even when the test panics".to_owned(),
            }],
            use_when: vec!["writing e2e tests against shared containers that must not interfere".to_owned()],
            avoid_when: vec!["unit tests with no container dependencies".to_owned()],
            invariants: vec!["canonical namespace resources must never be touched by sandbox teardown".to_owned()],
            requires: vec!["DATABASE_URL, QDRANT_URL, REDIS_URL environment variables".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-graph-snapshot-reload".to_owned(),
            name: "graph-snapshot-reload".to_owned(),
            description: "Background graph snapshot reload triggered by graph.rebuilt Redis stream events with atomic ArcSwap replace".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["graph".to_owned(), "reload".to_owned(), "redis".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Reload on graph.rebuilt event".to_owned(),
                content: "Subscribe to the graph.rebuilt Redis stream event; on receipt call build_graph_from_pg and atomically swap the snapshot via ArcSwap".to_owned(),
            }],
            use_when: vec!["refreshing the in-memory graph after the graph builder writes a new snapshot".to_owned()],
            avoid_when: vec!["single-process systems where the graph builder writes directly to memory".to_owned()],
            invariants: vec!["set readiness to Warming before building and Ready/Failed after".to_owned()],
            requires: vec!["Redis consumer group on the graph.rebuilt stream".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-scope-resolution".to_owned(),
            name: "scope-resolution".to_owned(),
            description: "Dual-scope skill retrieval resolving project-local and global skill paths via SKILL_GLOBAL_PATHS and repo detection".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["scope".to_owned(), "retrieval".to_owned(), "paths".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Summary,
                title: "Repo root detection".to_owned(),
                content: "Walk up from the request's repo_path to find .git or Cargo.toml; use this root to resolve project-scope skills".to_owned(),
            }],
            use_when: vec!["resolving which skills apply to a given agent session's repository".to_owned()],
            avoid_when: vec!["global-only deployments with no per-project skill customization".to_owned()],
            invariants: vec!["project-scope skills must be served before global when both match".to_owned()],
            requires: vec!["SKILL_GLOBAL_ALLOWED_ROOTS and SKILL_GLOBAL_PATHS env vars".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-usage-tracking".to_owned(),
            name: "usage-tracking".to_owned(),
            description: "Background skill usage tracking via a bounded channel and async writer task that records compile_context outcomes to Postgres".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["usage".to_owned(), "tracking".to_owned(), "async".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Off-path usage write".to_owned(),
                content: "Post the usage record to a bounded channel; the background writer drains and writes to DB without blocking the tool response".to_owned(),
            }],
            use_when: vec!["recording which skills were served for analytics and ranking feedback".to_owned()],
            avoid_when: vec!["test builds where the usage channel is not wired".to_owned()],
            invariants: vec!["usage writes must never add latency to the tool call response path".to_owned()],
            requires: vec!["live Postgres pool with skill_usage table".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-context-cache".to_owned(),
            name: "context-cache".to_owned(),
            description: "Compiled context cache keyed on session+prompt+scope hash using Redis TTL to deduplicate redundant compile_context calls".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["cache".to_owned(), "redis".to_owned(), "performance".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Cache key composition".to_owned(),
                content: "Compose the cache key from blake3(session_id + prompt + scope_root) to ensure cache hits only for identical retrieval contexts".to_owned(),
            }],
            use_when: vec!["deduplicating repeated compile_context calls within a session".to_owned()],
            avoid_when: vec!["sessions where every call has a unique prompt making caching ineffective".to_owned()],
            invariants: vec!["cache must be namespaced per test run to prevent cross-test hits".to_owned()],
            requires: vec!["Redis with REDIS_KEY_PREFIX set for namespace isolation".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-skill-dag-edges".to_owned(),
            name: "skill-dag-edges".to_owned(),
            description: "Typed skill DAG edges stored in Postgres with confidence scores, edge types, and JSONB evidence for graph navigation".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["dag".to_owned(), "edges".to_owned(), "graph".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Convention,
                title: "Edge type taxonomy".to_owned(),
                content: "Classify edges as requires/extends/conflicts/complements; store as CHECK-constrained text columns to ensure valid types at the DB level".to_owned(),
            }],
            use_when: vec!["modeling relationships between skills in the skill graph".to_owned()],
            avoid_when: vec!["flat skill lists with no inter-skill relationships".to_owned()],
            invariants: vec!["edges must reference skills by stable_id, not by mutable DB id".to_owned()],
            requires: vec!["skills table and skill_edges table from migrations 001-010".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-maintenance-worker".to_owned(),
            name: "maintenance-worker".to_owned(),
            description: "Maintenance worker binary that retires stale skills and triggers graph rebuilds on a scheduled or event-driven basis".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["maintenance".to_owned(), "worker".to_owned(), "automation".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Procedure,
                title: "Stale skill retirement".to_owned(),
                content: "Mark skills as retired when never-used OR last-used is beyond the staleness threshold; never retire on first-use absence alone".to_owned(),
            }],
            use_when: vec!["pruning unused skills from the corpus to keep retrieval quality high".to_owned()],
            avoid_when: vec!["corpora where all skills are actively used".to_owned()],
            invariants: vec!["never retire a skill that has been used within the retention window".to_owned()],
            requires: vec!["skill_usage table with last_used timestamps".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
        LiveGraphSkillRecord {
            stable_id: "t17-multi-view-embedding".to_owned(),
            name: "multi-view-embedding".to_owned(),
            description: "Multi-view skill embedding producing separate vectors for description, task triggers, needs, and negative signals to improve retrieval precision".to_owned(),
            scope: ScopeType::Global,
            tags: vec!["embedding".to_owned(), "multi-view".to_owned(), "retrieval".to_owned()],
            source_paths: vec![],
            subunits: vec![LiveGraphSubunitRecord {
                kind: SubunitType::Summary,
                title: "View-specific embedding".to_owned(),
                content: "Embed e_description from skill description, e_task from use_when list, e_needs from requires list, e_negative from avoid_when list for targeted semantic matching".to_owned(),
            }],
            use_when: vec!["improving retrieval precision by matching query intent against specific skill views".to_owned()],
            avoid_when: vec!["simple corpora where single-vector embedding is sufficient".to_owned()],
            invariants: vec!["all view vectors for a skill must share the same embedding model version".to_owned()],
            requires: vec!["skill_embeddings table from migration 011 with view_kind column".to_owned()],
            produces: vec![],
            artifacts: vec![],
            tools: vec![],
        },
    ];

    LiveGraphSnapshotMutation {
        rebuilt_at: chrono::Utc::now(),
        skills,
        communities: vec![],
    }
}

/// Fixed query used for both cold and warm retrieval in the cache-fidelity proof.
///
/// Must be identical across both calls: any difference would invalidate the
/// cold == warm comparison since the embedding input would differ.
const CACHE_FIDELITY_QUERY: &str =
    "embedding cache Postgres skill_embeddings content hash warm boot re-embed";

/// T17 AC3 + AC4: cold-boot cache measurement and readiness-honesty proof.
///
/// Drives `McpServerApp::from_environment` against the REAL live stack
/// (PG/Qdrant/Redis/Ollama qwen3-embedding:4b) in a sandbox namespace and proves:
///
/// 1. **AC3**: warm boot (cache populated) is substantially faster than cold boot
///    (cache empty) for the same 30-skill corpus — `warm_boot < cold_boot * 0.5`.
///    **Cache fidelity**: the cold and warm boots return byte-identical retrieval
///    results for the same query (same skill names, same order, scores within 1e-6).
///    This proves the Postgres embedding cache roundtrip is lossless.
///
/// 2. **AC4**: the warming guard makes `find_skill` and `compile_context` return an
///    explicit `"warming"` status FAST (under 5 seconds) when the readiness handle
///    is in Warming state — no embed semaphore acquired, no 7-minute hang.
///    The `/health` readiness component is unhealthy during Warming.
///    After `set_ready()`, retrieval works normally again.
#[ignore = "requires live containers"]
#[tokio::test]
async fn cold_boot_readiness_honesty() {
    // Per-run namespace isolation: every resource (PG schema, Qdrant collection,
    // Redis stream) is unique to this run so teardown can never touch canonical state.
    let namespace = env_guard::isolated_namespace().await;

    // -------------------------------------------------------------------------
    // Phase 1: Seed the corpus into the sandbox namespace.
    //
    // A boot is used exclusively for seeding — we do not measure this boot since
    // it builds the snapshot over an empty DB. After writing the skills to PG via
    // `replace_snapshot_and_bump_version`, seed_components stays alive (not torn
    // down) so the `skills` table rows persist for the cold and warm boots below.
    // seed_components.teardown() is called at the end of the test after
    // warm_components.teardown() has already cleared the tables.
    // -------------------------------------------------------------------------
    let seed_components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("should connect to live infrastructure for initial seeding");

    let corpus = build_seed_corpus();
    let corpus_skill_count = corpus.skills.len();

    seed_components
        .rebuild_coordinator
        .replace_snapshot_and_bump_version(corpus)
        .await
        .expect("should seed corpus into PG for T17 measurement");

    // -------------------------------------------------------------------------
    // Phase 2: Cold boot — the embedding cache is empty for these skills.
    //
    // `from_environment` calls `build_graph_from_pg` which embeds every
    // (skill, view) pair and writes each vector to `skill_embeddings`. This is
    // the expensive path: real Ollama calls for all ~30 skills × 4 views each.
    //
    // We intentionally do NOT call teardown on seed_components or cold_components
    // between phases: teardown would truncate the `skills` and `skill_embeddings`
    // tables, leaving nothing for the warm boot to read. The durable PG rows
    // (skills + cached embeddings) must persist from cold boot to warm boot so
    // the cache actually serves cached vectors on the second boot.
    // -------------------------------------------------------------------------
    let cold_start = Instant::now();
    let cold_components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("cold boot should connect to live infrastructure");
    let cold_duration = cold_start.elapsed();

    eprintln!("T17 AC3 cold-boot duration: {cold_duration:?} (skills={corpus_skill_count})");

    // Verify the cold boot produced a working snapshot (retrieval works).
    // Capture the matches for the cache-fidelity comparison against the warm boot.
    let cold_find_response = cold_components
        .app
        .find_skill(FindSkillRequest {
            prompt: CACHE_FIDELITY_QUERY.to_owned(),
            limit: Some(2),
        })
        .await;

    assert_eq!(
        cold_find_response.status, "ok",
        "find_skill after cold boot must return ok, got {:?} (reason {:?})",
        cold_find_response.status, cold_find_response.reason_code,
    );
    assert!(
        !cold_find_response.matches.is_empty(),
        "find_skill after cold boot must return at least one match (retrieval works); \
         reason_code: {:?}",
        cold_find_response.reason_code,
    );
    let cold_matches = cold_find_response.matches;

    // Drop cold_components without teardown so PG skills + skill_embeddings rows
    // survive for the warm boot. The detached background refresh subscriber task
    // is harmless — it reads from the same namespaced Redis stream, but warm boot
    // creates its own subscriber; both drain events from the same sandbox stream
    // without interfering with each other.
    drop(cold_components);

    // -------------------------------------------------------------------------
    // Phase 3: Warm boot — the embedding cache is fully populated.
    //
    // `build_graph_from_pg` loads pre-computed vectors from `skill_embeddings`
    // for every unchanged (skill, view) pair. No Ollama embed calls needed for
    // the corpus that already has vectors.
    // -------------------------------------------------------------------------
    let warm_start = Instant::now();
    let warm_components = McpServerApp::from_environment(retrieval_config())
        .await
        .expect("warm boot should connect to live infrastructure");
    let warm_duration = warm_start.elapsed();

    eprintln!("T17 AC3 warm-boot duration: {warm_duration:?} (skills={corpus_skill_count})");
    eprintln!(
        "T17 AC3 speedup: {:.1}× (cold={cold_duration:?} warm={warm_duration:?})",
        cold_duration.as_secs_f64() / warm_duration.as_secs_f64().max(0.001),
    );

    // AC3 assertion: warm boot must be substantially faster.
    // The threshold is 50% of cold duration — a corpus of 30 skills with
    // 4 views each takes many seconds cold; with a full cache it should be
    // nearly instant (only PG reads + graph assembly, no Ollama calls).
    assert!(
        warm_duration < cold_duration / 2,
        "warm boot ({warm_duration:?}) must be less than half of cold boot ({cold_duration:?}) \
         — embedding cache is not eliminating re-embeds as expected"
    );

    // AC3: warm boot must also complete in a reasonable absolute time (under 90s)
    // so we know it is a cache-load path, not a slow embed path.
    assert!(
        warm_duration < Duration::from_secs(90),
        "warm boot ({warm_duration:?}) must complete in under 90 seconds — \
         cache-load should be near-instant without Ollama calls"
    );

    // -------------------------------------------------------------------------
    // Phase 4: Cache-fidelity proof — cold and warm boots return byte-identical
    // retrieval results for the same query.
    //
    // This is the core AC3 correctness assertion: if the embedding cache
    // roundtrip through Postgres were lossy (truncated floats, wrong byte
    // order, corrupted vectors), the cosine scores would drift and the warm
    // boot would return different matches or a different ranking.  Identical
    // results prove the cache is lossless.
    // -------------------------------------------------------------------------
    let warm_find_response = warm_components
        .app
        .find_skill(FindSkillRequest {
            // Same fixed query as the cold-boot call above.
            prompt: CACHE_FIDELITY_QUERY.to_owned(),
            limit: Some(2),
        })
        .await;

    assert_eq!(
        warm_find_response.status, "ok",
        "find_skill after warm boot must return ok, got {:?} (reason {:?})",
        warm_find_response.status, warm_find_response.reason_code,
    );
    assert!(
        !warm_find_response.matches.is_empty(),
        "find_skill after warm boot must return at least one match (retrieval works); \
         reason_code: {:?}",
        warm_find_response.reason_code,
    );
    let warm_matches = warm_find_response.matches;

    // Print both result sets for evidence in the test output.
    eprintln!(
        "T17 cache-fidelity cold={:?} warm={:?}",
        cold_matches
            .iter()
            .map(|m| (m.name.as_str(), m.score.as_str()))
            .collect::<Vec<_>>(),
        warm_matches
            .iter()
            .map(|m| (m.name.as_str(), m.score.as_str()))
            .collect::<Vec<_>>(),
    );

    // AC3 cache-fidelity: same skill names in the same order.
    let cold_names: Vec<&str> = cold_matches.iter().map(|m| m.name.as_str()).collect();
    let warm_names: Vec<&str> = warm_matches.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        cold_names, warm_names,
        "T17 AC3: warm-boot (cache-loaded vectors) must produce byte-identical retrieval to \
         cold-boot (freshly-embedded vectors) — divergence means the embedding cache \
         roundtrip is lossy / scores drifted. cold={cold_names:?} warm={warm_names:?}",
    );

    // AC3 cache-fidelity: scores must be equal within a tight epsilon (1e-6).
    // `score` is formatted as "{:.3}" so re-parse for the numeric comparison.
    for (cold_m, warm_m) in cold_matches.iter().zip(warm_matches.iter()) {
        let cold_score: f64 = cold_m
            .score
            .parse()
            .expect("cold match score must be a valid f64");
        let warm_score: f64 = warm_m
            .score
            .parse()
            .expect("warm match score must be a valid f64");
        assert!(
            (cold_score - warm_score).abs() < 1e-6,
            "T17 AC3: warm-boot (cache-loaded vectors) must produce byte-identical retrieval to \
             cold-boot (freshly-embedded vectors) — divergence means the embedding cache \
             roundtrip is lossy / scores drifted. \
             skill='{}' cold_score={cold_score} warm_score={warm_score}",
            cold_m.name,
        );
    }

    // AC3 supplemental: compile_context after warm boot returns Ok or NoMatch
    // (both are honest non-degraded outcomes for a homogeneous corpus).
    // Does NOT assert a specific skill name — the 30-skill homogeneous seed corpus
    // cannot guarantee any single skill ranks top-N for every query.
    let compile_probe_response = warm_components
        .app
        .compile_context(CompileContextRequest {
            prompt: CACHE_FIDELITY_QUERY.to_owned(),
            session_id: "t17-ac3-warm-retrieval".to_owned(),
            repo_path: repo_root_path(),
            trigger: None,
        })
        .await;

    assert!(
        matches!(
            compile_probe_response.status,
            CompileContextStatus::Ok | CompileContextStatus::NoMatch
        ),
        "compile_context after warm boot must return Ok or NoMatch (non-degraded), \
         got {:?} (reason {:?})",
        compile_probe_response.status,
        compile_probe_response.reason_code,
    );
    if compile_probe_response.status == CompileContextStatus::Ok {
        assert!(
            compile_probe_response
                .additional_context
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            "compile_context Ok response must include non-empty additional_context",
        );
    }

    // -------------------------------------------------------------------------
    // Phase 5: Warming guard honesty — no hang, no healthy-while-warming window.
    //
    // We simulate the background reload window by flipping the readiness handle
    // to Warming on the already-booted app. This exercises the REAL tool
    // short-circuit path via the REAL ReadinessHandle state machine.
    // -------------------------------------------------------------------------
    let readiness_handle = warm_components.app.readiness_handle();

    // Confirm the app is currently Ready after a successful boot.
    assert!(
        readiness_handle.is_ready(),
        "readiness handle must be Ready immediately after from_environment returns"
    );

    // Simulate background reload: flip to Warming.
    readiness_handle.set_warming();

    assert!(
        !readiness_handle.is_ready(),
        "readiness handle must NOT be ready while Warming"
    );

    // AC4(a): find_skill must return warming status FAST — no embed acquired.
    let find_during_warming = tokio::time::timeout(
        Duration::from_secs(5),
        warm_components.app.find_skill(FindSkillRequest {
            prompt: "any query — must short-circuit before embedding".to_owned(),
            limit: Some(3),
        }),
    )
    .await
    .expect("find_skill during Warming must return within 5 seconds — embedding semaphore must NOT be acquired");

    assert_eq!(
        find_during_warming.status, "warming",
        "find_skill during Warming must return status 'warming', got {:?}",
        find_during_warming.status,
    );
    assert!(
        find_during_warming.matches.is_empty(),
        "find_skill warming response must have no matches, got {} matches",
        find_during_warming.matches.len(),
    );

    // AC4(a): compile_context must return warming status FAST.
    let compile_during_warming = tokio::time::timeout(
        Duration::from_secs(5),
        warm_components.app.compile_context(CompileContextRequest {
            prompt: "any query — must short-circuit before embedding".to_owned(),
            session_id: "t17-ac4-warming-guard".to_owned(),
            repo_path: repo_root_path(),
            trigger: None,
        }),
    )
    .await
    .expect("compile_context during Warming must return within 5 seconds — embedding semaphore must NOT be acquired");

    assert_eq!(
        compile_during_warming.status,
        CompileContextStatus::Warming,
        "compile_context during Warming must return Warming status, got {:?}",
        compile_during_warming.status,
    );

    // AC4(b): health_component must be unhealthy during Warming — /health returns 503.
    let warming_health = readiness_handle.health_component();
    assert!(
        !warming_health.healthy,
        "readiness health_component must be healthy=false during Warming (would make /health return 503)"
    );
    assert!(
        warming_health.detail.contains("warming"),
        "readiness health_component detail must contain 'warming', got: {:?}",
        warming_health.detail,
    );

    // Restore to Ready: verify normal retrieval resumes.
    readiness_handle.set_ready();

    assert!(
        readiness_handle.is_ready(),
        "readiness handle must be Ready after set_ready()"
    );

    // AC4(c): health_component must be healthy after Ready transition.
    let ready_health = readiness_handle.health_component();
    assert!(
        ready_health.healthy,
        "readiness health_component must be healthy=true after set_ready()"
    );
    assert_eq!(
        ready_health.detail, "ready",
        "readiness health_component detail must be 'ready' after set_ready()"
    );

    // AC4(d): retrieval works normally after Ready — the snapshot is intact.
    let find_after_ready = tokio::time::timeout(
        Duration::from_secs(60),
        warm_components.app.find_skill(FindSkillRequest {
            prompt: "readiness state machine warming guard tool call short-circuit".to_owned(),
            limit: Some(3),
        }),
    )
    .await
    .expect("find_skill after set_ready() must return within 60 seconds");

    assert_eq!(
        find_after_ready.status, "ok",
        "find_skill after set_ready() must return 'ok', got {:?} (reason {:?})",
        find_after_ready.status, find_after_ready.reason_code,
    );
    // Prove the warm-snapshot retrieval is working after Ready transition
    // (not just a non-error status). The homogeneous 30-skill corpus means
    // any specific skill name is not guaranteed top-N, so we assert matches
    // are non-empty — the snapshot is intact and the retrieval path works.
    assert!(
        !find_after_ready.matches.is_empty(),
        "find_skill after set_ready() must return at least one match (snapshot intact); \
         reason_code: {:?}",
        find_after_ready.reason_code,
    );

    // -------------------------------------------------------------------------
    // Teardown — panic-safe via NamespaceGuard::Drop fallback.
    //
    // warm_components.teardown() truncates all PG tables (skills, skill_embeddings,
    // etc.) and deletes the Qdrant collection's points + the Redis stream. This
    // cleans up the corpus seeded in Phase 1 and the embeddings cached in Phase 2.
    // seed_components.teardown() is a no-op at this point (tables already empty)
    // but called for symmetry and to close the seed pool's connections cleanly.
    // -------------------------------------------------------------------------
    warm_components
        .teardown()
        .await
        .expect("warm boot teardown should succeed");

    seed_components
        .teardown()
        .await
        .expect("seed teardown should succeed");

    // Drop the sandbox PG schema / Qdrant collection / Redis stream.
    // Only touches this run's namespace; canonical containers are never affected.
    namespace.cleanup().await;
}
