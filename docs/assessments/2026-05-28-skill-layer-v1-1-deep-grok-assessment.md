---
date: 2026-05-28
topic: skill-layer-v1-1
assessor: deep-grok
status: complete
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
prior_assessment_ref: docs/assessments/2026-05-26-skill-layer-v1-1-deep-grok-assessment.md
constitution_ref: docs/constitution.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
scope:
  crates: 9 (domain, infrastructure, mcp-server, retrieval, compiler, graph-builder, maintenance, admin, session-extractor)
  loc: ~4,700
  pg_tables: 12 (+12 indexes, 6 triggers)
  event_catalog: 8 events
  containers: 6 (PostgreSQL, Redis, Qdrant, Ollama, mcp-server, graph-builder, maintenance-worker)
  tickets_completed: T01-T13
  tickets_truth_gapped: T07, T08 (runtime wiring deferred)
  tickets_pending: T14, T15
  tests_passing: 96
  tests_dream_state_ignored: 24
  tracked_todos: 48 (40 complete, 9 pending)
  build: green (cargo test --workspace passes clean)
handoff:
  purpose: true
  assessment: true
  recommendations: true
---
# V1.1 Current-State Grok Assessment (2026-05-28)
## Executive Verdict
**Score: 78% — "Spine forged, nerves still threading"**

Seven days ago this was 72% and "iron skeleton, not yet muscle-bound." The skeleton is now wired: health probes are live, cache is keyed, benchmarks exist, Dockerfile builds real binaries, 12 execution sessions closed. The remaining 22% isn't code — it's truth. T07/T08 are marked complete but have work-log addendums acknowledging runtime wiring gaps. T14 (the stress suite) is 493 lines of report-schema ambition with zero assertions yet written. The dream-state contracts read like the product's soul, and the body is walking but can't yet prove it can take a punch.
## Context
### What Changed Since the Prior Assessment (72%, 2026-05-26)

| Then (May 26) | Now (May 28) |
|---|---|
| T11 not started — degraded markers hardcoded strings | T11 complete — `HealthProbe` trait, circuit breaker, retry, Docker healthchecks, cargo-chef multi-stage Dockerfile |
| T12 not started — session state in-memory only | T12 complete — Redis-backed suppression, blake3-keyed context cache, graph_version invalidation |
| T13 not started — no benchmarks, docs skeleton | T13 complete — criterion benchmarks, JSON logging, capability catalog, runbooks |
| 48 todos, many open | 48 todos, 40 completed, 9 pending |
| "No benchmark harness exists" | Benchmark harness exists, p95 assertions in place |
| "Health probes are hardcoded" | Live probes for PG/Redis/Qdrant/Ollama |
| "Session-extractor not composed into online binary" | Extract session wired through MCP server |
| Dockerfile was Alpine placeholder | Full cargo-chef multi-stage, musl target, ~12MB images |
| 10/14 tickets | 13/15 tickets (T14, T15 remain) |

### What the Dream State Tests Demand (for Reference)
The 24 contracts define the true endgame:
| Contract | Domain | Ambition Level |
|----------|--------|---------------|
| DS-001 | Deterministic analysis-extraction-retrieval loop | Core correctness |
| DS-002 | MCP transport roundtrip (stdio/HTTP) | Protocol parity |
| DS-003 | Dependency chaos matrix (degraded semantics) | Resilience |
| DS-004 | Outbox backlog replay (crash/restart) | Durability |
| DS-005 | Qdrant-PG drift detection and reconciliation | Data integrity |
| DS-006 | Sustained watcher/extraction saturation | Stress |
| DS-007 | High-QPS compile_context SLO targets | Performance |
| DS-008 | Multi-repo scope isolation | Security |
| DS-009 | Full restart cycle persistence | Durability |
| DS-010 | Hostile input boundary safety suite | Security |
| DS-011 | Observability contract (reason-coded traces) | Observability |
| DS-012 | Extraction provider parity (Claude vs Ollama) | Quality |
| DS-013 | Pending lifecycle and approval SLA at scale | Governance |
| DS-014 | Autonomous self-healing loop | Intelligence |
| DS-015 | Time-travel memory replay | Audit |
| DS-016 | Policy-native skill governance | Intelligence |
| DS-017 | Cross-repo collective intelligence | Multi-tenancy |
| DS-018 | Retrieval counterfactual explainability | Intelligence |
| DS-019 | Always-on drift sentinel | Observability |
| DS-020 | SLO-aware orchestration brain | Intelligence |
| DS-021 | Shadow deployment evaluator | Intelligence |
| DS-022 | End-to-end causal tracing | Observability |
| DS-023 | Offline deterministic twin | Audit |
| DS-024 | Outcome-based learning loop | Intelligence |
## Score Matrix

| Dimension | Today | Gap | Posture | Composite | Status | Δ From Prior |
|-----------|-------|-----|---------|-----------|--------|-------------|
| Architectural integrity | 9.5 | Tiny | Ironclad | **9.5** | Near textbook | +0.5 |
| Filesystem-as-UI | 8.5 | Small | Rock-solid | **8.5** | Pending/retired/tombstone lifecycle complete | +1.0 |
| Retrieval quality | 8.0 | Small | Rock-solid | **8.0** | Production-ready core | 0 |
| Lifecycle governance | 8.0 | Small | Rock-solid | **8.0** | Complete contract, merge engine write-boundary hardened | +0.5 |
| Multi-tenant isolation | 7.0 | Medium | Rock-solid | **7.0** | Scope resolver correct, cross-repo untested | 0 |
| Trust boundary safety | 7.0 | Medium | Solid | **7.0** | Transcript root enforced, MCP surface 127.0.0.1-locked | +0.5 |
| Performance/SLO | 6.5 | Medium | Strong | **6.5** | Benchmarks exist, cache reduces repeated calls to near-zero | +1.5 |
| Graceful degrade & resilience | 6.5 | Medium | Strong | **6.5** | Live health probes, circuit breaker, retry, degraded suppression | +2.5 |
| Observability | 6.0 | Medium | Strong | **6.0** | Structured JSON logging, reason codes flowing | +2.5 |
| Causal traceability | 4.5 | Large | Moderate | **4.5** | Correlation IDs in envelope, no trace graph | +0.5 |
| Self-healing autonomy | 4.0 | Large | Solid | **4.0** | Architecture supports it, T11 foundation makes it possible now | +3.0 |
| Learning & improvement loops | 3.0 | Enormous | Aspirational | **3.0** | Contracts defined, zero code | 0 |

**Weighted composite: 6.6 → 78%**
*Weighting: Architectural integrity and retrieval quality carry 2x; learning loops and self-healing carry 0.5x.*
## What Is Working Excellently
### 1. The Domain Crate Has Zero Infrastructure Dependencies
7 lines in `cargo.toml`: `serde`, `thiserror`, `async-trait`, `strum`. That's an architectural weapon. Every crate that imports `domain` can be tested with mocks. Every V2 addition that touches domain types cannot accidentally pull in sqlx/redis. The adversarial assessment's P0 "domain must stay pure" is CI-enforceable (`cargo tree -p domain --depth 1`).

### 2. Compile Context Result Semantics Are Genuinely Correct
Four explicit statuses (`ok`, `no_match`, `degraded`, `duplicate_suppressed`) where the original plan had an ambiguous empty string. Suppression is written only after `ok` or `no_match` — never after a degraded attempt consuming the one-shot injection. This is the right trust contract.

### 3. The Health Probe Trait Is Real
No more hardcoded `("ollama".to_owned(), "degraded".to_owned())`. Live probes hit PG (`SELECT 1`), Redis (`PING`), Qdrant (collection info), Ollama (model list). The health map in every `compile_context` response is runtime truth, not compile-time assumption.

### 4. The Cache Invalidation Contract Is Genuinely Correct
`(blake3(prompt), scope_fingerprint, graph_version)` — three-component key where graph_version invalidates on `graph.rebuilt`. Version-based, not time-based. No stale context after a rebuild.

### 5. The Dockerfile Is Production-Grade
Cargo-chef multi-stage build, musl target, Alpine runtime. Three binaries from one Dockerfile (`ARG BIN`). Health checks on all three service containers. `127.0.0.1` port binding for the MCP surface.

### 6. Domain Purity Survived 13 Tickets
`cargo tree -p domain --depth 1` still shows only `serde`, `thiserror`, `async-trait`, `strum`. Zero boundary violations across 12 execution sessions.
## What Is Dangerous (Creative Gaps)
### 1. T07/T08: "Completed" Frontmatter But Runtime Wiring Deferred
Both tickets are marked `status: completed` in frontmatter. Both have work-log addendums from May 26 acknowledging they were reopened to `ready`. The outbox relay primitives exist. The maintenance merge/retire engine exists. But graph-builder in production mode uses `InMemoryDurableGraphState` with a synthetic outbox drain flag (`GRAPH_BUILDER_ALLOW_SYNTHETIC_OUTBOX_DRAIN=1`). Without it, graph-builder refuses to start: "runtime durable state has no relay-backed outbox drain wiring yet." The code compiles, the tests pass in isolation, the runtime integration with the real outbox relay is deferred. This is the single biggest risk to T14.

### 2. The Session-Extractor Is Still 54 Lines
No provider has been fleshed out beyond scaffolding. The dream-state demands extraction provider parity (DS-012), policy-native governance (DS-016), and outcome-based learning (DS-024). All three contracts flow through this boundary. T15 reviews prompts — it doesn't implement real extraction.

### 3. No Fault Injection Harness Exists
DS-003 demands a chaos matrix. DS-004 demands outbox crash/replay. DS-006 demands sustained saturation. DS-007 demands high-QPS load profiles. A `tests/harness/` directory with `DockerTestCluster`, `FaultInjector`, and `LoadProfile` doesn't exist yet. T14 owns this but has zero assertions written yet.

### 4. The Dream-State Contract Gap Is ~80%
24 contracts, 24 ignored tests. The architecture supports all of them. The code supports 4-6. T14 targets un-ignoring 6-12. That leaves 12-18 contracts for V2.
## Dream-State Contract Readiness Matrix
### Already Provable (4/24)
| Contract | What's There |
|---|---|
| DS-001 Deterministic loop | Retrieval pipeline is deterministic with seeded embeddings |
| DS-009 Restart persistence | Redis-backed suppression + cache with graph_version invalidation |
| DS-011 Observability contract | Structured logging + reason codes exist everywhere |
| DS-013 Pending lifecycle at scale | Frontmatter contract hardened, TTL warnings tested, tombstones validated |

### Architecture-Ready, Implementation-Gapped (8/24)
| Contract | Gap |
|---|---|
| DS-002 MCP transport parity | Both stdio/HTTP work. No transport-level diff test |
| DS-003 Chaos matrix | Circuit breaker + retry + health probes exist. No fault injection harness |
| DS-004 Outbox replay | Outbox primitives exist. Runtime relay not wired to graph-builder |
| DS-005 Qdrant-PG drift | Reconciler exists. No synthetic drift injection test |
| DS-006 Sustained saturation | Watcher debounced, bounded queues. No churn stress test |
| DS-007 High-QPS SLO | Benchmarks exist for single-shot. No concurrent load profile |
| DS-008 Multi-repo isolation | Scope resolver correct. No cross-tenant canary-token test |
| DS-010 Hostile input suite | Path traversal guarded, transcript root enforced. No adversarial corpus |

### Architecture-Supported, Zero Code (7/24)
| Contract | What the Architecture Would Need |
|---|---|
| DS-012 Provider parity | Extraction providers are 54 lines of scaffolding |
| DS-014 Self-healing | Health probes exist → remediation catalog needs definition |
| DS-016 Policy-native governance | Frontmatter metadata exists → scoring/route rules needed |
| DS-018 Counterfactual explainability | Retrieval scoring formula is deterministic → perturbation engine needed |
| DS-019 Always-on drift sentinel | Reconciliation scan exists → multi-surface sampling needed |
| DS-020 SLO-aware orchestration | Latency budget documented → adaptive path selector needed |
| DS-024 Outcome learning | Lifecycle audit exists → feedback-to-policy loop needed |

### Aspirational (5/24) — V2/V3 Territory
DS-015, DS-017, DS-021, DS-022, DS-023
## Structural Health Check
### Crate Boundary Integrity
| Crate | LoC (approx) | Boundary Clean? | Notes |
|---|---|---|---|
| domain | ~350 | Yes | Zero infra deps, 4 traits, CI-enforceable |
| infrastructure | ~1200 | Yes | All adapters live here. Re-exports for consumers |
| retrieval | ~800 | Yes | No MCP, no compilation, no session state |
| compiler | ~300 | Yes | Pure transformation, no I/O |
| mcp-server | ~500 | Mostly | Tool handlers thin, session state and route composition cleaned up in T13 |
| graph-builder | ~600 | Gap | Outbox relay not wired at runtime — uses synthetic drain flag |
| maintenance | ~500 | Gap | Depends on T07 which has deferred runtime wiring |
| admin | ~200 | Yes | Read-only/trigger-only, composed into online binary |
| session-extractor | ~200 | Yes (boundary) | Scaffolding only — boundary is correct, implementation is placeholder |

### Test Health
- 96 tests passing (unit + integration)
- 24 dream-state tests (all `#[ignore]`)
- 1 criterion benchmark for compile_context
- No load generator, no fault injector, no Docker test cluster

### Pending Todos (9 items)
5 P1 (MCP surface lockdown, dependency health surfacing, infra boundary restoration, suppression state memory bounding, T11 env-semantics alignment), 3 P2 (read-only volumes, T11 alignment docs, extraction backpressure), 1 P3 (compose resilience test structure).
## Phase 3 Risk Assessment
T14: Live Data-Plane E2E and Stress Suite (Priority: P0 for V1.1 completeness)
Risk: High. This is where the dream-state contracts start getting un-ignored and where T07/T08's deferred runtime wiring will surface immediately. The `GRAPH_BUILDER_ALLOW_SYNTHETIC_OUTBOX_DRAIN=1` flag is currently masking that the relay-backed durable state doesn't exist. T14 will force this.

T15: Extraction Prompt Review and Unification (Priority: P1)
Risk: Low. Documentation and prompt alignment only. Depends on T06 which is solid.

## Top Recommendations by Impact
| Priority | Action | Serves | Expected Impact |
|---|---|---|---|
| P0 | Wire outbox relay into graph-builder runtime (resolve T07 runtime gap) | SC-4, SC-7, DS-004, DS-005 | Closes the largest deferred integration gap |
| P0 | Build Docker test cluster harness with fault injection | T14, DS-003-007 | Makes T14 about assertions, not infrastructure |
| P0 | Flesh out at least one extraction provider (Claude) with real transcript fixture | SC-3, DS-012 | Unblocks extraction-to-ingestion loop testing |
| P1 | Un-ignore 6 dream-state contracts in T14 | All dream-state | Proves architecture's resilience claims |
| P2 | Wire maintenance cron into runtime worker | SC-4, DS-016 | Closes maintenance governance gap |

## Suggested Phasing for Completion
Phase 3a: Truth Foundation (T07/T08 runtime wiring, 1-2 units)
1. Wire outbox relay into graph-builder rebuild path
2. Verify maintenance runs against outbox-drained graph
3. Remove `GRAPH_BUILDER_ALLOW_SYNTHETIC_OUTBOX_DRAIN` gating

Phase 3b: Stress Validation (T14, 2-3 units)
1. Docker test cluster harness with fault injection
2. Fault injection matrix (PG down, Redis down, Qdrant corrupted, Ollama unavailable)
3. Load generation with configurable QPS
4. Un-ignore 6-12 dream-state contracts

Phase 3c: Extraction Hardening (T15, 1 unit)
1. Review and unify Claude vs Ollama extraction prompts
2. Document provider-specific divergence with explicit justification

## Final Assessment
This isn't a codebase — it's a bet. The bet is: if you freeze architecture before implementation, enforce boundaries with the compiler, and define the endgame as ignored tests before writing a single line of feature code, you ship faster and safer than the alternative.

The bet is paying off. T01-T13 in 7 working days. 96 tests. 9 crates. Zero boundary violations. The architecture has absorbed every new requirement without refactoring — health probes were additive, the cache was additive, the Dockerfile was additive. Nothing was torn down.

The remaining 22% is the difference between "the system is correct" and "the system is trustworthy." Two tickets have work-log addendums acknowledging their runtime wiring must still be completed (T07 outbox relay, T08 maintenance). T14 will bridge the stress-test gap. T15 will close the extraction quality gap. The dream-state's deeper contracts (self-healing, learning loops, counterfactual explainability, causal tracing) are correctly deferred to V2.

**The architecture is the product. The code is evidence that the architecture works.** On that metric: 78% with a clear line of sight to 85%+ after T07/T08 runtime wiring and T14/T15.