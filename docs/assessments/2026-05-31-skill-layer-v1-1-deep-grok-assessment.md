---
date: 2026-05-31
topic: skill-layer-v1-1
assessor: deep-grok
status: complete
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
prior_assessment_ref: docs/assessments/2026-05-28-skill-layer-v1-1-deep-grok-assessment.md
constitution_ref: docs/constitution.md
tickets_ref: docs/tickets/2026-05-21-skill-layer-v1-1/index.md
v2_plan_ref: docs/plans/2026-05-26-feat-skill-layer-v2-plan.md
scope:
  crates: 9 (domain, infrastructure, mcp-server, retrieval, compiler, graph-builder, maintenance, admin, session-extractor)
  pg_tables: 12
  event_catalog: 8 events
  containers: 6
  tickets_completed: T01-T15 (T15a + T15b)
  tickets_truth_gapped: "online read-path: mcp-server binary serves empty in-memory graph; build_live_server is test-only"
  dream_contracts_total: 24
  dream_contracts_live_red_bodies: 5 (DS-003..DS-007)
  dream_contracts_provable_today: 4 (DS-001, DS-009, DS-011, DS-013)
  dream_contracts_stubbed: 15
  test_attributes_total: ~225
  build: green
handoff:
  purpose: true
  assessment: true
  recommendations: true
---
# V1.1 Current-State Grok Assessment (2026-05-31)

## Executive Verdict
**Score: 82% — "The loop closes on the bench, not yet in the body."**

Three days ago this was 78% ("spine forged, nerves still threading"). Since then the work crossed a real
threshold: the P0 that defined the prior gap — *"the outbox relay isn't wired into the graph-builder
runtime"* — is resolved. `graph-builder/src/main.rs:187` now constructs a real `PostgresDurableGraphState`,
and every rebuild calls `mark_outbox_drained()` which invokes a real `OutboxRelay` that drains the PG
`outbox` table into Qdrant by correlation ID. The synthetic-drain flag is now a *test-only* opt-in behind an
explicit guard (`rebuild.rs:174`), not a production crutch. T14 (extraction prompt unification), T15a (the
`build_live_server()` live-infra harness + report schema), and T15b (12 live-infra RED tests, DS-003–007
promoted from panic-stubs to real bodies) all landed.

But grokking the relay closely surfaced the new headline risk. **The two halves of the system are not
connected in the shipped binaries.** The offline writer (graph-builder) now correctly drains to PG+Qdrant.
The online reader — the actual deployed `mcp-server` — boots with `SeededGraph::new(Vec::new(), 0)`
(`mcp-server/src/main.rs:22`): an *empty, in-memory, version-0 graph*. The real-infra wiring
(`build_live_server`, which plugs `compile_context` into live PG/Qdrant retrieval) exists **only under
`#[cfg(test-utils)]` in the E2E suite**. So today: the loop is provable end-to-end *on the test bench*, the
write-nerve drains to the vector store, but the production online organism reads from an empty memory it
shares with nothing. In a real `docker compose up`, `compile_context` would return `no_match` forever,
because the server it ships isn't looking at the graph the builder writes.

That's the whole story of 82%: we went from "can't prove it takes a punch" to "the punch-test rig is built
and the write-nerve is connected" — but the rig hasn't been run green, and the deployed body still isn't
plugged into itself.

## What Changed Since 78% (May 28 → May 31)

| Then (78%) | Now (82%) |
|---|---|
| T07 outbox relay **not wired** at runtime — `SYNTHETIC_OUTBOX_DRAIN=1` masking the gap (the "single biggest risk to T14") | Relay **wired** — `PostgresDurableGraphState` in graph-builder main, real `OutboxRelay.drain_correlation_outbox()` per rebuild; synthetic flag demoted to test-only guard |
| Session-extractor was 54 lines of scaffolding | Grew ~9× to ~2,100 lines (lib 499, transcripts 294) — but `providers/claude.rs` and `providers/ollama.rs` are still **27 lines each** |
| T14, T15 pending | T14 done (prompt unification), T15a done (live harness factory + roundtrip), T15b done (stress/resilience RED suite) |
| 24 dream contracts, all panic-stubs | DS-003–007 promoted to **real live-infra test bodies**; DS-001/002, DS-008–024 still `pending_contract` panics |
| "No fault-injection harness exists" | `docker compose stop/start` chaos matrix is live in `test_dream_state_contract.rs`; `build_live_server` + report builder exist |
| 96 tests passing | ~225 test attributes total (incl. ignored live/dream); 12 new live tests are **RED-phase only — written to fail, not yet proven green** |

## Score Matrix
*(same dimensions/weighting as prior assessments; one creative dimension added)*

| Dimension | Score | Δ | Posture |
|-----------|-------|-----|---------|
| Architectural integrity | **9.5** | 0 | Relay wiring + harness were purely additive. Nothing torn down across 15 tickets. |
| Filesystem-as-UI | **8.5** | 0 | Pending/retired/tombstone lifecycle intact. |
| Retrieval quality (code) | **8.0** | 0 | MMR→RRF, scoring, dual-scope solid *as code*. (Deployment disconnect scored separately below.) |
| Lifecycle governance | **8.0** | 0 | Human-gate contract intact, write boundaries hardened. |
| Graceful degrade & resilience | **7.5** | +1.0 | Relay wired; DS-003 chaos matrix is now executable, not aspirational. |
| Trust boundary safety | **7.5** | +0.5 | 11-issue review batch landed P1 security fixes; transcript-root + 127.0.0.1 lock hold. DS-010 still stubbed. |
| Performance/SLO | **7.0** | +0.5 | DS-007 high-QPS is now a runnable live test; still single-shot benchmark for green proof. |
| Multi-tenant isolation | **7.0** | 0 | DS-008 still a panic-stub, not even promoted to live. |
| Observability | **6.0** | 0 | Reason codes + JSON logs flow; DS-011/022 trace-graph still stubbed. |
| Causal traceability | **5.0** | +0.5 | Correlation IDs now genuinely thread the relay drain; no trace graph yet. |
| Self-healing autonomy | **4.0** | 0 | V2 territory (DS-014). |
| Learning & improvement loops | **3.0** | 0 | V2 territory (DS-024). Zero code. |
| **★ Loop-closure / deployment truth** (new) | **5.0** | new | Loop is provable in E2E; **shipped binaries aren't connected**. Online server reads an empty graph. This is the gap between "correct in the lab" and "works on `docker compose up`." |

**Weighted composite ≈ 6.7 → 82%.** *(Architectural integrity + retrieval carry 2×; learning loops +
self-healing carry 0.5×. The new loop-closure dimension carries 1× and is what holds the score back from the
high-80s.)*

## What's Working Excellently
1. **The relay is real now.** `OutboxRelay::new(...).drain_correlation_outbox(...)` reads the PG outbox and
   pushes vector upserts to Qdrant, invoked on every rebuild. The dual-write consistency contract the plan
   obsessed over is no longer a primitive-on-a-shelf — it's in the runtime path.
2. **The architecture absorbed everything additively.** 15 tickets, relay wiring, a whole live-test harness,
   and the domain crate still has 4 dependencies. The central bet — *freeze contracts and enforce boundaries
   with the compiler so every addition is additive* — is still paying off. Nothing was refactored to add the
   relay.
3. **The dream-state suite is becoming executable.** DS-003 (chaos), DS-004 (outbox replay), DS-005 (drift),
   DS-006 (saturation), DS-007 (QPS) now have real bodies that stop/start live containers and assert degraded
   semantics. The endgame is no longer 24 panics — it's 5 runnable contracts and 19 stubs.
4. **The trust contract held under load.** Four explicit `compile_context` statuses, suppression only after
   healthy outcomes, version-keyed cache invalidation — all survived the relay and harness work unchanged.

## What's Dangerous (the Creative Gaps)
1. **★ The deployed system isn't plugged into itself.** `mcp-server/main.rs:22` ships an empty in-memory
   graph. `build_live_server()` — the thing that wires online retrieval to live PG/Qdrant — is test-only.
   This is *the* gap. It's likely a small change (swap `build_seeded_server` for the live builder in main,
   wire the env config), but until it's done, the offline writer and online reader are two systems that only
   meet in the test harness. A naïve `docker compose up` demo would show `no_match` on every prompt.
2. **RED ≠ GREEN.** T15b's STATE.md is explicit: *"RED PHASE ONLY... NO implementation fixes allowed."* The
   12 live tests + DS-003–007 were written to compile and **fail**. We have the *contracts that prove
   trustworthiness* but not yet the *green runs that demonstrate it*. The 82% counts the rig being built; it
   does not count a passing live suite.
3. **The extraction providers are still thin.** `claude.rs` and `ollama.rs` are 27 lines each. Session-extractor
   grew, but the provider implementations that DS-012 (parity) and the entire V2 quality story depend on are
   still close to scaffolding. The self-growth half of "self-growing skill layer" is the least-proven half.
4. **Multi-tenant isolation (DS-008) hasn't even been promoted to a live test.** For a system whose V2 endgame
   is cross-repo collective intelligence (DS-017), the cross-tenant leakage contract is still a panic-stub.
   That's the right V1.1 priority, but it's the soft underbelly of the team-scope future.

## Where We're Going — Reading the Endgame from the Dream-State Tests
The 24 contracts are the real product spec, and they stratify cleanly into three horizons.

- **V1.1 "trustworthy" band (DS-001–013):** deterministic full loop, transport parity, chaos resilience,
  outbox durability, drift reconciliation, saturation, QPS SLO, multi-repo isolation, restart persistence,
  hostile-input safety, observability, provider parity, lifecycle SLA. **This is what "done" means for V1.1.**
  Status: ~5 have live bodies (RED), 4 are arguably provable today (deterministic loop, restart persistence,
  observability, pending-lifecycle), the rest are stubs. The gating prerequisite for *all* of them going green
  is closing gap #1 (connect the online binary) and gap #2 (run the suite green).

- **V2 "intelligent" band (DS-014, 016, 018, 020, 024):** autonomous self-healing, policy-native governance,
  counterfactual explainability, SLO-aware orchestration, outcome-based learning. The V2 plan maps these 1:1
  to SkillLens/SkillOpt research — quality-scored extraction, validation gates, text-space skill optimization.
  This is where the system stops *accumulating* skills and starts *getting smarter*. The architecture has left
  the seams (`ContextCompiler`, `ScopeResolver`, `MergeSemanticVerifier` traits) — additive fills, not rewrites.

- **V2/V3 "platform" band (DS-015, 017, 021, 022, 023):** time-travel replay, cross-repo collective
  intelligence, shadow deployment, end-to-end causal tracing, deterministic twin. This is the team-platform
  vision — and it's where the current 5.0 causal-traceability and 4.0 self-healing scores say there's the most
  distance to travel.

The shape of the ambition is genuinely impressive: not "a RAG over markdown files." The endgame is **a
self-evolving, policy-governed, causally-traceable skill substrate that proves its own correctness via
deterministic replay and improves itself from outcome feedback** — and the dream-state tests encode that as
executable contracts *before the features exist*. That's the most disciplined version of "write the spec as
failing tests" at this scale.

## Bottom Line
This remains a bet, and the bet is still winning: 15 tickets, a wired relay, a live-test harness, 9 clean
crates, zero boundary violations, and an endgame defined as 24 ignored tests rather than a wishlist. The
architecture *is* the product; the code keeps proving the architecture absorbs change.

The 18% that remains is now sharply located:
- **+4–5 pts** the moment the online binary reads the live graph (`main.rs` swap to live wiring) and the T15b
  suite is run **green** against containers — converting "correct in the lab" to "works on deploy."
- **+remaining** is genuinely V2: the extraction-quality and learning loops, correctly deferred, zero code.

**Clear line of sight to 88% once the loop closes in the body, not just on the bench.**
