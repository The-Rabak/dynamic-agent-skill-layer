# Contributing to Dynamic Agent Skill Layer

Thank you for contributing. This document covers development setup, testing, and crate structure.

## Development Setup

### Prerequisites

- Rust 1.85+ with `cargo`
- Docker + Docker Compose
- `just` or `make` (optional, for task running)

### Clone and Build

```bash
git clone <repo-url>
cd dynamic-agent-skill-layer

# Build all crates
cargo build --workspace

# Run all tests
cargo test --workspace
```

### Environment Configuration

Copy the example environment file:

```bash
cp .env.example .env
```

Key variables for local development:

```bash
# Service ports
MCP_SERVER_PORT=3001
GRAPH_BUILDER_PORT=8080

# Scope directories (host paths)
SKILL_GLOBAL_HOST_PATH=./docs
GRAPH_BUILDER_PROJECT_ROOT=.
GRAPH_BUILDER_GLOBAL_ROOT=./docs
CLAUDE_TRANSCRIPT_ROOT=./tests/fixtures

# Required for graph-builder to start
GRAPH_BUILDER_ALLOW_SYNTHETIC_OUTBOX_DRAIN=1

# Logging
RUST_LOG=info
```

## Crate Structure

Nine crates with strict dependency direction:

```
domain ← infrastructure ← service crates
         ↑
    retrieval, compiler, graph-builder, maintenance, admin, session-extractor, mcp-server
```

### Dependency Rules

1. **`domain`** has ZERO infrastructure dependencies. Verify with:
   ```bash
   cargo tree -p domain --depth 1
   ```
   Should show only `std`/`core` deps (plus `serde`, `thiserror`, `async-trait` for traits).

2. **Service crates** never import `reqwest`, `sqlx`, or `redis` directly. Always use `infrastructure` re-exports.

3. **`mcp-server`** tool handlers are thin delegations. No business logic in transport code.

## Testing

### Test Levels

| Level | Command | Purpose |
|-------|---------|---------|
| Unit | `cargo test --workspace` | Pure logic, mock dependencies |
| Integration | `cargo test --test test_compile_context` | Seeded graphs, deterministic embeddings |
| E2E | `docker compose -f docker-compose.test.yml up --abort-on-container-exit` | Container topology verification |
| Benchmark | `cargo bench` | Latency evidence for compile_context |

### Running Specific Tests

```bash
# Compile context integration tests
cargo test -p mcp-server --test test_compile_context

# Dual scope retrieval tests
cargo test -p mcp-server --test test_dual_scope

# Resilience behavior tests
cargo test -p mcp-server --test test_resilience

# Watcher rebuild tests
cargo test -p graph-builder --test test_watcher_rebuild
```

### Benchmarks

```bash
# Run compile_context benchmark
cargo bench --bench compile_context_bench

# Results are written to target/criterion/
```

The benchmark uses a mock embedding service (no network calls) to isolate retrieval + compilation latency. Target: <500ms p95 at 5K skills.

## Code Conventions

### Rust

- Edition: 2024
- Format: `rustfmt` (run `cargo fmt`)
- Lint: Clippy strict (run `cargo clippy --workspace -- -D warnings`)
- Async: `tokio` with `async-trait` for trait methods

### Naming

- Traits in `domain`: `*Service`, `*Resolver`, `*Compiler`
- Concrete impls in `infrastructure`: `Ollama*Service`, `*Resolver` + `GitRootProjectResolver`, `EnvPathGlobalResolver`
- Events: `domain.event_name` (e.g., `skill.file_changed`)
- Reason codes: `snake_case` with domain prefix (e.g., `embedding_provider_unavailable`)

### Logging

All service binaries call `init_service_logging` or `init_logging` at startup:

```rust
use infrastructure::logging::{ServiceLoggingConfig, init_service_logging};

init_service_logging(ServiceLoggingConfig::new(
    "mcp-server",
    env!("CARGO_PKG_VERSION"),
    environment,
    "info",
))?;
```

Logs are structured JSON to stdout. Use `tracing::info!`, `tracing::warn!`, `tracing::error!` with structured fields:

```rust
tracing::info!(
    service = "graph-builder",
    graph_version = outcome.graph_version,
    "graph rebuilt"
);
```

## Docker Development

### Build Images

```bash
docker compose build
```

Builds three service images from a single Dockerfile using `cargo-chef`:
- `skill-layer/mcp-server`
- `skill-layer/graph-builder`
- `skill-layer/maintenance-worker`

### Run Stack

```bash
docker compose up -d
```

### View Logs

```bash
docker compose logs -f mcp-server
docker compose logs -f graph-builder
```

### Restart Service

```bash
docker compose restart mcp-server
```

## Adding a New MCP Tool

1. **Add handler in `mcp-server/src/tools/`**: Implement request/response types and tool logic
2. **Register in `mcp-server/src/protocol.rs`**: Add to tool descriptor list and routing
3. **Add tests in `tests/integration/`**: Verify with seeded graph
4. **Update `docs/reference/capability-catalog.md`**: Document contract

## Architecture Decisions

Major architectural decisions are recorded in:

- [`docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`](docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md) — canonical V1.1 architecture
- [`docs/constitution.md`](docs/constitution.md) — project principles and guardrails

Before proposing changes that cross crate boundaries or modify event contracts, review these documents.

## Submitting Changes

1. Run the full validation command:
   ```bash
   cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit && cargo bench
   ```

2. Ensure `cargo fmt` and `cargo clippy --workspace -- -D warnings` pass

3. Update relevant documentation in `docs/reference/` or `docs/runbooks/`

4. Verify no constitution principles are violated without explicit waiver

## Questions?

- Architecture deep-dive: [`docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`](docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md)
- Runtime state meanings: [`docs/runbooks/degraded-state.md`](docs/runbooks/degraded-state.md)
- Tool contracts: [`docs/reference/capability-catalog.md`](docs/reference/capability-catalog.md)
