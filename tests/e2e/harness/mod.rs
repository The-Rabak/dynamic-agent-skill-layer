/// Real-infra E2E harness — all tests drive the RUNNING containerized app.
///
/// # Principles (from `docs/reference/e2e-harness-contract.md`)
/// 1. Real app, real transport: tests call `mcp-server` over HTTP `:3001`.
/// 2. No stubs / fakes: embeddings are real Ollama; skills enter through the real ingest loop.
/// 3. White-box observation: `observe.rs` reads PG / Qdrant / Redis read-only.
/// 4. Detailed per-run, per-stage file logs: `stagelog.rs` writes JSON + Markdown.
/// 5. Local-first: `cloud_calls: none`; Ollama default.
///
/// # Including this module from a sibling test file
///
/// ```rust
/// #[path = "harness/mod.rs"]
/// mod harness;
/// ```
///
/// The path must be relative to the test file; all siblings in `tests/e2e/`
/// resolve to `harness/mod.rs` with the line above.
///
/// # Module responsibilities
/// - [`stack`]    — bring the full stack up; `kill`/`stop`/`start`/`pause`/`unpause`.
/// - [`app`]      — `McpClient` over HTTP: `compile_context`, `health`, `ingest_transcript`.
/// - [`seed`]     — sidecar volume writer/approver: `write_pending`, `approve`, `remove`, `list`.
/// - [`guard`]    — `SeededSkillGuard`: panic-safe RAII cleanup for volume-seeded skills.
/// - [`observe`]  — read-only `PgObserver`, `QdrantObserver`, `RedisObserver`.
/// - [`poll`]     — `poll_until`, `wait_for_rebuild`, `wait_for_health`.
/// - [`stagelog`] — per-run/per-stage JSON + MD logs; `E2EReport` emission.
pub mod app;
pub mod guard;
pub mod observe;
pub mod poll;
pub mod seed;
pub mod stack;
pub mod stagelog;
