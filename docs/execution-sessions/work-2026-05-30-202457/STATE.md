---
source_type: ticket
ticket_file: docs/tickets/2026-05-21-skill-layer-v1-1/15b-stress-resilience-and-edge-case-suite.md
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
source_packet_ref: "## Execution Slices > Slice 3.1, 3.2"
brainstorm_ref: docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md
started: 2026-05-30T20:24:57Z
status: in_progress
execution_shape: vertical-slices
current_unit: 6
total_units: 6
status: completed
session_id: work-2026-05-30-202457
review_mode: bulk
---
## WHY Context

### Problem Narrative
Developer using multiple coding agent harnesses faces triple compound cost: manual skill selection wastes 5-10 min per task, skill libraries rot unused, each harness operates in a silo. The autonomous loop is only trustworthy if the complete live path behaves correctly under realistic dependency conditions, load, and failure -- not only in happy-path or partially mocked slices.

### User Story
As a solo developer using multiple coding agent harnesses, I need a zero-touch, self-growing skill context layer that searches both project-local and global machine-wide skill scopes concurrently, merges via weighted RRF+MMR, and at session start compiles relevant skills into task-specific compact context.

### Architectural Context
Nine Rust crates (domain, infrastructure, mcp-server, retrieval, compiler, graph-builder, maintenance, admin, session-extractor) deployed via Docker Compose with PG, Qdrant, Redis, Ollama. MCP protocol is harness boundary. T15a delivered build_live_server() harness, report infrastructure, and one roundtrip test. T15b consumes that harness for full coverage multiplication.

### Success Criteria
- SC-1: Zero-touch context injection <500ms under concurrent load and during active rebuild
- SC-4: PG-to-Qdrant durability and replay validated as one data-plane flow under stress
- SC-7: Degraded semantics stay explicit under dependency loss and recovery

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: RED PHASE ONLY -- write tests, compile, run, expect failures, report
- Required evidence: Compilation + detailed failure report
- Exceptions: None. NO implementation fixes allowed.
- **Iron rule: LIVE INFRASTRUCTURE ONLY.** Zero mocks, zero stubs, zero fakes, zero deterministic embedding services.

### Constitution Context
- Version: 1.0.0
- All 5 principles apply
- Waivers: none

### Architecture Handoff
- Artifact: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
- Feature homes: tests/ (this ticket creates tests only)
- Canonical V1.1 contracts frozen

## Work Status
| # | Unit | Kind | Serves | Status | Attempts | Session File |
|---|------|------|--------|--------|----------|--------------|
| 1 | Extraction live E2E tests (inline + ref + pending/event flow) | hardening | SC-3 coverage | completed | 1 | unit-01-stress-resilience-suite.md |
| 2 | Degraded/recovery E2E test (dependency chaos) | hardening | SC-7 coverage | completed | 1 | unit-01-stress-resilience-suite.md |
| 3 | Watcher churn/reconciliation live PG+Qdrant | hardening | SC-4 coverage | completed | 1 | unit-01-stress-resilience-suite.md |
| 4 | Concurrency stress tests (compile + rebuild + extract) | hardening | SC-1 coverage | completed | 1 | unit-01-stress-resilience-suite.md |
| 5 | Dream-state contract promotions DS-003 through DS-007 | hardening | Dream-state coverage | completed | 1 | unit-01-stress-resilience-suite.md |
| 6 | Report aggregation + judge contract + runner script | hardening | Cross-test validation | completed | 1 | unit-01-stress-resilience-suite.md |

## Learnings Brief
- [testing] 12 live-infra tests added across 4 files. All consume `build_live_server()`. All marked `#[ignore = "requires live containers"]`. Compilation clean with `--features test-utils`.
- [testing] `LiveServerComponents::teardown()` gated behind `test-utils` feature. Requires `required-features = ["test-utils"]` in Cargo.toml test entries.
- [testing] `Arc<ConcreteType>` doesn't auto-coerce to `Arc<dyn Trait>`. Use `&impl Trait` parameters or `.as_ref()`.
- [testing] Ollama extraction: env vars `EXTRACT_SESSION_PROVIDER=ollama`, `OLLAMA_EXTRACTION_MODEL=granite4:3b` wire real extraction.
- [testing] Docker compose stop/start in tests: `Command::new("docker").args(["compose", "-f", compose_path, "stop", service])`. Sleep 2-8s between operations.
- [testing] `std::thread::sleep` not `tokio::time::sleep` for wall-clock waits during Docker operations.
- [build] `graph-builder` test needs dev-deps on `mcp-server` (+test-utils) and `retrieval` for watcher churn live test.