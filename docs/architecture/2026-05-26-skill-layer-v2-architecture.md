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