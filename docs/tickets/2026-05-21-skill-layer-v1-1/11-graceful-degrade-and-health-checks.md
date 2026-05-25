---
ticket_id: T11
title: Graceful degrade and health checks
kind: hardening
status: ready
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 3.1"
feature_home: crates/infrastructure/
depends_on:
  - T08
dependency_type: hard
serves:
  - SC-7: explicit degraded behavior and service health
  - SC-1/SC-5: production-grade container runtime replaces alpine placeholders so the self-growing loop can actually execute
files:
  - crates/infrastructure/src/resilience.rs
  - crates/infrastructure/src/health.rs
  - crates/mcp-server/src/main.rs
  - crates/graph-builder/src/main.rs
  - crates/session-extractor/src/lib.rs
  - docker-compose.yml
  - Dockerfile
  - tests/integration/test_resilience.rs
test_command: cargo test --workspace && docker compose -f docker-compose.test.yml up --abort-on-container-exit
tdd_mode: inherit
---

# Graceful degrade and health checks

## Serves

- SC-7 by making infrastructure failure modes explicit, retryable, and inspectable instead of silent or crash-shaped.

## Scope

Add shared retry/backoff and circuit-breaker behavior, health endpoints, degrade guards for online paths, and Docker health-check definitions that describe the runtime honestly.

## Scope Fence

- Do not add dashboards, alerting, or metrics stacks.
- Do not collapse healthy `no_match` into degraded-empty behavior.
- Keep resilience logic reusable in infrastructure and consumed by services rather than reimplemented ad hoc.

## What to Build — Service Containerization

The plan's Research Insights §"Docker Build Optimization" (lines 228-233) defines the build pipeline but no ticket owned it until now. This ticket replaces the three alpine placeholders in `docker-compose.yml` with production-grade multi-stage Docker builds and adds filesystem volume mounts so the runtime sees real skill directories and transcript roots.

### Dockerfile — Single Shared, Multi-Binary

Create one `Dockerfile` at repo root using `cargo-chef` for build cache separation and multi-stage compilation:

- **Stage 1 (planner):** `lukemathwalker/cargo-chef:latest-rust-1` — runs `cargo chef prepare` to compute the dependency fingerprint. No source compilation.
- **Stage 2 (builder):** Same base image — runs `cargo chef cook --release` to build and cache all dependencies, then `cargo build --release --bin ${BIN}` where `BIN` is passed as a build argument. Uses `--mount=type=cache,target=/app/target/` for incremental rebuilds.
- **Stage 3 (runtime):** `alpine:3.21` — copies only the compiled binary from builder into `/usr/local/bin/${BIN}`, sets `ENV RUST_LOG=info`, exposes no ports (MCP server binds inside Docker network). Image size target: ~12MB per binary.

Technical constraints:
- **musl target** with `tls-rustls` feature for SQLx — avoids OpenSSL C dependency in Alpine.
- **`ARG BIN`** per service so one Dockerfile builds three images:
  - `BIN=mcp-server` → `skill-layer/mcp-server`
  - `BIN=graph-builder` → `skill-layer/graph-builder`
  - `BIN=maintenance-worker` → `skill-layer/maintenance-worker`
- `sqlx-offline` mode for compile-time query checking without a live database.
- No `.env` file baked into images — all configuration via Docker Compose environment variables.

### Docker Compose — Replace Placeholders with Real Services

Replace the three alpine `tail -f /dev/null` service definitions in `docker-compose.yml`:

**mcp-server:**
```yaml
mcp-server:
  build:
    context: .
    dockerfile: Dockerfile
    args:
      BIN: mcp-server
  image: skill-layer/mcp-server:latest
  container_name: ${COMPOSE_PROJECT_NAME:-skill-layer}-mcp-server
  depends_on:
    postgres:
      condition: service_healthy
    redis:
      condition: service_healthy
    qdrant:
      condition: service_started
    ollama:
      condition: service_started
  ports:
    - "${MCP_SERVER_PORT:-3001}:3001"
  environment:
    - RUST_LOG=${RUST_LOG:-info}
    - DATABASE_URL=postgres://${POSTGRES_USER:-skill_layer}:${POSTGRES_PASSWORD:-skill_layer}@postgres:5432/${POSTGRES_DB:-skill_layer}
    - REDIS_URL=redis://redis:6379
    - QDRANT_URL=http://qdrant:6334
    - OLLAMA_URL=http://ollama:11434
    - SKILL_GLOBAL_PATHS=${SKILL_GLOBAL_PATHS:-/skills/global}
    - SKILL_GLOBAL_ALLOWED_ROOTS=${SKILL_GLOBAL_ALLOWED_ROOTS:-}
    - GRAPH_BUILDER_PROJECT_ROOT=${GRAPH_BUILDER_PROJECT_ROOT:-}
    - CLAUDE_TRANSCRIPT_ROOT=${CLAUDE_TRANSCRIPT_ROOT:-/transcripts}
  volumes:
    - ${SKILL_GLOBAL_PATHS:-~/.config/opencode/skills}:/skills/global:ro
    - ${CLAUDE_TRANSCRIPT_ROOT:-~/.claude/transcripts}:/transcripts:ro
  healthcheck:
    test: ["CMD", "wget", "-qO-", "http://localhost:3001/health"]
    interval: 15s
    timeout: 5s
    retries: 3
  restart: unless-stopped
```

**graph-builder:**
```yaml
graph-builder:
  build:
    context: .
    dockerfile: Dockerfile
    args:
      BIN: graph-builder
  image: skill-layer/graph-builder:latest
  container_name: ${COMPOSE_PROJECT_NAME:-skill-layer}-graph-builder
  depends_on:
    postgres:
      condition: service_healthy
    redis:
      condition: service_healthy
    qdrant:
      condition: service_started
    ollama:
      condition: service_started
  environment:
    - RUST_LOG=${RUST_LOG:-info}
    - DATABASE_URL=postgres://${POSTGRES_USER:-skill_layer}:${POSTGRES_PASSWORD:-skill_layer}@postgres:5432/${POSTGRES_DB:-skill_layer}
    - REDIS_URL=redis://redis:6379
    - QDRANT_URL=http://qdrant:6334
    - OLLAMA_URL=http://ollama:11434
    - SKILL_GLOBAL_PATHS=${SKILL_GLOBAL_PATHS:-/skills/global}
    - GRAPH_BUILDER_PROJECT_ROOT=${GRAPH_BUILDER_PROJECT_ROOT:-/skills/project}
    - GRAPH_BUILDER_GLOBAL_ROOT=${GRAPH_BUILDER_GLOBAL_ROOT:-/skills/global}
  volumes:
    - ${GRAPH_BUILDER_PROJECT_ROOT:-.}:/skills/project:ro
    - ${SKILL_GLOBAL_PATHS:-~/.config/opencode/skills}:/skills/global:ro
  healthcheck:
    test: ["CMD", "wget", "-qO-", "http://localhost:8080/health"]
    interval: 30s
    timeout: 5s
    retries: 3
  restart: unless-stopped
```

**maintenance-worker:**
```yaml
maintenance-worker:
  build:
    context: .
    dockerfile: Dockerfile
    args:
      BIN: maintenance-worker
  image: skill-layer/maintenance-worker:latest
  container_name: ${COMPOSE_PROJECT_NAME:-skill-layer}-maintenance-worker
  depends_on:
    postgres:
      condition: service_healthy
    redis:
      condition: service_healthy
  environment:
    - RUST_LOG=${RUST_LOG:-info}
    - DATABASE_URL=postgres://${POSTGRES_USER:-skill_layer}:${POSTGRES_PASSWORD:-skill_layer}@postgres:5432/${POSTGRES_DB:-skill_layer}
    - REDIS_URL=redis://redis:6379
    - QDRANT_URL=http://qdrant:6334
    - OLLAMA_URL=http://ollama:11434
  restart: unless-stopped
```

### Volume Mounts — Scope and Transcript Configuration

The following volume mounts enable the runtime to see real skill directories:

- **`SKILL_GLOBAL_PATHS`** — Comma-separated host paths to harness skill directories. Mounted read-only into `/skills/global`. Default: `~/.config/opencode/skills:~/.claude/skills`. See `.env.example` for multi-path syntax.
- **`SKILL_GLOBAL_ALLOWED_ROOTS`** — Absolute allowlist for global scope path validation. Mirrors `SKILL_GLOBAL_PATHS` entries but as absolute container paths. Required by `EnvPathGlobalResolver`. No implicit fallback — if empty string, global scope is disabled.
- **`GRAPH_BUILDER_PROJECT_ROOT`** — Host path to the git repository root. Mounted read-only into `/skills/project`. Default: `.` (current directory).
- **`CLAUDE_TRANSCRIPT_ROOT`** — Host path to Claude Code transcript directory. Mounted read-only into `/transcripts`. Used by `extract_session` for `transcript_ref` resolution under the trust boundary.

All skill-directory volumes are mounted read-only (`:ro`) to enforce constitution §1 (local-first, no writes outside `.pending`/`.retired` patterns). The session-extractor writes `.pending` files to a separate writable path (handled inside the container via `SKILL_GLOBAL_PATHS` env var which points to a writable copy or the global scope directory if writable mounts are configured separately).

### `.env.example` Additions

Add these variables to `.env.example`:
```bash
# Service ports
MCP_SERVER_PORT=3001
GRAPH_BUILDER_PORT=8080

# Scope directory mounts (host paths)
SKILL_GLOBAL_PATHS=~/.config/opencode/skills:~/.claude/skills
SKILL_GLOBAL_ALLOWED_ROOTS=/skills/global
GRAPH_BUILDER_PROJECT_ROOT=.
CLAUDE_TRANSCRIPT_ROOT=~/.claude/transcripts

# Logging
RUST_LOG=info
```

### Docker Build Verification

A new healthcheck contract: `docker compose build` must produce three images. `docker compose up` must start all 7 services with health checks passing and no `tail -f /dev/null` fallback.

## Acceptance Criteria

- MCP server returns explicit `degraded` outcomes when dependencies are unavailable.
- Graph builder and session extractor retry transient failures with bounded backoff.
- Circuit-breaker behavior is explicit and testable.
- Services expose `/health` with dependency-level status.
- Docker Compose health checks and startup order reflect the intended runtime topology.
- `Dockerfile` builds via `cargo-chef` with `ARG BIN` producing three images: `skill-layer/mcp-server`, `skill-layer/graph-builder`, `skill-layer/maintenance-worker`. Image size <20MB per binary.
- `docker compose build` succeeds without errors using musl + `tls-rustls` (no OpenSSL dependency).
- `docker compose up` starts all 7 services — no `tail -f /dev/null` placeholders remain.
- Volume mounts for scope directories and transcript root are correctly configured in docker-compose.yml and `.env.example`.
- All service images expose `/health` endpoints that Docker health checks consume.
- `docker compose down && docker compose up` survives a clean restart cycle with health checks passing.

## Shared / Global Notes

- The degraded vs healthy-empty distinction is a frozen top-level contract.
- Resilience helpers belong in `infrastructure`; services should apply them without re-owning the logic.
- This ticket hardens existing behavior; it should not reshape feature ownership.

## Local Context

WHY link: the user story requires zero-touch behavior that fails gracefully when local services are missing or restarting; brittle failure modes would break the session-start promise.

Work across the runtime entry points and shared resilience utilities only. Important now:

- Wrap `compile_context` in a degrade guard, not a fake success path.
- Keep retry behavior explicit for graph-builder and extractor flows.
- Use the same dependency names and reason-code semantics later documented in the runbook ticket.

This ticket also owns the Docker build pipeline because: (1) it already modifies `docker-compose.yml` for healthcheck definitions, (2) health checks are meaningless without real service images to health-check, and (3) the plan's Docker Build Optimization research was assigned to no ticket until now. The three alpine placeholders from T01 finally become real container images here. By this point in the dependency chain, all three service binaries (mcp-server from T03, graph-builder from T05, maintenance-worker from T08) exist and are buildable.

Unknowns: none beyond retry threshold tuning and circuit-breaker settings. Dockerfile musl + `tls-rustls` feature gate must be verified against SQLx compile-time query checking mode.

## Parent Refs

- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- Source packet: `## Execution Slices > Slice 3.1`
- Frozen contracts: `#### compile_context result contract`, `## Seams, Adapters, and Contracts`

## Deeper-Dive Refs

- `docs/constitution.md`
- `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md#interfaces-as-test-surfaces`
- `.github/skills/workflows-to-issues/references/tdd-evidence-contract.md`

## Coupling Notes

- Retry, degrade, and health semantics stay together because they all define what "safe failure" means across the runtime.
- Splitting runtime resilience from health reporting would make operator-facing status drift from actual fallback behavior.
