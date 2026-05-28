---
date: 2026-05-26
topic: skill-layer-v2
status: active
plan_ref: docs/plans/2026-05-26-feat-skill-layer-v2-plan.md
architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
research_inputs:
  - SkillLens (arXiv:2605.23899) — quality rubric, map-reduce extraction
  - SkillOpt (arXiv:2605.23904) — text-space optimization loop
  - SkillRAE (arXiv:2605.10114) — scoring formula foundation
reviewers:
  - architecture-strategist
  - uncle-bob
  - performance-oracle
  - security-sentinel
handoff:
  deepen_plan: true
  work: true
  review: true
---

# Dynamic Agent Skill Layer V2 — Canonical Architecture

## Purpose Linkage

- **Problem Narrative:** V1.1 delivers a working skeleton: skills extracted, stored, retrieved, maintained. But three gaps prevent real utility: extraction is blind (no quality signal), the system is solo-only (team knowledge locked in silos), and maintenance is passive (deletes stale, never improves). SkillLens proved 25% of LLM-generated skills cause negative transfer and that quality dimensions predict utility. SkillOpt proved skills can be optimized without model fine-tuning (+12.8-24.9pp across 7 models). V1.1's architecture deliberately left seams for these additions.

- **User Story:** Solo developer now on a team needs quality-scored extraction, shared team scope, self-improving skills, counterfactual explanations, and multi-harness portability — all on top of V1.1's working skeleton.

- **Success Criteria this artifact protects:**
  - SC-V2-1: Quality-scored extraction (SkillLens 3-dimension rubric)
  - SC-V2-2: Team scope retrieval with cross-tenant isolation
  - SC-V2-3: Utility-scored maintenance (quality-aware retirement)
  - SC-V2-4: Outcome-based learning from approval/usage signals
  - SC-V2-5: Autonomous self-healing for 7 cataloged degradations
  - SC-V2-6: Counterfactual explainability (feature contributions + perturbations)
  - SC-V2-7: Multi-harness compilation (Claude Code, OpenCode, Copilot, Codex)
  - SC-V2-8: LLM-guided context compilation behind ContextCompiler trait
  - SC-V2-9: SkillOpt optimization loop producing `.optimized` proposals
  - SC-V2-10: ≥ 12 of 24 dream-state contracts un-ignored and passing
  - **Preserved from V1.1:** SC-1 through SC-8 remain intact. V2 adds, never changes.

- **Architectural Context:** Eleven Rust crates (9 from V1.1 + 2 new), one new Docker Compose service (`skill-optimizer`), 4 new event types, additive PG schema migration (002_quality_intelligence + 003_team_scope). All V1.1 contracts (compile_context result semantics, transcript ingress, state invalidation, event catalog, scope persistence) are preserved unchanged. V2 widens the existing seams without cutting new ones.

## Feature Homes and Ownership

### Post-V2 crate structure

```
crates/
├── domain/           # +QualityScores, +CounterfactualExplanation, +TeamScope, +HarnessFormat, +HealthProbe trait, +Remediation
├── infrastructure/   # +RemoteTeamScopeResolver, +RemoteEmbeddingService, +HealthProbes (PG/Redis/Qdrant/Ollama), +CorrelationId propagation
├── retrieval/        # +Team scope concurrent search, +Counterfactual perturbation inputs
├── compiler/         # +OllamaGuidanceCompiler, +OpenCodeCompiler, +CopilotCompiler, +CodexCompiler
├── mcp-server/       # +explain_ranking, +trigger_optimization, +get_skill_quality tools, +harness config routing
├── session-extractor/ # Map-reduce extraction, quality rubric prepend, self-assessment parsing
├── graph-builder/    # +Team index construction, +Cross-tenant isolation, +Provision hashing
├── maintenance/      # +Utility-scored retirement, +Held-out validation gates, +Outcome-based learning, +Drift sentinel, +Self-healing orchestrator
├── admin/            # +list_team_scopes, +inspect_quality_scores, +trigger_self_heal, +trace_event
├── explainability/   # NEW: Counterfactual perturbation, feature contribution scoring, rationale generation, historical replay, deterministic twin
└── skill-optimizer/  # NEW service crate: SkillOpt loop (rollout, reflect, edit, gate, deploy)
```

### Feature-home ownership (V2 additions, V1.1 unchanged)

- **Feature home: `crates/domain/`** (extended)
  - V2 owns NEW: `QualityScores` type (failure_mechanism_score, actionable_specificity_score, high_risk_avoidance_score, combined_utility_score: f32); `CounterfactualExplanation` type (ranked_skills, sensitivity, threshold_distances); `TeamScope` config; `HarnessFormat` enum (Claude, OpenCode, Copilot, Codex); `HealthProbe` trait; `Remediation` type (id, action, reason_code, is_safe, rollback); `CorrelationId` newtype (UUIDv7 wrapper)
  - Crosses into: nothing — zero infrastructure dependencies preserved
  - Notes: V1.1 domain purity is V2's anchor. No sqlx/qdrant-client/redis deps. CI gate `cargo tree -p domain --depth 1` unchanged.

- **Feature home: `crates/infrastructure/`** (extended)
  - V2 owns NEW: `RemoteTeamScopeResolver` (implements `ScopeResolver` — config-driven, connects to remote PG+Qdrant); `RemoteEmbeddingService` (implements `EmbeddingService` — OpenAI-compatible API); `PostgresHealthProbe`, `RedisHealthProbe`, `QdrantHealthProbe`, `OllamaHealthProbe` (all implement `HealthProbe`); `CorrelationId` propagation utilities; `QualityRubricLoader` (reads rubric from packaged file)
  - Crosses into: `domain` (traits and types, unchanged)
  - Notes: All V2 adapters behind V1.1 traits. No new trait definitions in infrastructure — only in domain.

- **Feature home: `crates/retrieval/`** (extended)
  - V2 owns NEW: Team scope in `search_scopes_concurrently` (already handles N scopes, team is the third); team scope weight in `RetrievalConfig`; team scope Qdrant collection search
  - Crosses into: `domain`, `infrastructure`
  - Notes: V1.1 dual-scope → V2 N-scope. Same MMR+RRF fusion. Team scope is a new input, not a new algorithm.

- **Feature home: `crates/compiler/`** (extended)
  - V2 owns NEW: `OllamaGuidanceCompiler` (implements `ContextCompiler` — LLM-synthesized guidance, 3s timeout); `OpenCodeCompiler`, `CopilotCompiler`, `CodexCompiler` (all implement `ContextCompiler` — harness-specific formatting)
  - Crosses into: `domain` (types, ContextCompiler trait)
  - Notes: Five compiler implementations behind one trait. Harness selection is config toggle. TemplateOnlyCompiler unchanged from V1.1.

- **Feature home: `crates/mcp-server/`** (extended)
  - V2 owns NEW: `explain_ranking` tool handler (delegates to explainability crate); `trigger_optimization` tool handler (delegates to skill-optimizer); `get_skill_quality` tool handler (queries quality_scores from admin); harness config routing for compiler selection; correlation_id generation per invocation
  - Crosses into: `domain`, `infrastructure`, `retrieval`, `compiler`, `session-extractor`, `explainability`
  - Notes: Thin transport adapter — tool handlers delegate, never implement. V1.1 delegation pattern extended.

- **Feature home: `crates/session-extractor/`** (rewritten internals)
  - V2 owns NEW: Map phase (per-trajectory ModeSet extraction); Reduce phase (hierarchical merge → tool-calling synthesis); Quality rubric prepend (reads `meta_skills/quality_rubric.md`); Quality self-assessment parsing; `SkillStore` integration (create/update/blacklist skill operations)
  - Crosses into: `domain`, `infrastructure`
  - Notes: Trait boundary (`TranscriptSkillExtractionService::extract`) unchanged. Single-pass extraction kept as config-gated fallback. The internal pipeline is completely rewritten behind the same interface.

- **Feature home: `crates/graph-builder/`** (extended)
  - V2 owns NEW: Team index construction (writes to remote Qdrant collection on promotion events); Cross-tenant isolation (provenance hashing, origin repo tagging); Team scope event consumption (`skill.promoted_to_team`)
  - Crosses into: `domain`, `infrastructure`
  - Notes: Watcher unchanged for project/global scopes. Team scope index is rebuild-on-event, not watched — team scope has no filesystem.

- **Feature home: `crates/maintenance/`** (extended)
  - V2 owns NEW: Utility-scored retirement (usage_score × 0.6 + quality_score × 0.4); Held-out validation gates (test proposed merge against validation transcripts); Outcome-based learning orchestrator (collect signals → tune thresholds → sandbox → deploy); Drift sentinel (5 check types: PG↔Qdrant, vector↔content, filesystem↔graph, behavioral canary, lifecycle metadata); Self-healing orchestrator (detect → select → execute → verify → audit)
  - Crosses into: `domain`, `infrastructure`, `graph-builder`
  - Notes: Maintenance crate absorbs policy-heavy V2 workloads. Graph-builder remains pure construction. Admin remains read-only. This crate is where the system *thinks* about its own state.

- **Feature home: `crates/admin/`** (extended)
  - V2 owns NEW: `list_team_scopes` tool; `inspect_quality_scores` tool (extended from V1.1 inspect_skill); `trigger_self_heal` tool (manually invoke any remediation); `trace_event` tool (causal chain query by correlation_id)
  - Crosses into: `domain`, `infrastructure`, `graph-builder`, `maintenance`
  - Notes: Read-only + trigger-only. V1.1 boundary preserved. All mutation authority stays in maintenance (policy) and graph-builder (construction).

- **Feature home: `crates/explainability/`** (NEW)
  - V2 owns: `CounterfactualExplanation` engine (feature contribution scoring via Shapley-style ablation, perturbation sensitivity search, ranked rationale generation); `HistoricalReplay` engine (graph snapshot loading, deterministic context replay); `DeterministicTwin` mode (frozen clocks, fixed embeddings, seeded randomness); `CausalTrace` query (correlation_id → full event lineage)
  - Crosses into: `domain` (types, traits), `infrastructure` (PG for graph snapshots, audit_log for trace query)
  - Notes: Pure computation crate. No I/O beyond PG reads for snapshots. No compilation. No retrieval mutations. Testable with fixture scored skills and known graph states. Rationale: separate reason to change from retrieval (explain vs retrieve vs compile — three different failure modes, three different test surfaces).

- **Feature home: `crates/skill-optimizer/`** (NEW — service crate)
  - V2 owns: Rollout engine (replay skill against held-out session transcripts); Reflection engine (analyze success/failure minibatches); Edit engine (propose bounded add/delete/replace operations with budget enforcement); Validation gate (accept only if held-out score improves); `.optimized` file output
  - Crosses into: `domain` (types), `infrastructure` (PG for session_logs, Ollama for optimizer model)
  - Notes: Separate Docker Compose service. Runs on cron trigger or manual `trigger_optimization` MCP tool. Does not modify active skills — produces proposals. Constitution §3 (human gate) enforced: `.optimized` files require rename-to-approve. Rationale: optimization is a long-running batch process (minutes per epoch × 4 epochs × N skills). It does not belong in the online request path.

## Shared / Global Decisions (V2 additions)

| Candidate | Keep in feature home / Move to shared | Why |
|-----------|----------------------------------------|-----|
| `QualityScores` type | `domain` (shared) | Used by session-extractor (producer), infrastructure/PG (storage), maintenance (retirement scoring), admin (inspection), compiler (guidance prioritization). Stable vocabulary — should live in domain. |
| `CounterfactualExplanation` type | `domain` (shared) | Output contract for `explain_ranking` MCP tool. Used by explainability crate (producer) and mcp-server (transport). Trait-worthy but simple struct — domain is fine. |
| `HealthProbe` trait | `domain` (shared) | Used by infrastructure (implementations), mcp-server (startup probe), maintenance (self-healing detection), admin (inspection). Stable interface — `check() → HealthStatus`. |
| `CorrelationId` newtype | `domain` (shared) | Used by every service that publishes events or calls tools. UUIDv7 wrapper with trace context. Making it a domain type ensures all crates use the same propagation contract. |
| `HarnessFormat` enum | `domain` (shared) | Used by compiler (implementations) and mcp-server (config routing). Stable enum — adding a harness is a new variant, not a new trait. |
| Quality rubric file | `session-extractor` (packaged) + `domain` (reference) | The rubric is a markdown file prepended to extraction prompts. It lives in `session-extractor/src/meta_skills/`. The `domain` config references the file path. Not a shared code artifact — shared configuration content. |
| PG schema migrations (002, 003) | `infrastructure` (shared) | V2 migrations are additive (`ADD COLUMN`, `CREATE TABLE`). Same migration runner as V1.1. Applied once, idempotent. |
| Remote PG/Qdrant connections | `infrastructure` (shared) | Team scope resolver and team index builder both need remote connections. Single connection pool config per service. Not shared pools across services — per-service instances like V1.1. |
| Self-healing remediation catalog | `maintenance` (feature home) | The catalog maps reason_codes → remediation actions. It's maintenance policy, not infrastructure or admin. Changes when we add new remediation types, not when infrastructure changes. |
| SkillOpt optimizer model | `infrastructure` (shared adapter) + `skill-optimizer` (usage) | The optimizer model is an Ollama client like the embedding model. Adapter lives in infrastructure. Usage is in skill-optimizer. Same pattern as `EmbeddingService`. |
| Counterfactual perturbation engine | `explainability` (feature home) | Pure computation: takes ScoredSkill array, returns CounterfactualExplanation. No I/O. No state. Separate from retrieval to prevent the scoring formula from leaking into explanation logic. |
| Causal trace query | `explainability` (feature home) | Queries PG audit_log + Redis events by correlation_id. Cross-cutting concern but single responsibility: trace construction. Separate from admin tools (which expose traces) and from infrastructure (which stores them). |

## Canonical V2 Contracts

### New contracts (added in V2)

- **`explain_ranking` result contract:** Returns `CounterfactualExplanation` with `ranked_skills[].feature_contributions`, `sensitivity.perturbations[]`, and `sensitivity.threshold_distances`. Feature contributions must sum to within ±0.05 of the skill's final score. Perturbations must be plausible (minimal prompt changes, ≤ 5 words added/removed). Always returns `ok` — explanation is always possible (it's computed from deterministic scoring components, not LLM-generated).

- **`get_skill_quality` result contract:** Returns `QualityScores` for a given skill_id, or `not_found`. Includes `source_session_id` and `extraction_provider` for provenance. If no quality scores exist (pre-V2 skill), returns `default_quality: 0.5` with `reason_code: "pre_quality_scoring_era"`.

- **`trigger_optimization` contract:** Accepts `skill_id`. Returns immediately with `optimization_run_id`. Optimization runs asynchronously (minutes). Status queryable via `optimization_run_id` on `inspect_skill` (returns `pending|running|completed|failed`). On completion, `.optimized` file appears in skill scope directory. Event `skill.optimized` published.

- **Team scope isolation contract:** Team-scoped skills carry immutable `provenance_hash` computed from `blake3(content + origin_repo + promoted_at)`. Retrieval strips tenant-specific paths from team scope results. Merge proposals must share origin repo — cross-origin merges are BLOCKED. Team scope skills never include source file paths in retrieval output.

- **Health probe contract:** Four probes run on startup and every 30s. Each probe returns `HealthStatus { dependency, status: ok|degraded|unavailable, reason_code, latency_ms }`. Health is cached for 30s for compile_context response population. Degradation detection emits `health.degraded_detected` event. Recovery emits `health.self_healed` event.

- **Self-healing contract:** Seven remediations cataloged as safe-for-auto (no skill content mutation, no filesystem write outside `.pending`/`.retired` patterns, no configuration changes). Max 3 retry attempts with exponential backoff (1s, 2s, 4s). Each remediation audited with before/after health state. Unsuccessful after 3 attempts → escalate to admin trigger (no further automatic attempts).

- **SkillOpt optimization contract:** Produces `.optimized` proposal files in skill scope directory. Format identical to SKILL.md with additional frontmatter: `optimized_from: <skill_id>`, `optimization_run_id`, `validation_score_before`, `validation_score_after`, `epochs_completed`, `optimizer_model`. Human approves by renaming to `.md` (constitution §3). Rejected optimizations are recorded in `optimization_runs` with `status: rejected` and the rejection timestamp.

- **Outcome-based learning contract:** Signal window: trailing 30 days. Signals: acceptance rate (`.pending` → `.md`), rejection rate (`.pending` → `.rejected`), usage rate (skill appears in `skill_usage` with `context_status: ok`). Minimum signal count: 30 before tuning. Sandbox validation: candidate thresholds must improve on baseline in held-out signal slice. Regression guard: threshold change that would have rejected a previously-accepted skill is BLOCKED.

- **Multi-harness compilation contract:** Five compilers behind `ContextCompiler` trait. Input: identical (`Vec<ScoredSkill>` + prompt). Output: harness-specific format. Compiler selection by config `harness` field. Format roundtrip must preserve all subunits (procedure, convention, asset counts match across harness transformations). No retrieval path changes — same scored skills, different formatting.

- **V2 event catalog contract:** V1.1's 8 events preserved. V2 adds: `skill.quality_scored`, `skill.optimized`, `skill.promoted_to_team`, `health.degraded_detected`, `health.self_healed`. Total V2 catalog: 13 events. All new events follow V1.1 envelope contract: `{event_id: UUIDv7, event_type, correlation_id, idempotency_key, schema_version, timestamp, payload}`.

### Preserved V1.1 contracts (unchanged)

- `compile_context` result contract — `ok`, `no_match`, `degraded`, `duplicate_suppressed` unchanged
- Transcript ingress contract — `transcript_ref` under mounted root, optional `transcript_inline` unchanged
- State and invalidation contract — `graph_version` bump → `graph.rebuilt`, cache invalidation by version mismatch unchanged
- Watcher reconciliation contract — startup scan + periodic reconciliation unchanged
- Scope persistence contract — scalar `scope` + `merged_from_scopes TEXT[]` unchanged (team scope is additive via junction table)
- Original event catalog — all 8 V1.1 events unchanged in schema and semantics

## Deletion Test (V2 additions)

| Candidate | Keep/Delete/Delay | Why |
|-----------|-------------------|-----|
| `explainability` as separate crate | **Keep** | Counterfactual computation is a different failure mode from retrieval. Changes when we add perturbation types or explanation formats — not when retrieval scoring changes. The V1.1 principle of separating retrieval/compilation applies identically here. |
| `skill-optimizer` as separate service | **Keep** | Optimization is a batch process (minutes per epoch). Running it in the online MCP server path would violate the < 500ms SLO. Separate service prevents this tension. If optimization load is low, it can be co-located later — but decoupling at greenfield is zero-cost. |
| `HealthProbe` trait vs inline health checks | **Keep as trait** | Four probe implementations, configurable intervals, cached results, and degradation detection logic. A trait isolates the probe contract from the probe implementations and from the consumers (mcp-server, maintenance). Inline checks would scatter health logic across 4+ crates. |
| `CorrelationId` as domain newtype | **Keep** | Used by every service that publishes events. Making it a UUIDv7 wrapper with display formatting in domain ensures all crates use identical propagation. Without it, correlation IDs become inconsistent strings. |
| Team scope as separate scope type | **Keep** | V1.1 already has `ScopeType::Team` in the enum. V2 fills it in. The resolver trait already returns `Vec<ScopeDescriptor>`. The retrieval pipeline already handles N scopes. Team scope is filling a seam, not cutting a new one. |
| `skill_scopes` junction table | **Keep (was V1.1 "Delay")** | V1.1's deletion test explicitly deferred this: "Easier to add later than remove now." That moment is now. Team scope needs many-to-many membership. Additive migration — the existing `skills.scope` column stays as the primary scope. |
| `OllamaGuidanceCompiler` as separate compiler | **Keep** | V1.1's deletion test explicitly deferred this behind `ContextCompiler` trait: "V2 adds LLM-synthesized guidance." Trait was designed for this moment. Guidance compiler is a new implementation, not a refactor. |
| Multi-harness compilers as separate implementations | **Keep** | Each harness has a different context injection format. Each format changes when the harness changes (not when retrieval changes). Five implementations behind one trait — the V1.1 pattern of `TemplateOnlyCompiler` extended. |
| Quality rubric as packaged file vs built-in prompt | **Keep as file** | The rubric is a markdown document that should be inspectable, versioned, and diffable — not a string constant buried in code. Filesystem-observable state (constitution §5) extends to the rubric that governs extraction quality. |
| `skill.quality_scored` as separate event | **Keep** | Quality scoring happens during extraction — distinct from extraction completion. Allows consumers (maintenance, learning) to react to quality data without coupling to extraction lifecycle. |
| `health.degraded_detected` and `health.self_healed` events | **Keep as pair** | Degradation and recovery are causally linked but temporally separated. Paired events allow traceability: "detected at T1, healed at T2, duration = T2-T1." A single "health_changed" event loses the pairing. |
| Remote embedding endpoint as config option | **Keep** | Teams with shared GPU infrastructure may want a centralized embedding service. Behind the same `EmbeddingService` trait as local Ollama. Opt-in config. Local Ollama remains default. |
| SkillOpt as V2 vs V3 feature | **Keep in V2** | The architecture supports it now (trait boundaries, session_logs for replay, validation gating pattern in maintenance). Deferring to V3 would mean waiting for usage data anyway — but the infrastructure should exist so that when enough data accumulates, the loop can activate without new code. |

## Interfaces as Test Surfaces (V2 additions)

- **Interface: `HealthProbe` (in `domain`)**
  - Callers/tests rely on: `check(&self) -> HealthStatus` returning `{dependency, status, reason_code, latency_ms}` and `dependency_name(&self) -> &'static str`
  - Must not leak: PG connection pool internals, Redis client details, Qdrant collection config, Ollama model name
  - Evidence needed: Mock probe returning known degraded states. Integration: real container kill → probe reflects failure. Benchmark: 4 concurrent probes complete in < 5s.

- **Interface: `CounterfactualExplanation` engine (in `explainability`)**
  - Callers/tests rely on: `explain(prompt, scored_skills) -> CounterfactualExplanation` with feature contributions summing to within ±0.05 of score
  - Must not leak: Retrieval pipeline internals, scoring formula implementation details, embedding vectors
  - Evidence needed: Known scored skill set → correct feature contributions. Perturbation search → plausible minimal changes. No explanations hallucinated — all computed deterministically.

- **Interface: SkillOpt optimizer (in `skill-optimizer`)**
  - Callers/tests rely on: `optimize(skill_id, config) -> OptimizationRunId` followed by async execution producing `.optimized` file
  - Must not leak: Optimizer model prompt engineering, validation set contents, edit budget internals
  - Evidence needed: Mock rollout → known reflect output → correct edits proposed. Integration: real skill + real transcripts → validation improvement. Gate: candidate worse than baseline → correctly rejected.

- **Interface: Team scope resolver (in `infrastructure`)**
  - Callers/tests rely on: `RemoteTeamScopeResolver::resolve()` returning team `ScopeDescriptor` when remote PG+Qdrant are available, or omitting team scope when unavailable
  - Must not leak: Remote PG connection strings, Qdrant API keys, collection naming conventions
  - Evidence needed: Mock resolver returning team scope descriptor. Integration: real remote PG+Qdrant → scope resolution succeeds with latency measurement.

- **Interface: V2 PG schema (migrations 002, 003)**
  - Callers/tests rely on: `skills.quality_scores JSONB`, `session_logs.success_ratio`, `skill_scopes` junction table, `optimization_runs`, `learning_state`, `health_events`, `graph_snapshots`
  - Must not leak: Connection pool config, migration versioning, JSONB query syntax
  - Evidence needed: Migration idempotency (run twice, no errors). V1.1 queries still work on migrated schema. New columns populated correctly through application write paths.

## Seams, Adapters, and Contracts (V2 additions)

- **Seam: Quality-scored extraction**
  - **Adapter:** Quality rubric (packaged markdown file) + map-reduce pipeline (new extraction internals)
  - **Contract:** Input = `SessionTranscript`. Output = `ExtractionResult` with `candidates[].quality_dimensions: QualityScores`. Error = `ExtractionError::ProviderUnavailable` or `ExtractionError::QualityParseFailure`. Map phase timeout = 10s per trajectory. Reduce phase timeout = 30s per merge group. Must produce quality scores for every candidate or log warning.

- **Seam: Health monitoring and self-healing**
  - **Adapter:** `HealthMonitor` (in `infrastructure`, calls probes) + `SelfHealingOrchestrator` (in `maintenance`, consumes health events)
  - **Contract:** Health probes run concurrently every 30s. Degradation detected when any probe returns `degraded` or `unavailable`. Self-healer responds within one monitor interval. Remediation bounded: 3 attempts × (1s + 2s + 4s backoff) = 21s max. Audit trail complete per remediation. Must not mutate skill content, filesystem outside `.pending`/`.retired`, or configuration.

- **Seam: Team scope retrieval**
  - **Adapter:** `RemoteTeamScopeResolver` (implements `ScopeResolver`) + team scope Qdrant collection search (extends `search_scopes_concurrently`)
  - **Contract:** Team scope runs concurrently with project+global. Timeout = 800ms (remote penalty over 400ms local). Weight in RRF = 0.5 (lower than global's 0.7). Provenance stripping applied to all team scope results before response. Must not leak source repo paths, origin developer identity, or remote connection details.

- **Seam: Counterfactual explainability**
  - **Adapter:** `CounterfactualEngine` (in `explainability`, pure computation)
  - **Contract:** Input = prompt + `Vec<ScoredSkill>`. Output = `CounterfactualExplanation`. Computation < 200ms (no I/O, no LLM). Feature contributions computed via Shapley-style ablation with 2^4 = 16 permutations (4 features: semantic, lexical, prior, community_boost). Perturbation search caps at 20 iterations. Must not modify retrieval output — explanation is read-only.

- **Seam: Skill optimization**
  - **Adapter:** `SkillOptimizer` (in `skill-optimizer`, batch service)
  - **Contract:** Input = `skill_id` + optimizer config. Output = `.optimized` file + `skill.optimized` event. Epochs = configurable (default 4). Batch size = configurable (default 10). Optimizer model = configurable (default: same as extraction provider). Validation split = configurable (default 0.2). Gate rejects worse candidates. Human approves via filesystem rename (constitution §3).

- **Seam: Outcome-based learning**
  - **Adapter:** `LearningOrchestrator` (in `maintenance`, periodic cron pass)
  - **Contract:** Signal window = 30 days. Minimum signals = 30. Candidate thresholds validated in sandbox against held-out signal slice. Regression guard blocks threshold changes that would reject previously-accepted skills. Learning state stored in `learning_state` singleton. Audit trail records every tuning decision. Must not auto-deploy without sandbox validation.

## Design-It-Twice Options (V2)

- **Option A: Merge explainability into retrieval crate**
  - Pros: Fewer crates, counterfactual engine reuses retrieval internals directly
  - Cons: Retrieval crate grows from single responsibility (retrieve) to dual (retrieve + explain). Explanation format changes when harnesses add new ranking dimensions — different change frequency from retrieval scoring. Tests couple explanation assertions to retrieval internals.
- **Option B: Separate explainability crate (chosen)**
  - Pros: Retrieval stays pure retrieval. Explanation is a separate surface with different consumers (debug tools, admin inspection, future web UI). Testable with fixture scored skills. Prevents retrieval bloat.
  - Cons: One more crate to manage. Counterfactual engine needs scored skill input — but that's the retrieval output, not its internals.
- **Chosen for now:** Option B. Follows V1.1 precedent of separating retrieval/compilation. Explanation is the third consumer of retrieval output (alongside compilation and admin inspection).

- **Option A: Run SkillOpt in maintenance cron inline**
  - Pros: No new service. Cron trigger is already implemented. No deployment topology change.
  - Cons: SkillOpt epochs take minutes (rollout + reflect + edit + gate × 4 epochs × N skills). Maintenance cron is a periodic pass, not a long-running batch job. Mixing batch optimization into the cron pass would block merge/retire/drift/learning cycles for minutes. Violates separation of concerns.
- **Option B: Separate `skill-optimizer` service (chosen)**
  - Pros: Optimization runs independently of maintenance cron. Can run concurrently with other maintenance passes. Can be resource-limited (CPU/memory) independently. Fails without affecting online requests or other offline passes.
  - Cons: New Docker Compose service. New MCP tool for triggering. New deployment artifact.
- **Chosen for now:** Option B. SkillOpt is fundamentally a long-running batch process. It deserves its own service boundary.

- **Option A: Team scope as a separate retrieval pipeline**
  - Pros: Team scope has different latency characteristics — separate pipeline could optimize for remote calls
  - Cons: Duplicates MMR+RRF fusion logic. Violates the N-scope design of `search_scopes_concurrently` which already handles arbitrary scope counts. Creates divergence risk when fusion parameters change.
- **Option B: Team scope as third element in existing N-scope pipeline (chosen)**
  - Pros: Same fusion algorithm. Team scope is just a new ScopeDescriptor with different config (remote URLs, higher timeout). No new retrieval code — just a new scope filter.
  - Cons: Team scope latency could slow down the overall fusion if not properly timeout-guarded.
- **Chosen for now:** Option B. The concurrency architecture already handles this. Team scope timeout guards prevent remote latency from affecting project/global results.

## Context Tiers (V2)

- **Global context:** Constitution v1.0.0 (5 principles, 8 approval boundaries — unchanged). `domain` types and traits (extended: +QualityScores, +HealthProbe, +CorrelationId, +HarnessFormat, +CounterfactualExplanation). Config defaults (V1.1 defaults + V2 additions: `SKILL_TEAM_PG_URL`, `SKILL_TEAM_QDRANT_URL`, `SKILL_COMPILER_MODE`, `SKILL_HARNESS`). PG schema contracts (V1.1 + V2 migrations). Redis event envelope contract (V1.1 + 4 new events). Docker Compose topology (V1.1 + skill-optimizer). Crate dependency direction rules (domain ← infrastructure ← service crates — unchanged).

- **On-demand context:** This architecture artifact (V2 deepening candidates, design-it-twice, drift checks). V1.1 architecture artifact (preserved contracts, deletion test foundation). Vertical-slice architecture contract. SkillLens paper (quality rubric, map-reduce extraction). SkillOpt paper (optimization loop, validation gates, textual learning rate). SkillRAE paper (scoring formula eq.3 — unchanged). PG schema ERD (extended with V2 tables). Qdrant collection config (extended with team collection). Latency budget table (extended: +team scope 800ms, +guidance compiler 3s, +explain_ranking 200ms). Event catalog (13 events total).

- **Ticket-local context:** Exact feature home (which crate). Files list from execution slice. Scope fence and non-goals. Acceptance criteria. Evidence command (`cargo test --workspace` or `docker compose -f docker-compose.test.yml`). Problem narrative + user story linkage. Any slice-specific architectural decisions. V1.1 contract preservation checklist (for slices touching V1.1 code).

## Recommendations for `/deepen-plan`

- Ensure the 11-crate feature home split is maintained in every research-pass enhancement. Do not collapse explainability into retrieval or skill-optimizer into maintenance.
- Ensure Slice 1.1's quality rubric file is adapted for coding-agent domains (Rust, Docker, git workflows, testing), not copied verbatim from SkillLens's ALFWorld/SpreadsheetBench domains.
- Ensure Slice 1.2's map-reduce extraction injects the quality rubric at both map and final-reduce prompts (SkillLens injects at final reduce, but map phase benefits from mode extraction quality).
- Ensure Slice 2.4's cross-tenant isolation is verified with canary-tagged multi-tenant fixtures (DS-017 contract).
- Ensure Slice 3.5's SkillOpt optimizer uses the same optimizer model config pattern as V1.1's extraction provider config — config field routes to concrete implementation.
- Ensure Slice 4.1's counterfactual engine does not depend on retrieval internals — it receives `Vec<ScoredSkill>` as input, not the `SeededGraph` or `RetrievalConfig`.
- Keep `cargo tree -p domain --depth 1` as a CI gate to enforce zero infrastructure deps (V1.1 gate, unchanged).

## Recommendations for `/workflows:work`

- Build Phase 1 slices in strict dependency order: 1.1 (rubric) → 1.2 (map-reduce) → 1.3 (schema) → 1.4 (guidance compiler). Slice 1.5 (remote embedding) can run in parallel with any Phase 1 slice.
- Phase 2 builds on V1.1 trunk + Phase 1 completion. Phase 3 builds on Phase 2 team scope (self-healing needs health events, which need health probes, which need team scope monitoring).
- Phase 4 (explainability) is parallel-safe with Phase 3 once the explainability crate exists. Slice 4.1 is the tracer bullet.
- Phase 5 (multi-harness) is parallel-safe with Phase 4. Build it after Phase 1 (needs quality-scored extraction for provider parity) and Phase 3 (needs optimization proposals for cross-harness verification).
- Run `docker compose -f docker-compose.test.yml up --abort-on-container-exit` after every slice. The E2E test suite should never regress.
- Verify V1.1 contracts are preserved: compile_context returns 4 explicit statuses, suppression written only after ok/no_match, transcript_ref under mounted root, graph_version invalidation correct.
- Quality rubric file must be configurable — different teams may want different rubrics. Support `SKILL_EXTRACTION_RUBRIC_PATH` env var with the V2 default rubric as fallback.
- Team scope is opt-in — when env vars are absent, V2 behavior must be identical to V1.1. No regression path for solo users.

## Recommendations for `/workflows:review`

- Verify `domain` crate still has zero deps on sqlx, qdrant-client, redis, reqwest — only tokio for async trait methods (unchanged CI gate).
- Verify `infrastructure` is the only crate that directly instantiates Ollama/Qdrant/PG/Redis clients — including the new remote clients for team scope.
- Verify all new events (5 types) include `correlation_id` and `idempotency_key` — matching V1.1 event envelope contract.
- Verify `explainability` crate does not import `sqlx` or `qdrant-client` — it receives `Vec<ScoredSkill>`, not raw PG/Qdrant types.
- Verify `skill-optimizer` produces `.optimized` proposals, never auto-approves — constitution §3 enforcement.
- Verify counterfactual explanations are deterministic — same input produces identical explanation. No LLM in the explanation pipeline.
- Verify self-healing catalog contains only safe operations — no skill content mutation, no config changes, no filesystem writes outside `.pending`/`.retired`.
- Verify team scope cross-tenant isolation — no source repo paths in retrieval output, no cross-origin merges.
- Verify V1.1 contracts preserved: compile_context status codes, transcript ingress, state invalidation ordering, event envelope schema.
- Verify `cargo tree -p domain --depth 1` unchanged (CI gate).
- Verify all V1.1 integration tests still pass on V2 schema (backward compatibility).
- Verify constitution compliance: all five principles (§1-§5), all V1.1 baselines, zero un-waived violations.

## Drift Checks (V2)

- **Feature-home drift:** Explainability logic leaking into retrieval. SkillOpt optimization logic leaking into maintenance cron. Health probe implementations leaking into mcp-server. Quality scoring logic leaking into graph-builder. Counterfactual computation depending on retrieval internals. Any of these = refactoring to fix would require splitting code already coupled inside the wrong crate.
- **Shared/global drift:** `domain` accumulating V2 infrastructure imports. `infrastructure` accumulating business rules (remediation decisions, optimization policy, learning thresholds). Team scope config leaking into retrieval internals instead of staying in `ScopeDescriptor.config`.
- **Horizontal scattering:** Quality scoring appearing in both session-extractor and maintenance (should flow: extractor → PG → maintenance reads PG). Health probe logic duplicated in mcp-server and graph-builder (should be in infrastructure, consumed by both). Correlation ID generation duplicated across tool handlers (should be centralized in mcp-server request pipeline).
- **Unearned abstractions:** Adding a trait for quality scoring before the three dimensions are proven in coding domains. Adding a junction table for a pattern that only has one consumer. Adding a cache layer for counterfactual explanations (they're 200ms — benchmark before optimizing).
- **V1.1 contract erosion:** V2 slices accidentally changing V1.1 behavior (e.g., team scope timeout causing compile_context to return degraded when project scope times out). V1.1 tests no longer passing. V1.1 events changing schema. All blocked by contract preservation checks + V1.1 integration test suite.

## Resolved Operational Choices

- **`explainability` is a separate crate but its engine is called inline from the MCP server's `explain_ranking` tool handler.** Pure computation, instant response, no async — no need for a separate service. Same pattern as `compiler`.

- **`skill-optimizer` is a separate Docker Compose service.** Optimization is a batch process (minutes). Running it in the online path would violate SLOs. Cron-triggered or MCP-triggered, never in the request path.

- **Team scope is opt-in via Docker Compose environment variables.** `TEAM_PG_URL` and `TEAM_QDRANT_URL` are optional. When absent, V2 behaves identically to V1.1 — project scope + global scope only. Team scope resolver returns empty when env vars are unset.

- **Quality rubric is a packaged markdown file, not a built-in string constant.** Filesystem-observable configuration (constitution §5). Configurable path via `SKILL_EXTRACTION_RUBRIC_PATH` env var. V2 default rubric shipped in `crates/session-extractor/src/meta_skills/quality_rubric.md`.

- **Self-healing is bounded to 7 cataloged operations.** No open-ended repair. No LLM-driven remediation (that's V3). Catalog is explicit, auditable, and constitution-compliant (zero skill content mutations).

- **SkillOpt produces proposals, not mutations.** `.optimized` files require human rename-to-approve (constitution §3). Rejected optimizations stay recorded for learning signals.

- **V1.1 contracts are preserved throughout V2.** No breaking changes to compile_context, transcript ingress, event catalog, scope persistence, or state invalidation. V2 is additive from the first schema migration to the last compiler.

## Pipeline Architecture

### Pipeline 1: Session-End Extraction (Event-Driven — Async)

**Trigger:** MCP `extract_session` tool called by harness on session end. Returns immediately with `run_id`. Extraction runs async in background.

```
Session transcript
  │
  ▼
┌──────────────────────────────────────────────┐
│ STAGE 1: PREFLIGHT                           │
│ Runtime: synchronous, seconds                │
│                                              │
│ a) Split transcript into individual          │
│    trajectory turns (tool-call boundary)     │
│    ┌────────────────────────────────────┐    │
│    │ Turn 1: [user prompt] [agent act]  │    │
│    │   [tool calls] [result]            │    │
│    │   Outcome: SUCCESS                 │    │
│    │ Turn 2: ...                        │    │
│    │   Outcome: FAILURE (compile error) │    │
│    │ Turn N: ...                        │    │
│    └────────────────────────────────────┘    │
│                                              │
│ b) Classify each turn as success/failure     │
│    • Success: task completed, tests pass,    │
│      user confirms, no error signals         │
│    • Failure: compile error, test failure,   │
│      user corrects agent, agent retries,     │
│      explict "didn't work" signal           │
│    • Unknown: ambiguous or partial result    │
│      → treat as success with low confidence  │
│                                              │
│ c) Load quality rubric from filesystem:      │
│    crates/session-extractor/src/meta_skills/ │
│      quality_rubric.md                       │
│    — OR env var SKILL_EXTRACTION_RUBRIC_PATH │
│    Contains 4 scoring dimensions:            │
│    1. Failure Mechanism Encoding (FME)       │
│    2. Actionable Specificity (AS)            │
│    3. High-Risk Action Avoidance (HRA)       │
│    4. Environment/Tool Semantics (ETS)       │
│    Each: 0.0-1.0 scale, coding-domain        │
│    examples and anti-examples                 │
│                                              │
│ d) Assemble system prompt:                   │
│    system_prompt = rubric_text +             │
│      extraction_instructions +               │
│      provider-specific formatting            │
│                                              │
│ e) Error handling:                           │
│    • Transcript too short (<3 turns) →       │
│      ExtractionError::InsufficientTurns      │
│    • Transcript too long (>500 turns) →      │
│      truncate to last 200 turns + summary    │
│    • Transcript not valid JSONL →            │
│      ExtractionError::InvalidFormat          │
│    • Rubric file missing →                   │
│      ExtractionError::MissingRubric,         │
│      fallback: built-in minimal rubric        │
│                                              │
│ Decision points:                             │
│ • Provider routing: config `provider` field  │
│   → ClaudeExtractor | OllamaExtractor        │
│ • Extraction mode: config flag               │
│   → single-pass | map-reduce                 │
│ • Fallback chain on model failure:            │
│   primary → secondary → single-pass          │
│   → ExtractionError::AllProvidersExhausted    │
└──────────────────┬───────────────────────────┘
                   ▼
┌──────────────────────────────────────────────┐
│ STAGE 2: MAP PHASE                           │
│ Runtime: parallel, minutes                   │
│ Concurrency: LLM semaphore (default 4)        │
│ Timeout: 10s per trajectory turn              │
│                                              │
│ For each turn (tokio::spawn, bounded join):  │
│                                              │
│ a) Assign prompt based on turn outcome:      │
│    • SUCCESS turn → success_mode prompt:     │
│      "What patterns, decisions, or           │
│       procedures led to success?"            │
│    • FAILURE turn → failure_mode prompt:     │
│      "What caused the failure? What          │
│       specific mechanism failed? What        │
│       should be avoided?"                    │
│    • UNKNOWN turn → mixed prompt:            │
│      "What happened? What patterns           │
│       emerged regardless of outcome?"        │
│                                              │
│ b) LLM extracts ModeSet per turn:            │
│    {                                         │
│      turn_index: N,                          │
│      outcome: "success" | "failure",         │
│      success_modes: [                        │
│        "pattern: docker build cache          │
│         invalidated by timestamp drift —     │
│         use --no-cache for reproducible      │
│         builds"                               │
│      ],                                      │
│      failure_modes: [                        │
│        "pattern: cargo check succeeds but    │
│         cargo build fails when test code     │
│         has unused imports — run cargo       │
│         test --no-run for full compilation"   │
│      ],                                      │
│      summary: "Docker + Rust compilation     │
│        consistency patterns"                 │
│    }                                         │
│                                              │
│ c) Caps enforced:                            │
│    • max_modes_per_trajectory = 3            │
│    • Extra modes dropped (log warning)       │
│    • Quality dimension scoring deferred      │
│      to Stage 5 (separate evaluator)         │
│                                              │
│ d) Per-turn error handling:                  │
│    • Turn timeout → mark turn with           │
│      extraction_status: "skipped_timeout"    │
│    • Turn LLM error → retry once, then skip  │
│    • Turn hallucination (invalid JSON) →     │
│      retry with format: "json" hint          │
│    • Turn too short (<50 tokens) → skip      │
│      with reason "too_short"                 │
│                                              │
│ e) Partial failure semantics:                │
│    • If >50% of turns fail → abort,          │
│      fallback to single-pass extraction      │
│    • If ≤50% fail → continue with            │
│      successful turns only, log warning      │
│    • If ALL turns fail →                     │
│      ExtractionError::MapPhaseFailed         │
└──────────────────┬───────────────────────────┘
                   ▼
┌──────────────────────────────────────────────┐
│ STAGE 3: REDUCE — INTERMEDIATE MERGE          │
│ Runtime: sequential groups, minutes           │
│ Timeout: 30s per merge group                  │
│                                              │
│ a) Group ModeSets by merge_group_size:        │
│    • Default G=5 (coding domain —             │
│      transcripts 5-10x longer than            │
│      SkillLens domains)                       │
│    • If ≤5 ModeSets total → skip to           │
│      STAGE 4 (final reduce directly)          │
│    • 20 turns → 4 groups of 5                │
│    → 4 merges → still >5?                    │
│    → merge those 4 into 1 → Stage 4          │
│                                              │
│ b) Per-group merge via LLM:                  │
│    Input: N ModeSets (N ≤ G)                 │
│    Prompt:                                   │
│    "Consolidate these ModeSets into one.     │
│     Preserve at least one instance of        │
│     each unique failure class. Drop truly    │
│     vague or contradictory patterns.         │
│     Favor concrete procedural patterns       │
│     over generic advice."                    │
│    Output: consolidated ModeSet              │
│                                              │
│ c) Rare-pattern preservation:                │
│    If a failure pattern has quality          │
│    dimension >0.7 AND appears only 1x:       │
│    → min_pattern_frequency=1 override        │
│    → do NOT drop as "too narrow"             │
│    Savable pattern classes:                  │
│    • Domain-specific tool failure (unique    │
│      build system quirk, compiler flag,      │
│      docker quirk, git edge case)            │
│    • Environment-specific (OS, arch,         │
│      version-specific)                       │
│    • Non-obvious ordering dependency          │
│      (must run X before Y because...)        │
│    Droppable pattern classes:                 │
│    • Generic advice ("be careful",            │
│      "test thoroughly")                      │
│    • Contradictory advice in same group      │
│    • Overly specific to a single file        │
│      that was renamed/deleted                │
│                                              │
│ d) Convergence check:                        │
│    • After each round: count remaining       │
│      ModeSets                                │
│    • If count has not decreased →            │
│      break out (cannot merge further)        │
│    • If count ≤ 5 → proceed to Stage 4      │
│    • Max 4 merge rounds (configurable)       │
│                                              │
│ e) Error handling:                           │
│    • Merge LLM timeout → retry once with     │
│      smaller group (split into sub-groups)   │
│    • Merge produces invalid output →         │
│      retry with format enforcement           │
│    • Merge drops ALL patterns →               │
│      preserve originals, log warning         │
│    • Infinite loop detection: if merge       │
│      round produces same count as prior      │
│      → break with partial result             │
└──────────────────┬───────────────────────────┘
                   ▼
┌──────────────────────────────────────────────┐
│ STAGE 4: REDUCE — FINAL SYNTHESIS             │
│ Runtime: single call, ~30-60 seconds          │
│ Timeout: 60s                                  │
│                                              │
│ a) Tool-calling LLM receives:                │
│    • Remaining ModeSets (≤5 after merge)     │
│    • Quality rubric (guides output quality)  │
│    • Skill budget constraints                │
│    • Tool definitions:                       │
│      create_skill(name, desc, tags)           │
│      update_skill(id, patches)               │
│      add_procedure(skill_id, steps[])         │
│      add_convention(skill_id, rules[])        │
│      add_asset(skill_id, path, content)      │
│      blacklist_action(skill_id, action)       │
│                                              │
│ b) Synthesis prompt:                         │
│    "You have ModeSets from session analysis. │
│     Extract reusable skills. Each skill       │
│     must:                                    │
│     • Encode concrete failure mechanisms     │
│     • Be actionable (specific steps)         │
│     • Avoid blacklisting harmful actions     │
│     • Respect tool semantics (ci, test,      │
│       build, deploy, docker, git)            │
│     Budget: max 3 skills, max 3000 chars     │
│     each. Prefer fewer higher-quality        │
│     skills."                                 │
│                                              │
│ c) Budget enforcement (post-processing):     │
│    • If >3 skills produced → keep top-3      │
│      by LLM confidence scores                │
│    • If skill >3000 chars → truncate at      │
│      sentence boundary near limit            │
│    • If skill <200 chars → drop (too thin)   │
│    • If duplicate skill names → suffix       │
│      with origin distinction                 │
│                                              │
│ d) Tool-calling validation:                  │
│    • Claude: tool_use with strict: true      │
│      → guaranteed schema conformance         │
│    • Ollama: format: "json"                  │
│      → best-effort, post-validate            │
│    • Fallback: if model cannot follow        │
│      tool-calling protocol → switch to       │
│      single-pass extraction with quality     │
│      rubric only                             │
│                                              │
│ e) Skill deduplication (within run):         │
│    • If two extracted skills have cosine     │
│      similarity >0.9 → merge into one        │
│    • If skill exists in graph already        │
│      (same name + cosine >0.85) →            │
│      propose update_skill instead            │
│    • If skill is near-identical to active    │
│      skill → skip (no value add)             │
│                                              │
│ Decision points:                             │
│ • Single-pass fallback if map-reduce fails    │
│   (config-gated, always available)           │
│ • Tool-calling capable? → yes: tool route,   │
│   no: structured JSON route                  │
│ • Skill already exists? → update vs new      │
│   vs skip                                    │
└──────────────────┬───────────────────────────┘
                   ▼
┌──────────────────────────────────────────────┐
│ STAGE 5: QUALITY ASSESSMENT                   │
│ Runtime: parallel per-skill, seconds          │
│ Evaluator: separate Ollama model              │
│            (NOT self-assessment — SkillLens    │
│            proves self-assessment biased,     │
│            LLM judges 46.4% accurate)          │
│                                              │
│ For each extracted skill (parallel):          │
│                                              │
│ a) Evaluator model prompt:                    │
│    "Score this skill on 4 dimensions          │
│     (0.0 = absent, 1.0 = perfect):            │
│     1. Failure Mechanism Encoding:            │
│        Does it encode HOW and WHY a           │
│        specific failure occurs?               │
│        NOT: 'test before deploying'           │
│        YES: 'cargo check catches unused       │
│        imports but cargo build needs those    │
│        imports resolved — run cargo test       │
│        --no-run for full compilation check'   │
│     2. Actionable Specificity:                │
│        Does it provide concrete steps?        │
│        NOT: 'use Docker effectively'           │
│        YES: 'docker build --no-cache          │
│        --build-arg ENV=prod -t app .'          │
│     3. High-Risk Action Avoidance:            │
│        Does it blacklist harmful actions?     │
│        NOT: 'be careful with sudo'            │
│        YES: 'never use sudo npm install -g;   │
│        use npx or local installs'              │
│     4. Environment/Tool Semantics:            │
│        Does it encode tool behavior?          │
│        NOT: 'use git for version control'     │
│        YES: 'git rebase vs merge: rebase      │
│        rewrites history, never rebase         │
│        shared branches'                       │
│     Return JSON: { scores, reasoning }"       │
│                                              │
│ b) Compute combined_utility_score:            │
│    unweighted average of 4 dimensions          │
│    (SkillLens found weights don't improve     │
│     beyond simple average for 64-66%          │
│     dimension accuracy)                       │
│                                              │
│ c) Score thresholds (display-only, not gate): │
│    • ≥0.7: high quality (green)               │
│    • 0.4-0.69: medium quality (yellow)        │
│    • <0.4: low quality (red, flagged)         │
│    • <0.2: likely extraction artifact          │
│      (should not be approved)                  │
│    These are DISPLAY signals. Do not          │
│    auto-reject based on quality scores        │
│    (SkillLens: 34% error rate).               │
│                                              │
│ d) Error handling:                           │
│    • Evaluator model unavailable →            │
│      skip scoring, write skill without        │
│      quality scores, log warning              │
│    • Evaluator returns invalid JSON →         │
│      retry once, then skip                    │
│    • Evaluator gives all 0.5 (lazy) →         │
│      flag as "unreliable_evaluator"           │
│    • Score computation overflow →             │
│      clamp to [0.0, 1.0]                      │
└──────────────────┬───────────────────────────┘
                   ▼
┌──────────────────────────────────────────────┐
│ STAGE 6: OUTPUT                               │
│ Runtime: synchronous, milliseconds            │
│                                              │
│ a) For each skill meeting minimum bar:        │
│    Write to filesystem:                       │
│    .skills/<skill-name>/SKILL.md.pending      │
│                                              │
│    YAML frontmatter:                         │
│    ┌──────────────────────────────────┐      │
│    │ ---                              │      │
│    │ name: docker-build-cache-invalid │      │
│    │ description: |                   │      │
│    │   Docker build cache invalidates │      │
│    │   when timestamps drift. Use     │      │
│    │   --no-cache for reproducible    │      │
│    │   CI builds.                     │      │
│    │ extraction_run_id: uuid          │      │
│    │ extraction_source_harness: cc    │      │
│    │ extraction_source_model: sonnet  │      │
│    │ source_session_id: uuid          │      │
│    │ extracted_at: ISO8601            │      │
│    │ quality_scores:                  │      │
│    │   failure_mechanism: 0.72        │      │
│    │   actionable_specificity: 0.65   │      │
│    │   high_risk_avoidance: 0.81      │      │
│    │   tool_semantics: 0.68           │      │
│    │   combined_utility: 0.715        │      │
│    │   evaluator_model: llama3.2:3b   │      │
│    │ tags: [docker, ci, caching]      │      │
│    │ warning_at: +7d                  │      │
│    │ expires_at: +30d                 │      │
│    │ ---                              │      │
│    │ # Docker Build Cache Invalidation│      │
│    │ ...                              │      │
│    └──────────────────────────────────┘      │
│                                              │
│ b) Emit events (Redis streams):              │
│    • extraction.completed {                   │
│        run_id, session_id,                    │
│        skills_extracted: 2,                   │
│        skills_skipped: 1,                     │
│        provider: "claude",                    │
│        extraction_mode: "map_reduce",         │
│        duration_ms: 245000,                   │
│        correlation_id                        │
│      }                                        │
│    • skill.quality_scored {                   │
│        skill_id, quality_scores,              │
│        evaluator_model                        │
│      } (per skill)                            │
│                                              │
│ c) PG writes:                                │
│    • session_logs: extraction_status =       │
│      "completed", skill_count                │
│    • skills table: INSERT with               │
│      lifecycle = "pending",                  │
│      quality_scores = JSONB                  │
│    • skill_usage: initial row with           │
│      extraction context                      │
│                                              │
│ d) MCP response (immediate, async):          │
│    {                                         │
│      extraction_run_id: uuid,                │
│      status: "running",                      │
│      estimated_completion: ISO8601           │
│    }                                         │
│    Client polls admin tool for status         │
│                                              │
│ e) Partial failure handling:                 │
│    • If 2 of 3 skills written OK:             │
│      extraction.completed with warnings      │
│    • If some skills fail to write:            │
│      log per-skill errors, continue          │
│    • If ALL skills fail to write:             │
│      extraction.failed event                 │
│    • If PG write succeeds but event fails:    │
│      outbox relay retries later              │
│    • If event succeeds but PG fails:           │
│      extraction.completed with               │
│      pg_write_failed flag                     │
└──────────────────────────────────────────────┘

Total pipeline time: 3-8 minutes
  • Preflight: 1-2s
  • Map phase: 20-60s (parallel, dep on transcript length)
  • Intermediate merge: 30-120s (dep on group rounds)
  • Final synthesis: 30-60s
  • Quality assessment: 10-30s (parallel per skill)
  • Output: 50ms

Trigger: per-session-end (event-driven)
  NOT periodic — fires when harness calls extract_session MCP tool
  Async — returns immediately, extraction runs in background
  Single session at a time (extraction run queue, FIFO)
```

### Pipeline 2: Graph Builder (Continuous + Periodic)

Four distinct background loops operating at different cadences.

#### Loop 2A: Filesystem Watcher (Continuous, inotify)

**Trigger:** Filesystem change in any `.skills/` directory.

```
inotify event (create, modify, rename, delete on SKILL.md files)
  │
  ▼
┌──────────────────────────────────────────────┐
│ W1. EVENT DEBOUNCE & DEDUPLICATE              │
│    • Debounce 500ms burst into single event   │
│    • Deduplicate by (file_path, mtime_hash)  │
│    • Emit skill.file_changed with             │
│      idempotency key                          │
│                                              │
│ W2. PARSE & VALIDATE                          │
│    • Read file, parse YAML frontmatter        │
│    • Validate name: ^[a-z0-9]+(-[a-z0-9]+)*$ │
│    • Validate description: 1-1024 chars       │
│    • If invalid → ExtractionError::           │
│      InvalidSkillFormat, skip                 │
│    • Detect lifecycle:                        │
│      .md → active                             │
│      .md.pending → proposed                   │
│      .md.retired → retired                    │
│      .md.optimized → optimized proposal       │
│      .md.merge-proposal → merge proposal      │
│      .promote → promotion request             │
│                                              │
│ W3. EMBEDDING GENERATION                      │
│    • Generate embedding from skill content    │
│      (name + description + body)              │
│    • via EmbeddingService trait                │
│    • 768-dim vector                           │
│    • Timeout: 500ms per skill                 │
│    • Concurrency: 4 parallel (semaphore)      │
│    • Cache: content hash → embedding          │
│      (skip if content unchanged)              │
│    • For team scope: configurable remote      │
│      embedding endpoint                       │
│                                              │
│ W4. GRAPH INSERT (PG + Qdrant dual-write)     │
│    a) PG: INSERT/UPDATE skills row            │
│       • If new: INSERT with scope, tags,      │
│         lifecycle, quality_scores             │
│       • If update: UPDATE changed fields,     │
│         bump graph_version                    │
│       • If delete (file removed):             │
│         UPDATE lifecycle = "deleted",          │
│         do NOT DELETE row (audit trail)       │
│    b) PG: scope junction table                │
│       • If team scope promotion: INSERT       │
│         INTO skill_scopes with                │
│         provenance_hash, origin_repo          │
│    c) Qdrant: UPSERT vector                   │
│       Project → project_skills collection     │
│       Global → global_skills collection       │
│       Team → skills_team (remote) collection  │
│    d) PG outbox: write Qdrant write intent    │
│       • If Qdrant succeeds → mark done        │
│       • If Qdrant fails → relay retries       │
│       • If PG fails after Qdrant →            │
│         orphan vector (reconciliation)        │
│                                              │
│ W5. COMMUNITY REASSIGNMENT                     │
│    • Compute skill embedding                  │
│    • HDBSCAN clustering against existing       │
│      community centroids                      │
│    • Tag-based community membership            │
│    • Update community → skill edges           │
│    • Recompute community centroid              │
│    • Mark changed communities for              │
│      retrieval cache invalidation             │
│                                              │
│ W6. GRAPH VERSION BUMP                        │
│    • graph_version += 1 (atomic, PG sequence) │
│    • Write graph_snapshots row:               │
│      { version, timestamp, skill_count,        │
│        community_count, snapshot_hash }        │
│    • Invalidate retrieval cache:               │
│      cache_key = version mismatch →           │
│      retrieval rebuilds on next request        │
│    • Emit event: graph.rebuilt {               │
│        version, skills_changed,                │
│        communities_changed, duration_ms }       │
│                                              │
│ W7. CONSISTENCY VERIFICATION (inline)          │
│    • PG skill count ≈ Qdrant vector count     │
│      (allow ±2 for in-flight writes)          │
│    • Graph version is sequential              │
│    • No orphan skill nodes (skills with       │
│      no community edge)                       │
│    • No orphan community nodes (empty         │
│      communities)                             │
│    Errors → drift alarm event (non-blocking)  │
└──────────────────────────────────────────────┘

Trigger: inotify on .skills/ directories
Frequency: near-instant on change
Reconciliation: every 5 minutes (catches missed events)
```

#### Loop 2B: Maintenance — Merge Proposals (Periodic, 30min)

**Trigger:** Maintenance cron pass (tokio::time::interval, 30min).

```
Cron triggers merge pass
  │
  ▼
┌──────────────────────────────────────────────┐
│ M1. CANDIDATE DISCOVERY                       │
│    Query PG:                                  │
│    SELECT s1.id, s2.id,                      │
│           1 - (s1.embedding <=>               │
│                s2.embedding) AS similarity    │
│    FROM skills s1                             │
│    JOIN skills s2 ON s1.id < s2.id            │
│    WHERE s1.lifecycle = 'active'              │
│      AND s2.lifecycle = 'active'              │
│      AND 1 - (s1.embedding <=>                │
│          s2.embedding) > 0.85                 │
│      AND s1.scope_type = s2.scope_type        │
│      -- team scope: both must share           │
│      -- origin_repo (cross-tenant BLOCK)      │
│      AND (s1.scope_type != 'team' OR           │
│           s1.provenance_hash::jsonb->>         │
│           'origin_repo' =                      │
│           s2.provenance_hash::jsonb->>         │
│           'origin_repo')                      │
│    • Deduplicate pairs (A,B same as B,A)      │
│    • Skip pairs with active merge proposal    │
│    • Capped: max 10 candidate pairs per pass  │
│                                              │
│ M2. MERGE EXECUTION                           │
│    For each candidate pair (sequential):      │
│    a) LLM merges skill pair:                  │
│       • Combine procedures (deduplicate)      │
│       • Merge conventions (resolve by         │
│         specificity: more specific wins)      │
│       • Union of tags                         │
│       • Union of assets/references            │
│       • Preserve provenance from both         │
│    b) Merge prompt:                           │
│       "Merge these two skills into one.       │
│        Resolve conflicts by keeping the       │
│        more specific version. If they         │
│        disagree, note the conflict and        │
│        choose the safer option."              │
│    c) Output: merged Skill struct             │
│    d) If merge LLM fails (timeout,            │
│       garbage) → skip pair, log               │
│                                              │
│ M3. HELD-OUT VALIDATION (behavioral)          │
│    For the merged skill:                      │
│    a) Load 5 held-out session transcripts     │
│       (different from training set)           │
│    b) Replay each transcript with:            │
│       • No skills (baseline)                  │
│       • Original skill A                      │
│       • Original skill B                      │
│       • Merged skill                          │
│    c) Score: task completion rate             │
│       (successful turns / total turns)        │
│    d) Must strictly improve over baseline     │
│       AND over both originals                 │
│       Ties rejected (SkillLens: tie=noise)    │
│    e) If merged degrades any → reject         │
│       log, skip pair                          │
│    IMPORTANT: behavioral validation,           │
│    NOT LLM judge (46.4% accuracy proven       │
│    harmful by SkillLens)                      │
│                                              │
│ M4. PROPOSAL OUTPUT                           │
│    If validation passes:                      │
│    a) Write merged skill as:                  │
│       .skills/<name>/SKILL.md.merge-proposal  │
│    b) Frontmatter:                            │
│       merged_from: [id_a, id_b]              │
│       validation_score_before: 0.65           │
│       validation_score_after: 0.73            │
│       held_out_session_ids: [...]             │
│       merge_proposed_at: ISO8601              │
│    c) PG: INSERT INTO merge_proposals         │
│       (merged_skill_id, source_ids,           │
│        validation_scores)                     │
│    d) Event: skill.merge_proposed {           │
│        merged_from, validation_delta,          │
│        correlation_id }                        │
│    e) Human approves by renaming               │
│       .merge-proposal → .md                   │
│    f) On approval: retire source skills       │
│       (mark lifecycle: "retired_by_merge")    │
│                                              │
│ M5. CLEANUP                                   │
│    • Remove duplicate candidate pairs from    │
│      discovery (already processed)            │
│    • Log merge pass metrics:                  │
│      candidates_found, candidates_merged,     │
│      candidates_rejected (by validation),     │
│      duration_ms                              │
│    • If merge_acceptance_rate <20% over       │
│      4 passes → tune similarity threshold    │
│      up (0.85 → 0.90)                        │
└──────────────────────────────────────────────┘
```

#### Loop 2C: Maintenance — Retirement Proposals (Periodic, 30min)

**Trigger:** Maintenance cron pass (same 30min interval as merge).

```
Cron triggers retirement pass
  │
  ▼
┌──────────────────────────────────────────────┐
│ R1. USAGE COLLECTION                          │
│    For each active skill (lifecycle='active'):│
│    a) Query skill_usage:                      │
│       SELECT COUNT(*) AS invocations,         │
│              COUNT(*) FILTER (WHERE            │
│                context_status = 'ok')          │
│                AS successful_uses,             │
│              MAX(timestamp) AS last_used      │
│       FROM skill_usage                        │
│       WHERE skill_id = $1                     │
│         AND timestamp > NOW() -               │
│             INTERVAL '90 days'                 │
│    b) Team scope aggregated:                  │
│       SUM across all team members             │
│       (GROUP BY skill_id in team scope)       │
│    c) Days since last use                     │
│    d) Compute: usage_per_month                │
│       = invocations / 3 (3-month window)      │
│                                              │
│ R2. UTILITY SCORING                           │
│    a) Load quality_scores from PG             │
│    b) If no quality scores (pre-V2):          │
│       default quality = 0.5                   │
│    c) Compute:                                │
│       usage_component =                       │
│         min(usage_per_month / 10, 1.0)       │
│       quality_component =                     │
│         combined_utility_score                │
│       utility =                               │
│         usage_component * USAGE_WEIGHT +      │
│         quality_component * QUALITY_WEIGHT     │
│    d) Confidence scaling:                     │
│       if n_samples < 30:                      │
│         confidence = sqrt(n / 30)             │
│         utility *= confidence                 │
│    e) Quality weight phases:                  │
│       V2 launch:    USAGE=0.8 QUALITY=0.2     │
│       V2 + 30 days: USAGE=0.7 QUALITY=0.3     │
│       V2 + 90 days: USAGE=0.6 QUALITY=0.4     │
│       Phase shifts require behavioral         │
│       canary validation (Pearson r ≥ 0.3)     │
│                                              │
│ R3. GRADUATED RETIREMENT THRESHOLDS           │
│    utility < 0.15 → propose retire            │
│    utility < 0.25 → warn annotation            │
│    utility < 0.35 → flag (14-day grace)        │
│                                              │
│    Team scope grace periods:                  │
│    • Default: 90 days zero usage (all         │
│      team members, aggregate)                 │
│    • Low-quality (utility < 0.3): 30 days     │
│    • High-quality (utility > 0.7): 180 days   │
│    • Zero-usage: skill must have zero          │
│      retrievals AND zero uses                 │
│      across ALL team members for              │
│      the full grace period                    │
│                                              │
│ R4. PROPOSAL OUTPUT                           │
│    If skill meets retire threshold:           │
│    a) Rename SKILL.md → SKILL.md.retired      │
│    b) Update PG: lifecycle = "retired",       │
│       retired_at = NOW()                      │
│    c) PG: INSERT INTO retirement_proposals    │
│       (skill_id, utility_score,               │
│        usage_samples, retire_reason)          │
│    d) Event: skill.retirement_proposed {      │
│        skill_id, utility_score,               │
│        days_since_last_use,                    │
│        retirement_reason }                     │
│    e) If quality < 0.3: add frontmatter       │
│       flag "possible_extraction_artifact"     │
│    f) Human approves by deleting file or      │
│       renaming back to .md                    │
│    g) On approval: remove from Qdrant         │
│       collection, keep PG row (audit)         │
│                                              │
│ R5. REGRESSION SAFETY                         │
│    Before any batch retirement pass:          │
│    a) Snapshot: all currently active skills   │
│    b) Simulate: new thresholds against         │
│       snapshot                                │
│    c) Check: no previously-accepted skill     │
│       would be retired under new thresholds   │
│    d) If yes: BLOCK threshold change          │
│       (regression guard is automatic,         │
│       no human override)                      │
│                                              │
│ R6. CROSS-SCOPE RETIREMENT                     │
│    • Project-scope skills: retired if         │
│      unused in project                        │
│    • Global-scope skills: retired if          │
│      unused across all projects               │
│    • Team-scope skills: retired if            │
│      unused across all team members           │
│    • Skill can be retired in one scope        │
│      but active in another (junction          │
│      table enables per-scope retirement)      │
└──────────────────────────────────────────────┘
```

#### Loop 2D: SkillOpt Optimizer (On Trigger or Weekly, Long-Running)

**Trigger:** MCP `trigger_optimization` tool OR weekly cron OR after +10 new usage samples.

```
Optimization triggered
  │
  ▼
┌──────────────────────────────────────────────┐
│ O0. PREREQUISITE CHECK                        │
│    • Skill has ≥5 usage samples?              │
│      If no → OptimizationError::              │
│      InsufficientData, abort                  │
│    • Held-out transcripts exist (≥20% split)? │
│      If no → OptimizationError::              │
│      NoHeldOutData, abort                     │
│    • Optimizer model configured and           │
│      stronger than extraction provider?       │
│      If config not set → error                │
│      If configured but unreachable →          │
│      OptimizationError::                      │
│      OptimizerModelUnavailable                │
│    • Return immediately: run_id               │
│      Status queryable via admin tool          │
│                                              │
│ O1. DATA LOADING                              │
│    a) Load skill from PG (current version)    │
│    b) Load batch_size=40 held-out             │
│       transcripts (stratified: 20 success,     │
│       20 failure)                             │
│       If <40 available: use all, log          │
│       warning about low sample size           │
│    c) Load current validation baseline        │
│       (last accepted skill score)             │
│    d) Initialize:                             │
│       edit_budget = 4 (epoch 0)               │
│       rejected_edit_buffer = empty             │
│       best_skill = current version            │
│       best_score = baseline score             │
│       slow_update = false (epochs < 2)        │
│                                              │
│ O2. EPOCH LOOP (1..4)                         │
│    For each epoch (sequential, ~5min each):   │
│                                              │
│    ┌────────────────────────────────────┐    │
│    │ O2a. ROLLOUT STAGE                 │    │
│    │                                    │    │
│    │ • Load 40 transcripts              │    │
│    │ • For each transcript (parallel):   │    │
│    │   1. Inject current skill into      │    │
│    │      context (simulate retrieval)   │    │
│    │   2. Compare task completion:       │    │
│    │      with-skill vs without-skill    │    │
│    │   3. Score per transcript:          │    │
│    │      • Did the skill help?          │    │
│    │      • Which steps were followed?   │    │
│    │      • Which steps were ignored?    │    │
│    │      • Did the task succeed?        │    │
│    │ • Produce:                          │    │
│    │   success_batch = top-8 scoring     │    │
│    │   failure_batch = bottom-8 scoring  │    │
│    │ • Cross-consumer rollout: replay    │    │
│    │   same transcripts on ALL consumer  │    │
│    │   models (Claude, Ollama, etc.)     │    │
│    │   Store per-consumer scores          │    │
│    │ • Timeout: 10s per transcript       │    │
│    └──────────────┬─────────────────────┘    │
│                   ▼                           │
│    ┌────────────────────────────────────┐    │
│    │ O2b. REFLECT STAGE                 │    │
│    │                                    │    │
│    │ • Optimizer model receives:         │    │
│    │   - Current skill text              │    │
│    │   - 8 success transcripts           │    │
│    │   - 8 failure transcripts           │    │
│    │   - Per-consumer scores             │    │
│    │   - Rejected edit buffer:           │    │
│    │     "Previously rejected edits      │    │
│    │      (avoid these):                 │    │
│    │      - Added 'always use docker' →  │    │
│    │        score dropped 0.12           │    │
│    │      - Removed 'check .env' →       │    │
│    │        score dropped 0.08"          │    │
│    │ - 3 refinement rounds:              │    │
│    │   Round 1: surface-level patterns    │    │
│    │   Round 2: deeper analysis           │    │
│    │   Round 3: final diagnosis           │    │
│    │ • Output: reflection analysis        │    │
│    │   {                                  │    │
│    │     success_patterns: [...],         │    │
│    │     failure_patterns: [...],         │    │
│    │     specific_edits: [                │    │
│    │       {                              │    │
│    │         type: "add"|"delete"|        │    │
│    │               "replace",             │    │
│    │         target: "...",               │    │
│    │         new_content: "...",          │    │
│    │         reasoning: "...",            │    │
│    │         confidence: 0.85,            │    │
│    │         consumer_impact: {           │    │
│    │           claude: +0.05,             │    │
│    │           ollama: -0.02              │    │
│    │         }                            │    │
│    │       }                              │    │
│    │     ]                                │    │
│    │   }                                  │    │
│    └──────────────┬─────────────────────┘    │
│                   ▼                           │
│    ┌────────────────────────────────────┐    │
│    │ O2c. EDIT STAGE                    │    │
│    │                                    │    │
│    │ • Budget = cosine_decay(epoch, 4)   │    │
│    │   epoch 0: 4 edits allowed          │    │
│    │   epoch 1: 3 edits allowed          │    │
│    │   epoch 2: 2 edits allowed          │    │
│    │   epoch 3: 2 edits allowed (floor)  │    │
│    │ • Apply edits in priority order     │    │
│    │   (highest confidence first)        │    │
│    │ • Each edit is:                     │    │
│    │   add(text, position)               │    │
│    │   delete(start, end)                │    │
│    │   replace(start, end, new_text)     │    │
│    │ • Protected sections (epoch 2+):    │    │
│    │   <!-- PROTECTED_START -->          │    │
│    │   <core procedure text>             │    │
│    │   <!-- PROTECTED_END -->            │    │
│    │   Step-edits cannot touch these      │    │
│    │   Only slow_update (epoch-end) can  │    │
│    │ • Stop when budget exhausted OR     │    │
│    │   no edits with confidence >0.6     │    │
│    │ • Produce: edited_skill             │    │
│    └──────────────┬─────────────────────┘    │
│                   ▼                           │
│    ┌────────────────────────────────────┐    │
│    │ O2d. GATE STAGE                    │    │
│    │                                    │    │
│    │ • Test edited_skill against         │    │
│    │   held-out validation split         │    │
│    │   (DIFFERENT from rollout batch)    │    │
│    │ • Minimum 8 validation transcripts  │    │
│    │ • Acceptance criteria:              │    │
│    │   1. STRICT improvement over        │    │
│    │      current best (ties rejected)   │    │
│    │   2. Cross-consumer: must improve   │    │
│    │      at least 1 consumer without    │    │
│    │      degrading any other by >5%     │    │
│    │ • If accepted:                      │    │
│    │   best_skill = edited_skill         │    │
│    │   best_score = validation_score     │    │
│    │   edit_budget remains for epoch     │    │
│    │ • If rejected:                      │    │
│    │   buffer rejected edits for next    │    │
│    │     reflection round                │    │
│    │   revert to previous best_skill     │    │
│    │   continue to next step if budget   │    │
│    │     remains                         │    │
│    └────────────────────────────────────┘    │
│                                              │
│    EPOCH-END SLOW UPDATE:                    │
│    • Epoch 2+: analyze all epoch's           │
│      accepted/rejected patterns              │
│    • Update PROTECTED sections if            │
│      all cross-consumer scores improved      │
│    • Update meta-skill (optimizer-side       │
│      state, never in skill file)             │
│    • Compute cosine decay for next epoch     │
│    • Log: epoch_completed, edits_accepted,   │
│      edits_rejected, score_delta             │
│                                              │
│ O3. FINAL OUTPUT                              │
│    If best_score > baseline_score:           │
│    a) Write best_skill as:                   │
│       .skills/<name>/SKILL.md.optimized      │
│    b) Frontmatter:                           │
│       optimized_from: <skill_id>             │
│       optimization_run_id: <uuid>            │
│       validation_score_before: 0.65          │
│       validation_score_after: 0.78           │
│       epochs_completed: 4                    │
│       edits_accepted: 3                      │
│       edits_rejected: 5                      │
│       optimizer_model: <model>               │
│       per_consumer_scores: {                 │
│         claude: +0.13, ollama: +0.09         │
│       }                                      │
│    c) Event: skill.optimized                 │
│    d) PG: INSERT INTO optimization_runs      │
│       (run_id, skill_id, epochs,             │
│        score_before, score_after,            │
│        optimizer_model)                      │
│    e) Human approves: rename                  │
│       .optimized → .md                       │
│    If best_score ≤ baseline_score:            │
│    a) OptimizationRunStatus::NoImprovement    │
│    b) Write optimization analysis only (no   │
│       .optimized file — nothing to approve)  │
│    c) Log: why no improvement found,         │
│       which consumer regressed               │
│                                              │
│ Total time: ~20 min per skill                │
│   • Data loading: 5-10s                      │
│   • 4 epochs × ~5min each                    │
│   • Final output: 1-2s                       │
│ Concurrent: N skills can optimize in         │
│   parallel (separate tokio tasks)            │
│ Not in request path: async batch service     │
└──────────────────────────────────────────────┘
```

### Pipeline 3: Health Monitoring (Continuous, 30s)

```
Health probe tick (every 30s)
  │
  ▼
┌──────────────────────────────────────────────┐
│ H1. PROBE CONCURRENTLY                        │
│    tokio::join!(                              │
│      postgres_probe.check(),  → SELECT 1,     │
│        2s timeout                             │
│      redis_probe.check(),     → PING,         │
│        1s timeout                             │
│      qdrant_probe.check(),    → collection    │
│        info, 2s timeout                       │
│      ollama_probe.check(),    → model list,   │
│        3s timeout                             │
│    )                                          │
│                                              │
│ H2. COMPARE TO PREVIOUS STATE                 │
│    Read cached health from RwLock             │
│    For each dependency:                       │
│      current_state == previous_state?         │
│        → no change, skip                     │
│      current_state != previous_state?         │
│        → state transition detected            │
│                                              │
│ H3. STATE TRANSITION HANDLING                 │
│    Healthy → Degraded:                        │
│      Publish health.degraded_detected {       │
│        dependency, reason_code, latency_ms }  │
│      Write to health_events table             │
│    Degraded → Unavailable:                    │
│      Publish health.degraded_detected (again) │
│      Update severity to "critical"            │
│    Any → Healthy:                             │
│      Publish health.self_healed {             │
│        dependency, recovery_method }          │
│      Write to health_events table             │
│                                              │
│ H4. CACHE UPDATE                              │
│    Update RwLock<HealthState>                  │
│    30s TTL + 500ms jitter                     │
│    Used by compile_context response           │
│    health field                               │
│                                              │
│ H5. SELF-HEALING TRIGGER (if degradation)     │
│    Match reason_code → remediation catalog:   │
│    ┌──────────────────────────────────────┐   │
│    │ embedding_unavailable                │   │
│    │   → switch fallback provider          │   │
│    │ qdrant_collection_missing             │   │
│    │   → drop-if-exists + create           │   │
│    │ qdrant_orphan_vectors                 │   │
│    │   → purge by filter                   │   │
│    │ watcher_stale                         │   │
│    │   → force heartbeat + reconcile       │   │
│    │ graph_version_mismatch               │   │
│    │   → ESCALATE to admin (NEVER auto)    │   │
│    │ pg_connection_lost                    │   │
│    │   → reconnect pool + verify            │   │
│    │ outbox_backlog                        │   │
│    │   → drain at reduced rate             │   │
│    └──────────────────────────────────────┘   │
│    Execute remediation with:                  │
│      • Bounded retries: 3 attempts            │
│      • Exponential backoff: 1s, 2s, 4s       │
│      • Max total: 21s (3 attempts × max 7s)  │
│      • Audit: write per-attempt to            │
│        remediation_events table               │
│      • On success: publish                    │
│        health.self_healed                     │
│      • On failure (3 attempts):               │
│        publish health.remediation_failed      │
│        escalate to admin alert                │
│    Constitutional check: remediation          │
│    must not mutate skill content              │
│    (compile-time enforced via                 │
│    EntityType::SkillContent gate)             │
└──────────────────────────────────────────────┘
```

### Pipeline 4: Drift Sentinel (Periodic, 5min)

```
Drift check tick (every 5min)
  │
  ▼
┌──────────────────────────────────────────────┐
│ D1. PG↔QDRANT CONSISTENCY                     │
│    • PG: SELECT COUNT(*) FROM skills          │
│      WHERE lifecycle = 'active'               │
│    • Qdrant: count vectors per collection     │
│    • Compare: allowed drift ±2                │
│    • Content hash comparison (sample 10%):    │
│      PG skill content hash vs Qdrant          │
│      payload content hash. If any mismatch:   │
│      → orphan/missing detection               │
│                                              │
│ D2. VECTOR↔CONTENT CONSISTENCY                │
│    • Sample N skills (default 50)             │
│    • Regenerate embedding from content        │
│    • Cosine distance to stored embedding      │
│    • If distance > 0.1 (10% drift):           │
│      → potential embedding model drift        │
│      (model changed, corruption, etc.)        │
│    • CUSUM tracking:                          │
│      cusum_high = max(0, cusum_high +          │
│        (distance - target_mean) - k)          │
│      if cusum_high > h (4σ threshold):        │
│        → sustained drift alert                │
│                                              │
│ D3. FILESYSTEM↔GRAPH CONSISTENCY             │
│    • Walk .skills/ directories                │
│    • List all SKILL.md files                  │
│    • Compare to PG graph nodes                │
│    • Missing nodes → reconciliation gap       │
│    • Orphan nodes (in PG, not on disk)        │
│      → possible watcher event loss            │
│    • Quarantine: if node count diff >5%,      │
│      trigger reconciliation scan              │
│                                              │
│ D4. BEHAVIORAL CANARY (retrieval quality)     │
│    • Maintain 50-100 fixed canary queries     │
│      with known expected top-3 skill          │
│      rank positions                           │
│    • Run all canaries through full            │
│      retrieval pipeline                       │
│    • Compare output ranking to golden         │
│      baseline                                 │
│    • Any canary skill >1 position deviation   │
│      from expected rank → alert               │
│    • Two consecutive canary failures →        │
│      HIGH severity drift alarm                │
│    • Rebuild graph trigger: if 5+ canaries    │
│      fail in one pass                         │
│                                              │
│ D5. LIFECYCLE METADATA CONSISTENCY            │
│    • PG lifecycle field vs filesystem         │
│      extension (.md, .pending, .retired,      │
│      .optimized, .merge-proposal)             │
│    • Compare counts:                          │
│      PG active ≈ filesystem .md count         │
│      PG pending ≈ filesystem .pending count   │
│      PG retired ≈ filesystem .retired count   │
│    • Mismatch >5% → drift alarm               │
│    • Stale .pending files (warning_at          │
│      elapsed, not yet approved/denied) →      │
│      flag for human attention                 │
│                                              │
│ D6. ALARM & QUARANTINE                         │
│    If any check HIGH severity:                │
│    • Emit drift alarm event:                  │
│      { check_type, severity,                  │
│        detected_at, diagnostic_snapshot }     │
│    • Quarantine affected skills:              │
│      UPDATE lifecycle = 'drift_quarantine'    │
│      Excluded from retrieval results          │
│    • Quarantine is REVERSIBLE:                │
│      human admin or automatic on              │
│      drift clearance                          │
│    • Never auto-delete from PG/Qdrant         │
│      (quarantine excludes, doesn't remove)    │
│    • False positive tracking:                 │
│      FP rate must stay <5% over               │
│      rolling 24-hour window                   │
└──────────────────────────────────────────────┘
```

### Pipeline 5: Reconciliation (Periodic, 5min)

```
Reconciliation scan tick (every 5min)
  │
  ▼
┌──────────────────────────────────────────────┐
│ RC1. FULL FILESYSTEM SCAN                     │
│     Walk all .skills/ directories             │
│     (project + global + optional team)        │
│     Build map: file_path → (mtime, hash)      │
│                                              │
│ RC2. PG GRAPH COMPARISON                      │
│     Query all active skills from PG           │
│     Build map: skill_name → (version, hash)   │
│     Compare:                                  │
│       • Filesystem has file, PG has node →    │
│         verify hash match. If mismatch →      │
│         trigger rebuild                       │
│       • Filesystem has file, PG missing →     │
│         missed watcher event. Emit            │
│         skill.file_changed for the file        │
│       • PG has node, filesystem missing →     │
│         file was deleted. Update PG           │
│         lifecycle to "deleted"                │
│                                              │
│ RC3. QDRANT COMPARISON                        │
│     Query all vectors from Qdrant collections │
│     Compare vector IDs to PG skill IDs        │
│     • Orphan vectors (Qdrant, not PG) →       │
│       mark for cleanup, log                   │
│     • Missing vectors (PG, not Qdrant) →      │
│       outbox relay still pending or            │
│       lost write. Re-emit to outbox           │
│                                              │
│ RC4. EVENT OUTBOX DRAIN                       │
│     Query PG outbox: pending events           │
│     where created_at > 5min ago               │
│     Replay to Redis streams                   │
│     Mark as sent after ACK                    │
│                                              │
│ RC5. RECONCILIATION SUMMARY                   │
│     Log:                                      │
│     • skills_scanned: N                       │
│     • mismatches_detected: N                  │
│     • files_replayed_to_watcher: N            │
│     • orphan_vectors_cleaned: N               │
│     • outbox_events_replayed: N               │
│     • duration_ms                             │
│     If mismatches >0.5% of total:              │
│       emit drift alarm (low severity)          │
└──────────────────────────────────────────────┘
```

### Pipeline 6: Outcome-Based Learning (Periodic, 7 days)

```
Learning pass tick (every 7 days)
  │
  ▼
┌──────────────────────────────────────────────┐
│ L1. SIGNAL COLLECTION (30-day window)         │
│    Acceptance signals:                        │
│      .pending → .md rename event count        │
│    Rejection signals:                          │
│      .pending → .rejected count               │
│      Or .pending file >30 days unapproved     │
│    Usage signals:                             │
│      skill_usage rows with context_status=ok  │
│    Non-usage signals:                          │
│      Active skills with 0 usage in 30 days    │
│    Quality signals:                           │
│      Skills with quality >0.7 that failed     │
│      Skills with quality <0.3 that succeeded  │
│                                              │
│ L2. MINIMUM SIGNAL CHECK                       │
│    Total signals < 30 →                       │
│      skip tuning: "insufficient_data"         │
│    Total signals ≥ 30 → proceed to tune       │
│    Statistical power check:                   │
│      Wilson CI for binomial proportions       │
│      If CI width >20% → skip tuning           │
│                                              │
│ L3. THRESHOLD TUNING                          │
│    For each tunable threshold:                │
│    a) Candidate thresholds: grid search       │
│       extraction_quality_floor:               │
│         [0.3, 0.4, 0.5, 0.6]                 │
│       retirement_utility_threshold:           │
│         [0.10, 0.15, 0.20, 0.25]             │
│       quality_weight_in_retirement:            │
│         [0.1, 0.2, 0.3, 0.4]                 │
│    b) Retrospective simulation:               │
│       "If this threshold had been in place,    │
│        which skills would have been treated    │
│        differently?"                           │
│    c) Score each candidate by:                │
│       • F-measure of good/bad skill           │
│         classification                        │
│       • Proportion of known-good skills       │
│         that would survive                    │
│    d) Select best candidate                   │
│                                              │
│ L4. SANDBOX VALIDATION                        │
│    a) Hold-out 20% of signals (random,         │
│       time-based split — not recent)          │
│    b) Train thresholds on 80%                 │
│    c) Validate on held-out 20%                │
│    d) Must improve on baseline                │
│       (current thresholds)                    │
│    e) If no improvement → keep current        │
│       thresholds                              │
│                                              │
│ L5. REGRESSION GUARD (automatic, no override) │
│    a) Load all historically accepted skills   │
│       (skills that were approved by human     │
│        and never retired)                     │
│    b) Apply candidate thresholds              │
│    c) Count: how many would now be rejected   │
│       or retired that previously weren't?     │
│    d) If count >0 → BLOCK threshold change    │
│       This is automatic — no human override   │
│       Regression guard is absolute            │
│                                              │
│ L6. DEPLOY (if sandbox passes + no regression)│
│    a) Update learning_state singleton PG row: │
│       { threshold_name, old_value,            │
│         new_value, tuned_at,                   │
│         validation_score, signal_count }       │
│    b) Emit event: learning.thresholds_tuned   │
│    c) Audit trail: every tuning decision      │
│       recorded with reasoning                  │
│    d) Config files updated (filesystem-        │
│       observable, constitution §5)             │
│    e) Next pass: compare new thresholds       │
│       against updated data                    │
│                                              │
│ L7. DEGRADATION DETECTION                     │
│    If thresholds keep changing (oscillating):  │
│      → flag: "unstable_thresholds"             │
│      → freeze thresholds for 2 cycles         │
│    If quality scores stop predicting usage:   │
│      → alert: "quality_score_decay"            │
│      → needs rubric review/update              │
│    If acceptance rate drops:                  │
│      → alert: "extraction_quality_drop"       │
│      → recommend rubric review                │
└──────────────────────────────────────────────┘
```

## System Cadence Summary

```
        continuous  ────  watcher (inotify on .skills/)
                                      │
            30s     ────  health probes (PG, Redis, Qdrant, Ollama)
            30s     ────  self-healing (on health degradation event)
                                      │
           5min     ────  drift sentinel (5 checks)
           5min     ────  reconciliation scan (catch watcher misses)
                                      │
          30min     ────  merge proposals
          30min     ────  retirement proposals
                                      │
    per-session-end ────  extraction pipeline (3-8 min async, event-driven)
                                      │
per-trigger / weekly ────  SkillOpt optimization (20 min async, long-running)
                                      │
             7d     ────  outcome-based learning (threshold tuning)
                                      │
   on-promotion     ────  team index rebuild (event-driven)
```

**Concurrency rules:**
- Watcher: exclusive (only one watcher loop at a time)
- Health probes: concurrent with everything
- Self-healing: executes on health events, bounded to 3 attempts
- Merge + Retirement: same cron pass, sequential (retirement before merge)
- Drift sentinel: concurrent with everything (read-only)
- Reconciliation: runs between cron passes, exclusive (only one reconcilliation at a time)
- Extraction: concurrent with everything (separate async task)
- SkillOpt: concurrent with everything (separate service)
- Learning: exclusive (only one learning pass at a time)

**Overlap prevention:**
- Graph rebuild triggered by watcher → drift sentinel AND reconciliation both wait (version lock)
- Merge pass → watcher events during merge buffered, processed after merge completes
- Extraction running during merge → skills extracted with pre-merge graph version, merged later
- SkillOpt running during retirement → optimizer reads skill version at start (snapshot isolation)

## E2E Test Specification

Exhaustive end-to-end tests covering every stage of every pipeline.

### E2E-1: Extraction Pipeline (Full Lifecycle)

| # | Test | Setup | Assertion | Evidence |
|---|------|-------|-----------|----------|
| E2E-1.1 | Session-end trigger | Write fixture transcript (15 turns, mixed success/failure) | `extract_session` returns `run_id` with `status: "running"` within 200ms | Unit: `cargo test -p mcp-server -- extract_session_trigger` |
| E2E-1.2 | Preflight: transcript splitting | Fixture transcript | Transcript split into exactly 15 turns, each classified success/failure/unknown correctly | Unit: `cargo test -p session-extractor -- preflight_split` |
| E2E-1.3 | Preflight: rubric loading | Rubric file present | Rubric loaded, system prompt contains rubric text | Unit: `cargo test -p session-extractor -- rubric_loaded_in_prompt` |
| E2E-1.4 | Preflight: rubric missing fallback | Rubric file absent | Built-in minimal rubric used, warning logged | Unit: `cargo test -p session-extractor -- rubric_fallback` |
| E2E-1.5 | Map phase: success mode extraction | Success turn fixture | ModeSet contains ≥1 success_mode, 0 failure_modes | Unit: `cargo test -p session-extractor -- map_success_mode` |
| E2E-1.6 | Map phase: failure mode extraction | Failure turn fixture (compile error) | ModeSet contains ≥1 failure_mode with concrete mechanism | Unit: `cargo test -p session-extractor -- map_failure_mode` |
| E2E-1.7 | Map phase: partial failure recovery | 20 turns, 6 LLM timeouts simulated | ≥14 turns successful, result contains "partial_failure" warning, does NOT abort | Unit: `cargo test -p session-extractor -- map_partial_failure` |
| E2E-1.8 | Map phase: total failure fallback | All turns timeout | ExtractionError::MapPhaseFailed, falls back to single-pass extraction | Unit: `cargo test -p session-extractor -- map_total_failure_fallback` |
| E2E-1.9 | Reduce: intermediate merge | 15 ModeSets → group by 5 | 3 consolidated ModeSets, each merging ≤5 originals | Unit: `cargo test -p session-extractor -- reduce_intermediate` |
| E2E-1.10 | Reduce: rare pattern preserved | ModeSet with unique failure pattern (quality >0.7) | Pattern survives merge, NOT dropped as "too narrow" | Unit: `cargo test -p session-extractor -- reduce_rare_pattern_preserved` |
| E2E-1.11 | Reduce: generic advice dropped | ModeSet with "be careful" pattern | Generic pattern dropped, log: "dropped_generic_pattern" | Unit: `cargo test -p session-extractor -- reduce_generic_dropped` |
| E2E-1.12 | Reduce: final synthesis | 5 consolidated ModeSets | 1-3 skills produced via tool-calling, each 200-3000 chars | Unit: `cargo test -p session-extractor -- final_synthesis` |
| E2E-1.13 | Budget enforcement: max_skills | Synthesis produces 5 skills | Only top-3 kept (by confidence), 2 logged as skipped | Unit: `cargo test -p session-extractor -- budget_max_skills` |
| E2E-1.14 | Budget enforcement: max_chars | Skill at 3500 chars | Truncated at sentence boundary ≤3000 chars | Unit: `cargo test -p session-extractor -- budget_max_chars` |
| E2E-1.15 | Quality assessment: evaluator scores | 3 extracted skills | Each gets 4 dimension scores, combined_utility_score = avg of 4 | Unit: `cargo test -p session-extractor -- quality_assessment` |
| E2E-1.16 | Quality assessment: evaluator unavailable | Ollama evaluator down | Skills written without quality scores, warning logged | Unit: `cargo test -p session-extractor -- quality_no_evaluator` |
| E2E-1.17 | Output: .pending file created | 3 skills | 3 `.pending` files written, YAML frontmatter valid, all required fields present | Integration: `cargo test --test test_extract_session` |
| E2E-1.18 | Output: extraction_source metadata | Extraction complete | `.pending` frontmatter contains `extraction_source_harness: cc` and `extraction_source_model` | Integration: `cargo test --test test_extract_session -- metadata` |
| E2E-1.19 | Output: events published | Extraction complete | `extraction.completed` + `skill.quality_scored` (×3) in Redis streams | E2E: `docker compose -f docker-compose.test.yml up --abort-on-container-exit` |
| E2E-1.20 | Output: PG writes | Extraction complete | `session_logs` updated, `skills` rows inserted with quality_scores JSONB, `skill_usage` initial row | E2E: Docker Compose test |
| E2E-1.21 | Single-pass fallback path | Config: extraction_mode=single_pass | Skills extracted via single-pass with quality rubric, map-reduce skipped | Unit: `cargo test -p session-extractor -- single_pass_fallback` |
| E2E-1.22 | Provider switching | Config: provider=ollama | OllamaExtractor used, outputs match schema | Unit: `cargo test -p session-extractor -- provider_ollama` |
| E2E-1.23 | End-to-end: transcript → .pending | Full Docker Compose, fixture transcript | `.pending` files created, frontmatter valid, quality scores present, events published | E2E: `docker compose -f docker-compose.test.yml up --abort-on-container-exit` |

### E2E-2: Graph Builder (Watcher + Ingestion)

| # | Test | Setup | Assertion | Evidence |
|---|------|-------|-----------|----------|
| E2E-2.1 | New skill: .pending → .md rename | Create .pending file, rename to .md | `skill.file_changed` event, watcher detects, parses, embeds, inserts to PG+Qdrant, bumps graph version | E2E: Docker Compose test |
| E2E-2.2 | Skill update: modify .md content | Active skill, modify procedure text | `skill.file_changed` event, PG UPDATE, Qdrant UPSERT with new embedding, graph version bumped | E2E |
| E2E-2.3 | Skill deletion: remove .md file | Active skill, delete SKILL.md | `skill.file_changed` event, PG lifecycle = "deleted", Qdrant vector removed | E2E |
| E2E-2.4 | Invalid skill: bad YAML | SKILL.md with malformed frontmatter | ExtractionError::InvalidSkillFormat logged, no PG/Qdrant write, watcher skips (not crashes) | E2E |
| E2E-2.5 | Invalid skill: bad name format | Name with slashes | ExtractionError logged, skill skipped | E2E |
| E2E-2.6 | Embedding generation | Valid skill | 768-dim vector, cosine similarity to content re-embed < 0.05 | Unit: `cargo test -p graph-builder -- embedding_generation` |
| E2E-2.7 | Community assignment | Skill with tags | Skill assigned to correct HDBSCAN community + tag-based community | Unit: `cargo test -p graph-builder -- community_assignment` |
| E2E-2.8 | Graph version bump atomicity | 3 concurrent skill changes | Version increments by exactly 3, no gaps, no duplicates | Unit: `cargo test -p graph-builder -- version_atomicity` |
| E2E-2.9 | Retrieval cache invalidation | Graph version bump | Next compile_context call after version bump returns fresh results (not stale cache) | Integration: `cargo test --test test_compile_context -- cache_invalidation` |
| E2E-2.10 | Team scope promotion | .promote → .md in team scope dir | Provenance hash computed, skill_scopes junction table populated, remote Qdrant written | E2E |
| E2E-2.11 | Reconciliation: missed event catchup | Write .md file, kill watcher, restart | Reconciliation scan detects file, emits skill.file_changed, graph rebuilt correctly | E2E |
| E2E-2.12 | Debounce: rapid writes | Write same file 5 times in 100ms | Only 1 skill.file_changed event emitted (debounce 500ms) | Unit: `cargo test -p graph-builder -- watcher_debounce` |

### E2E-3: Maintenance — Merge Proposals

| # | Test | Setup | Assertion | Evidence |
|---|------|-------|-----------|----------|
| E2E-3.1 | Candidate discovery: high similarity | Two skills with cosine >0.85 | Pair discovered as merge candidate | Unit: `cargo test -p maintenance -- merge_candidate_discovery` |
| E2E-3.2 | Candidate discovery: low similarity | Two skills with cosine <0.85 | Pair NOT discovered, no merge proposal | Unit: `cargo test -p maintenance -- merge_low_similarity_skipped` |
| E2E-3.3 | Cross-tenant merge BLOCKED | Two skills in team scope, different origin_repo | Pair discovered BUT filtered out by cross-origin check | Unit: `cargo test -p maintenance -- merge_cross_tenant_blocked` |
| E2E-3.4 | Merge execution: LLM produces valid skill | Valid candidate pair | Merged skill has combined procedures, deduplicated conventions, union of tags | Unit: `cargo test -p maintenance -- merge_execution` |
| E2E-3.5 | Held-out validation: improvement | Merged skill improves 3 of 5 transcripts | Validation passes, proposal written | Unit: `cargo test -p maintenance -- merge_validation_pass` |
| E2E-3.6 | Held-out validation: degradation | Merged skill degrades on 2 of 5 transcripts | Validation fails, proposal NOT written, logged as rejected | Unit: `cargo test -p maintenance -- merge_validation_fail` |
| E2E-3.7 | Held-out validation: tie | Merged skill ties baseline | Validation FAILS (ties rejected per SkillLens finding) | Unit: `cargo test -p maintenance -- merge_validation_tie_rejected` |
| E2E-3.8 | Proposal output: .merge-proposal file | Validation passes | File written with merged_from, validation_scores in frontmatter | Integration |
| E2E-3.9 | Human approval: rename to .md | .merge-proposal present, rename it | Source skills retired (lifecycle: retired_by_merge), merged skill active | Integration |
| E2E-3.10 | Duplicate pairs skipped | Same pair already has active merge proposal | Pair NOT re-discovered | Unit: `cargo test -p maintenance -- merge_duplicate_skip` |
| E2E-3.11 | Merge pass metrics logged | Merge pass completes | candidates_found, merged, rejected counts all correct | Unit |

### E2E-4: Maintenance — Retirement Proposals

| # | Test | Setup | Assertion | Evidence |
|---|------|-------|-----------|----------|
| E2E-4.1 | Usage collection: unused skill | Skill with 0 usage in 90 days | usage_per_month = 0.0 | Unit: `cargo test -p maintenance -- retire_usage_zero` |
| E2E-4.2 | Usage collection: frequently used | Skill with 30 uses in 90 days | usage_per_month = 10.0 (capped at 1.0 for scoring) | Unit |
| E2E-4.3 | Utility scoring: high quality survives | Quality 0.8, 0 usage for 60 days | Skill flagged but NOT retired (quality >0.7 → 180-day grace) | Unit |
| E2E-4.4 | Utility scoring: low quality retires early | Quality 0.2, 0 usage for 35 days | Skill proposed for retirement (quality <0.3 → 30-day grace) | Unit |
| E2E-4.5 | Quality weight phasing: V2 launch | Config: phase=launch (usage 0.8, quality 0.2) | Higher quality has minimal retirement protection | Unit |
| E2E-4.6 | Confidence scaling: low samples | 5 usage samples | Utility score dampened by sqrt(5/30) = 0.41 | Unit |
| E2E-4.7 | Regression guard: no previously-accepted blocked | Simulate threshold change that would retire an accepted skill | Change BLOCKED, log recorded | Unit: `cargo test -p maintenance -- retire_regression_guard` |
| E2E-4.8 | Team scope: per-team-member aggregation | 3 team members, 1 uses skill, 2 don't | Skill NOT retired (at least one usage in 90 days) | E2E |
| E2E-4.9 | Team scope: zero all members | 3 team members, 0 usage for 180 days (low quality) | Skill proposed for retirement | E2E |
| E2E-4.10 | Cross-scope retirement: active in project, retired in team | Skill active in project scope, 0 team usage | Team scope only: retirement proposed. Project scope: unaffected | E2E |
| E2E-4.11 | .retired file with quality flag | Low quality retires | .retired frontmatter contains "possible_extraction_artifact" flag | Integration |

### E2E-5: SkillOpt Optimizer

| # | Test | Setup | Assertion | Evidence |
|---|------|-------|-----------|----------|
| E2E-5.1 | Prerequisites: insufficient data | Skill with 2 usage samples | OptimizationError::InsufficientData | Unit: `cargo test -p skill-optimizer -- prereq_insufficient_data` |
| E2E-5.2 | Prerequisites: adequate data | Skill with 10 usage samples, held-out data | Proceeds to rollout | Unit |
| E2E-5.3 | Rollout: success/failure batch | 40 transcripts, skill helps on 12 | success_batch = top-8, failure_batch = bottom-8 | Unit |
| E2E-5.4 | Reflect: identified patterns | 8 success + 8 failure minibatches | Reflection output contains ≥2 specific edits with confidence | Unit |
| E2E-5.5 | Edit: budget enforcement | 4 edits allowed, 6 proposed | Only top-4 (by confidence) applied, 2 skipped | Unit |
| E2E-5.6 | Edit: protected sections | Protected section marker, step-edit tries to touch | Edit REJECTED, log: "protected_section_violation" | Unit |
| E2E-5.7 | Edit: cosine schedule | epoch 0: budget=4, epoch 2: budget=2 | Budget correctly computed per epoch | Unit |
| E2E-5.8 | Gate: strict improvement | Candidate scores +0.05 over baseline | Accepted, best_skill updated | Unit |
| E2E-5.9 | Gate: tie rejection | Candidate scores 0.00 delta | Rejected (ties = noise, per SkillOpt paper) | Unit |
| E2E-5.10 | Gate: cross-consumer check | Claude +0.10, Ollama -0.06 (>5% regression) | Rejected (degraded one consumer beyond tolerance) | Unit |
| E2E-5.11 | Gate: cross-consumer minor regression | Claude +0.10, Ollama -0.02 (<5% regression) | Accepted (within tolerance) | Unit |
| E2E-5.12 | Rejected-edit buffer | Gate rejects 3 edits | Next reflection round: buffer prepended to prompt with "avoid these" | Unit |
| E2E-5.13 | Slow update (epoch 2+) | Protected sections, all consumers improve | Protected section updated, meta-skill state updated | Unit |
| E2E-5.14 | Full loop: 4 epochs converge | Skill with improvement potential | best_score improves over epoch 0 baseline | Integration: `cargo test --test test_optimization_loop` |
| E2E-5.15 | Full loop: no improvement possible | Skill at ceiling (all edits rejected by gate) | OptimizationRunStatus::NoImprovement, no .optimized file | Integration |
| E2E-5.16 | Output: .optimized file | Accepted candidate | File written, frontmatter complete (optimized_from, scores, epochs, per_consumer_scores) | Integration |
| E2E-5.17 | Output: event + PG | Optimization complete | skill.optimized event, optimization_runs row | E2E |
| E2E-5.18 | Concurrent optimization runs | 2 skills optimize simultaneously | Separate run_ids, separate files, no state contamination | Integration |

### E2E-6: Health & Self-Healing

| # | Test | Setup | Assertion | Evidence |
|---|------|-------|-----------|----------|
| E2E-6.1 | All probes healthy | All services running | compile_context health field: all status=ok | Integration |
| E2E-6.2 | Degradation detection: PG down | Kill PG container | health.degraded_detected event with reason_code=pg_connection_lost | E2E |
| E2E-6.3 | Degradation detection: Qdrant down | Kill Qdrant container | health field reflects: qdrant=unavailable, others=ok | E2E |
| E2E-6.4 | Recovery detection: restore PG | Restart PG container | health.self_healed event with dependency="postgres" | E2E |
| E2E-6.5 | Self-healing: reconnect pool | PG connection lost | attempts=1, remediation="reconnect_pool", health=recovered | E2E |
| E2E-6.6 | Self-healing: max retry limit | PG connection lost, 3 reconnections fail | remediation_failed event, escalation to admin alert | E2E |
| E2E-6.7 | Self-healing: idempotent remediation | qdrant_collection_missing, run twice | Second run is no-op (collection already created), no error | E2E |
| E2E-6.8 | Self-healing: graph_version_mismatch escalation | Simulate mismatch | ESCALATED to admin (never auto-healed), remediation_failed event | E2E |
| E2E-6.9 | Self-healing: skill content protection | Malicious remediation catalog entry tries to mutate skill | Compile-time rejection (EntityType::SkillContent gate, CI enforces) | Unit: compile test |
| E2E-6.10 | Audit trail completeness | 3 remediation cycles | remediation_events: 3 rows × 3 attempts each = 9 rows, all with correlation_id | E2E |
| E2E-6.11 | Health caching | Probe results, then wait 15s (before 30s refresh) | compile_context returns cached health (not re-running probes) | Unit |
| E2E-6.12 | Jitter prevents thundering herd | 5 compile_context calls simultaneously | Health probes run once (not 5 times), same cached result served | Integration |

### E2E-7: Drift Sentinel

| # | Test | Setup | Assertion | Evidence |
|---|------|-------|-----------|----------|
| E2E-7.1 | PG↔Qdrant: healthy | All stores in sync | Drift check passes, no alarm | E2E |
| E2E-7.2 | PG↔Qdrant: skill missing from Qdrant | Delete one Qdrant vector | Drift alarm emitted (severity: low), skill marked for outbox replay | E2E |
| E2E-7.3 | Vector↔content: embedding drift | Corrupt 3 embeddings (flip bits) | CUSUM detects sustained drift (4σ), alarm emitted (severity: medium) | E2E |
| E2E-7.4 | Filesystem↔graph: watcher missed event | Create .md file, don't trigger watcher | Reconciliation gap detected by FS↔graph check, alarm emitted | E2E |
| E2E-7.5 | Behavioral canary: ranking stable | All 50 canary queries | 0 failures, 0 alarms | E2E |
| E2E-7.6 | Behavioral canary: ranking drift | Corrupt 3 embedding vectors | Canary queries fail >5, alarm emitted (severity: HIGH) | E2E |
| E2E-7.7 | Lifecycle metadata: pending files stale | 3 .pending files >30 days old | Alarm emitted (severity: low), files flagged for human attention | E2E |
| E2E-7.8 | Quarantine: drift skills excluded | HIGH severity drift → quarantine | Quarantined skills not in compile_context results, quarantined count visible in admin tool | E2E |
| E2E-7.9 | Quarantine: reversible | Clear drift condition | Admin tool `clear_quarantine` → skills reappear in retrieval | E2E |
| E2E-7.10 | Quarantine: no data deletion | Quarantine 5 skills | PG rows still exist (lifecycle=drift_quarantine), Qdrant vectors still present | E2E |
| E2E-7.11 | False positive rate | Healthy system, 24 hours | Drift alarms <5% false positive rate (measured: FP alarms / total checks) | E2E |

### E2E-8: Outcome-Based Learning

| # | Test | Setup | Assertion | Evidence |
|---|------|-------|-----------|----------|
| E2E-8.1 | Insufficient signals: skip | 15 signals in 30 days (<30 minimum) | Learning pass skips with "insufficient_data", no tuning | Unit: `cargo test -p maintenance -- learning_insufficient_data` |
| E2E-8.2 | Adequate signals: tuning proceeds | 50 signals in 30 days | Candidate thresholds computed, sandbox validated | Unit |
| E2E-8.3 | Sandbox: no improvement | Candidate thresholds fail validation | Current thresholds retained, log: "no_improvement_sandbox" | Unit |
| E2E-8.4 | Sandbox: improvement | Candidate thresholds improve F-measure by +0.05 | Candidate advances to regression guard | Unit |
| E2E-8.5 | Regression guard: blocks regression | Candidate would retire 2 previously-accepted skills | Change BLOCKED, log recorded, current thresholds retained | Unit |
| E2E-8.6 | Regression guard: no regression | Candidate retires 0 previously-accepted skills | Change applied, learning_state updated | Unit |
| E2E-8.7 | Deployment: learning_state updated | Thresholds changed | PG singleton row updated with old_value/new_value/tuned_at | Integration |
| E2E-8.8 | Deployment: filesystem observable | Thresholds changed | Config file written with new threshold values (constitution §5) | Integration |
| E2E-8.9 | Degradation: quality score decay | Quality scores stop predicting usage (r < 0.2) | Alert emitted: "quality_score_decay", rubric review recommended | Unit |
| E2E-8.10 | Degradation: oscillating thresholds | 3 consecutive pass cycles with threshold changes | Alert emitted: "unstable_thresholds", frozen for 2 cycles | Unit |
| E2E-8.11 | 30-day window: correct trailing | Insert signals at days 1, 15, 31, 35 | Only signals from days 5-35 (last 30) counted, day-1 signal excluded | Unit |
| E2E-8.12 | DS-024 contract: learning loop | Full simulated 30-day window with acceptance/rejection/usage signals | DS-024 test un-ignored and passing | E2E: `cargo test --test test_dream_state_contract -- ds_024` |

### E2E-9: Cross-Cutting Integration

| # | Test | Setup | Assertion | Evidence |
|---|------|-------|-----------|----------|
| E2E-9.1 | Full data plane: extract → score → build → retrieve → compile → explain | Fixture transcript | End-to-end: transcript in, explanation out. Every stage produces correct output | E2E: `docker compose -f docker-compose.test.yml up --abort-on-container-exit` |
| E2E-9.2 | V1.1 backward compatibility: compile_context | V2 schema, V1.1 query patterns | Same status codes, response shape, <500ms latency | E2E |
| E2E-9.3 | V1.1 backward compatibility: event catalog | V2 events published alongside V1.1 events | V1.1 events unchanged in schema and semantics | E2E |
| E2E-9.4 | V1.1 backward compatibility: scope persistence | V2 schema, V1.1 scope column reads | skills.scope still works, V1.1 code paths unchanged | E2E |
| E2E-9.5 | Concurrent: extraction + retrieval | Extract session while compile_context runs | No race conditions, no deadlocks, no stale data | E2E |
| E2E-9.6 | Concurrent: optimization + retrieval | SkillOpt running while compile_context runs | Optimizer works on snapshot, retrieval reads current version (no lock contention) | E2E |
| E2E-9.7 | Concurrent: merge + retirement | Merge and retirement pass overlap | Retirement before merge (sequential cron pass), no skill retired during active merge | E2E |
| E2E-9.8 | Graceful shutdown: SIGTERM | Send SIGTERM to all services | Active operations complete within 10s, no partial writes, no corruption | E2E |
| E2E-9.9 | Crash recovery: kill mid-extraction | Kill session-extractor during map phase | On restart: in-progress run marked as failed, no partial .pending files, no orphan Qdrant vectors | E2E |