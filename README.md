# Dynamic Agent Skill Layer

A local-first, self-growing skill context layer that automatically compiles task-relevant skills for coding agent harnesses.

## What This Does

- **Zero-touch context injection:** At session start, `compile_context` searches project-local and global machine-wide skill scopes concurrently, merges results via weighted RRF+MMR, and compiles structured markdown for your agent harness — all in under 500ms.
- **Self-growing:** After each session, `extract_session` analyzes transcripts and proposes new skills as `.pending` drafts for human approval.
- **Offline maintenance:** `graph-builder` watches the filesystem and rebuilds the skill graph incrementally. `maintenance-worker` runs periodic merge/retire policy passes.
- **Portable:** Skills are plain `SKILL.md` files with YAML frontmatter. Move them between projects and harnesses without conversion.

## Quickstart

### Prerequisites

- Docker + Docker Compose
- Rust 1.85+ (for local development)

### Stand Up the Stack

```bash
# 1. Copy environment template
cp .env.example .env

# 2. Build all service images
docker compose build

# 3. Start infrastructure + services
docker compose up -d

# 4. Verify health checks
docker compose ps
```

The MCP server exposes tools on `http://127.0.0.1:3001`. Health endpoints:
- MCP server: `http://127.0.0.1:3001/health`
- Graph builder: `http://127.0.0.1:8080/health`

### Run Tests

```bash
# Unit + integration tests
cargo test --workspace

# Integration tests with containers
docker compose -f docker-compose.test.yml up --abort-on-container-exit

# Benchmarks
cargo bench
```

## Architecture

Nine Rust crates with explicit feature homes:

```
crates/
├── domain/           # Pure domain: types, traits, config (ZERO infra deps)
├── infrastructure/   # Concrete impls: Ollama clients, PG pool, Redis, resilience, logging
├── retrieval/        # Retrieval pipeline: Qdrant search, PG graph, scoring, MMR+RRF
├── compiler/         # Context compilation: template, rescue, formatting
├── mcp-server/       # MCP transport: bootstrap, tool handlers, session state
├── graph-builder/    # Offline graph construction: watcher, extraction, embeddings, HDBSCAN
├── maintenance/      # Policy workflows: merge detection, retirement, cron trigger
├── admin/            # Online admin/debug MCP tools
└── session-extractor/ # Post-session: transcript analysis, skill extraction, .pending files
```

Deep reference: [`docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`](docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md)

## Key Contracts

- **`compile_context` result:** `ok`, `no_match`, `degraded`, `duplicate_suppressed`
- **Transcript ingress:** `transcript_ref` rooted under mounted `CLAUDE_TRANSCRIPT_ROOT`
- **Human gate:** All mutations produce `.pending` or `.retired` proposals — never auto-apply
- **Event catalog:** `skill.file_changed`, `skill.extraction_requested`, `extraction.completed`, `extraction.failed`, `graph.rebuilt`, `graph.rebuild_failed`, `skill.retired`, `skill.merged`

See [`docs/reference/capability-catalog.md`](docs/reference/capability-catalog.md) for tool contracts and [`docs/runbooks/degraded-state.md`](docs/runbooks/degraded-state.md) for runtime state meanings.

## Project Principles

Defined in [`docs/constitution.md`](docs/constitution.md):

1. **Local-first execution** — all services run on your machine via Docker Compose
2. **Zero-touch session start** — context injection requires no manual skill selection
3. **Human gate for mutations** — `.pending` drafts require human rename-to-approve
4. **Portable scope** — skills move between projects and harnesses without conversion
5. **Filesystem-observable state** — all graph mutations are visible as filesystem changes

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for development setup, testing conventions, and crate structure overview.

## License

MIT
