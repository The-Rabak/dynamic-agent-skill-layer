---
date: 2026-06-02
topic: skill-layer-v1-5-current-state
assessor: opencode-grok
status: complete
constitution_ref: docs/constitution.md
v1_1_plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
v1_5_plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
v2_plan_ref: docs/plans/2026-05-26-feat-skill-layer-v2-plan-deepened.md
v1_1_architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
v1_5_architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
v2_architecture_ref: docs/architecture/2026-05-26-skill-layer-v2-architecture.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
dream_state_ref: tests/e2e/test_dream_state_contract.rs
scope:
  assessed_through: v1.5 T08 pending boundary
  v1_1_tickets: T01-T15a-T15b reviewed via tickets and execution records
  v1_5_tickets_completed: T01-T06 plus T07 superseded by todo 103
  v1_5_next_gate: T08, T09, T10
handoff:
  purpose: true
  assessment: true
  recommendations: true
---

# Skill Layer V1.5 Current-State Assessment

## Verdict

Score now: **84% current-state truth, 91% trajectory, 76% ship-confidence until live suite green.**

Creative label: **"Body has a heartbeat now; nervous system still fails final reflex test."**

V1.1 at prior assessment was 82% because the loop worked in lab but the deployed binary was not plugged into the graph. V1.5 work moved the system over a real threshold: the deployed server now boots the live graph by default, the refresh subscriber exists, the usage loop exists, extraction got serious, and the transcript-ingest queue fixes a real Claude Code path issue. This is no longer just a skeleton.

But T08/T09/T10 remain the gate. The live suite is still not proven green. The current branch also has governance/provider drift around the `claude-code` extraction path. So I would not call this ready. I would call it **architecturally correct, materially close, not yet trustworthy by its own standard.**

## Project In One Sentence

Dynamic Agent Skill Layer is becoming a **local-first, human-gated, self-growing skill substrate for coding agents**: it watches and builds a SkillRAE-style skill graph, injects task-relevant skill context automatically, extracts new skills from sessions, records usage, retires/merges stale knowledge, then later evolves into team-scope, quality-scored, explainable, self-optimizing agent skill infrastructure.

Not "RAG over markdown." More like **agent operating-system memory discipline**, but scoped to procedural skills instead of chat memory.

## What This Product Wants To Be

Endgame from plans, architecture, and dream tests:

- Agent starts task.
- System retrieves project/global/team skills automatically.
- Context is compiled into exact harness format.
- Agent works with better priors.
- Transcript is captured automatically at `PreCompact` and `SessionEnd`.
- Extractor proposes new skills as `.pending`.
- Human approves by filesystem rename.
- Graph rebuilds.
- Running server hot-swaps graph without restart.
- Usage rows show what skills matter.
- Maintenance retires, merges, and later optimizes skills.
- V2 quality layer blocks bad skills before pollution.
- Team scope shares useful knowledge without tenant leakage.
- Explainability shows why a skill won.
- Shadow/twin/replay system proves no silent regressions.
- Outcome learning improves policy over time without auto-approval.

That is a serious product. Big ambition, but unusually well-fenced. Best thing here: dream tests encode future as contracts, not vibes.

## Scope Read

V1.1 scope:

- 9-crate Rust workspace.
- Local Docker Compose stack.
- Domain/infrastructure/retrieval/compiler/mcp-server/graph-builder/maintenance/admin/session-extractor split.
- MCP `compile_context`, `find_skill`, `extract_session`, admin tools.
- Dual-scope retrieval, template compile, watcher rebuild, extraction drafts, merge/retire, outbox, health, cache, logging, benchmark, live/stress E2E bodies.

V1.5 scope:

- Not new product surface.
- "Make V1.1 true when deployed."
- T01: production boots live graph.
- T02: graph refreshes on `graph.rebuilt`.
- T03: ratify Option A, Qdrant write-side CQRS, honest health.
- T04: Claude Code lifecycle hooks.
- T05: real extraction reliability/provider cleanup.
- T06: usage writes plus retirement plus deterministic prior.
- T07: superseded by durable transcript-ingest queue from todo 103.
- T08: Qdrant port handling plus suppression isolation plus body limit.
- T09: retrieval quality/source paths/fixture corpus.
- T10: live suite green plus CI gate.

Current repo says T01-T06 done, T07 folded/resolved, T08 still ready/pending in ticket docs. Code confirms T08 not complete.

## Current Reality

Strong landings:

- `crates/mcp-server/src/main.rs` now defaults to `MCP_RETRIEVAL_MODE=live` and calls `McpServerApp::from_environment`.
- `RetrievalSnapshot` replaced old seeded graph semantics.
- `RetrievalOrchestrator` now uses `ArcSwap<GraphSnapshot>` for atomic graph+version swap.
- `graph_refresh_subscriber.rs` consumes `graph.rebuilt`, reloads PG snapshot, swaps read model, ACKs only after reload success.
- Option A is coherent now: read path = in-memory PG snapshot, Qdrant = write-side vector store. Health markers stopped lying about Qdrant/Postgres/Redis read dependencies.
- Lifecycle hooks gained `trigger`, compaction bypass support, and SessionStart/PreCompact/SessionEnd contract.
- Transcript path bug was correctly rethought. Shipping absolute host paths into a container was wrong. Durable content-ingest queue is better architecture than patching path validation.
- T05 worker pool redesign is directionally right: MPMC receiver, single terminal-event owner, no-pool timeout, Ollama default, Anthropic API path.
- T06 usage loop is good: bounded background writer, prompt hash, append-log rows, transaction, real retirement usage, deterministic prior.
- V2 seam discipline remains good: usage/adapters stay in infrastructure; prior math stays in retrieval; maintenance does not import mcp-server.

Weak / unfinished:

- T08 not done: Qdrant adapter still just takes one REST endpoint string; no REST/gRPC derivation; `protocol.rs` has no `DefaultBodyLimit`; `suppression_state.clear_session` still uses `suppression:{session}` prefix against local keys shaped `{session}::{repo_path}`, so local clear is broken; hot-path suppression still checks Redis before DashMap.
- T09 not done: `build_graph_from_pg` still uses scope-root fallback because DB lacks real `skills.source_paths`; ticket explicitly says this is stand-in, not final provenance.
- T10 not done: dream/live tests still contain old assumptions. DS-003 and degraded E2E assert Qdrant stop means `Degraded`, contradicting Option A. They must be rewritten to "read unaffected, write-side health degraded."
- Current dirty branch has provider governance drift. `claude-code` CLI provider was added after the ratified "no CLI subprocess" decision. Owner decided to keep and document, but paper trail is not complete.
- `provider=claude` routing currently contradicts the constitution until #116/#119 realign it back to Anthropic API.
- Model defaults changed without human-gate record; owner decided to keep, but docs/approval record is pending.
- `claude_code` JSON brace parser has a P1 candidate-loss bug.
- Subprocess env/path/security hardening is pending.
- Sanitization parity across providers is pending.
- Some live tests are too lenient: they accept failed extraction events where product promise needs `.pending`.

## Dream-State Interpretation

Dream tests are the real product map. They split into three bands.

### Band 1: Trustworthy Core, DS-001..013

This is "V1.5/V1.1 done means done."

- Deterministic loop.
- MCP transport parity.
- Dependency chaos.
- Outbox replay.
- PG/Qdrant drift.
- Watcher/extraction saturation.
- High-QPS SLO.
- Multi-repo isolation.
- Restart persistence.
- Hostile input safety.
- Observability.
- Provider parity.
- Pending lifecycle SLA.

Status: core code has many pieces; executable proofs remain incomplete. DS-003..007 have bodies, but some assertions are stale versus Option A. DS-008/010/012 are still major trust surfaces.

### Band 2: Intelligence Layer, DS-014/016/018/020/024

This is V2 "system gets smarter."

- Self-healing.
- Policy-native governance.
- Counterfactual retrieval explainability.
- SLO-aware orchestration.
- Outcome-based learning.

Status: architecture good, code mostly absent. Correctly deferred. Do not pull into V1.5.

### Band 3: Platform/Audit Layer, DS-015/017/021/022/023

This is "team/product platform."

- Time-travel replay.
- Cross-repo collective intelligence.
- Shadow deployment.
- End-to-end causal trace.
- Offline deterministic twin.

Status: future moat. Current foundations support it, but causal traceability and tenant isolation are not yet deep enough.

## Score Matrix

| Dimension | Score | Why |
|---|---:|---|
| Architectural Integrity | 9.0 | 9-crate split still right. `domain`/`retrieval` purity mostly preserved. Provider governance drift dents score, not core structure. |
| Loop Closure / Deployment Truth | 8.2 | Empty production graph fixed; refresh path exists. Still missing T08/T09/T10 live proof. |
| Self-Growth Trigger | 8.0 | Hook path improved, transcript queue is strong. Gap: no-hook-fired crash gap accepted, queue drain proof is gated live infra. |
| Extraction Reliability | 7.0 | Worker pool/provider work much stronger. But CLI parser P1, provider routing contradiction, sanitization parity, subprocess hardening still open. |
| Retrieval Quality | 7.6 | Algorithm solid, thresholds improved. Real `source_paths` provenance and fixture corpus still pending. |
| Filesystem Human Gate | 8.7 | `.pending`/`.retired` philosophy holds. Queue still outputs `.pending`; no auto-approval. |
| Usage / Maintenance Loop | 8.0 | T06 is strong: append-log, bounded writer, retirement read, deterministic prior. Some scope heuristic and hot-path polish remain. |
| Local-First Compliance | 7.2 | Default path local. Cloud/API/CLI provider semantics need cleanup and docs. Model/config human-gate misses hurt. |
| Live Proof / E2E Truth | 6.2 | Many tests exist. But `--include-dream` not green, DS-003 stale, T08/T09/T10 pending. |
| Observability / Causal Trace | 6.5 | Reason codes/events/logs exist. Full causal trace graph not there; usage-write health added. |
| Security / Trust Boundary | 7.0 | Transcript queue avoids path bug; localhost + secret added. Open body limit, provider sanitization, subprocess env/path gaps. |
| Performance/SLO | 7.6 | In-memory snapshot and ArcSwap are good. Usage off-path. Need live p95 under full suite plus no eager Qdrant read checks. |
| V2 Readiness | 8.5 | Seams are mostly right. V2 can add team scope/quality/explainability without rewrite if current drift is reconciled. |
| Dream Intelligence | 4.5 | Mostly designed, not built. Correct for phase. |

Composite: **84%**.

## Honest Read

Project is doing one rare thing right: it treats **correctness of the loop** as product surface. Not UI, not dashboard, not "agent magic." The actual feature is: when an agent needs skill context, the right skill appears; when a session teaches something, the skill graph grows; when graph changes, the live server sees it; when a skill stops helping, the system proposes cleanup; every mutation is human-gated and auditable.

That is coherent.

Architecture has real taste:

- Small pure domain.
- Retrieval not polluted with infrastructure.
- Compiler separate from retrieval.
- Maintenance not in graph-builder.
- Filesystem is approval UI.
- Qdrant/PG outbox discipline.
- Version-keyed cache.
- Explicit `ok/no_match/degraded/duplicate_suppressed`.
- Human gate never silently bypassed.

But project also shows classic ambitious-system danger:

- Docs/plans/tickets are so detailed they can drift from code fast.
- Post-ticket "good idea" commits reversed ratified decisions.
- Tests are sometimes contract-shaped but lenient enough to pass while product behavior is not proven.
- Many ignored live tests give confidence architecture is aimed right, not that behavior works.
- T08/T09/T10 are integration truth gates. Without them, score ceiling should stay under 90 no matter how nice code looks.

## Biggest Strategic Strength

You are building **trust infrastructure before intelligence infrastructure**.

That matters. Most agent-memory projects jump to "learning loop" too early. This repo instead says:

- status semantics first,
- event envelope first,
- graph versioning first,
- filesystem human gate first,
- live suite first,
- then SkillLens/SkillOpt/team scope.

That ordering is correct. V2 becomes additive because V1.5 is forcing body truth.

## Biggest Strategic Risk

Governance drift. Not architecture drift.

The codebase is not collapsing into spaghetti. But decisions are being silently superseded by later commits. The `claude-code` provider is the prime example: probably useful, code probably solid, but it reversed a researched "no CLI subprocess" decision without ADR/constitution/ticket update. That is exactly how this project could lose its superpower: not by bad Rust, by losing traceability.

If this remains disciplined, the project can reach 90+. If this becomes "plans say one thing, code another, todos explain why," trust will erode.

## Where It Is Going

Near-term after T08/T09/T10:

- `docker compose up` should deliver real context.
- Approved skill should become retrievable without restart.
- Session transcript capture should reach `.pending`.
- Usage data should affect retirement/prior.
- Live suite should stop being aspirational and become regression gate.

Post-V1.5:

- System should be around **90-92% V1 trust complete**.
- Remaining work becomes actual V2 intelligence, not V1 wiring debt.

V2 direction:

- Quality-scored extraction matters because SkillLens says 25% of generated skills can harm.
- Behavioral gates matter because LLM judges are weak.
- Team scope matters only after isolation/canary tests exist.
- SkillOpt should be proposal-only, never auto-apply.
- Counterfactual explanation should stay deterministic, no LLM explanation hallucination.
- Multi-harness likely easier than expected because AgentSkills convergence means Claude/OpenCode/Copilot mostly share `SKILL.md`; hard part is utility across consumer models, not formatting.

## Top Risks

1. **Live suite not green**

   Until `./scripts/run-e2e-tests.sh --include-dream` passes, product truth remains unproven. T10 is not optional ceremony. It is release gate.

2. **T08 not done**

   Port mismatch, suppression leak, no body limit. These are boring but block trust. Also T08 owns infra config human-gate, which is already under pressure from undocumented config changes.

3. **T09 source-path provenance**

   Scope-root fallback is acceptable bridge, not final. Real source paths matter for correct scope matching, future isolation, explainability, and audit.

4. **Provider contract drift**

   Resolve #115/#116/#117/#118/#120/#121/#122 before calling extraction reliable. Current provider story is powerful but not contract-stable.

5. **Tests accepting weaker behavior than product promise**

   A live extraction test that accepts either `completed` or `failed` is a smoke test, not proof of self-growth. The transcript queue E2E is better because it demands `.pending`.

6. **Causal trace still shallow**

   Correlation IDs exist, but DS-022 is not there. As system grows, debugging without causal lineage will hurt.

## Recommended Next Moves

1. Finish T08 exactly, no scope creep.
2. Finish T09 with `skills.source_paths` migration and shared retrieval corpus.
3. Rewrite DS-003 and degraded E2Es to Option A CQRS semantics.
4. Run T10 until `--include-dream` is green or explicitly log any deterministic deferral.
5. Close provider/governance P1s before final V1.5 assessment.
6. Produce a new assessment after T10, not before. That will be real score jump.

## Final Assessment

This repo is unusually serious. It has a clear product soul and strong architecture. V1.5 is exactly the right bridge: stop expanding, make deployed truth match contract truth.

Current state: **not shippable yet, but no longer speculative.** Core organism is connected. Remaining work is proof, polish, and governance cleanup.

If T08/T09/T10 land clean and provider drift gets reconciled, expected band:

- **Architecture:** 9.2/10
- **V1 product truth:** 8.8/10
- **V2 readiness:** 9.0/10
- **Overall:** **90-92%**

Today: **84%**. Good trajectory. Still needs final reflex test.
