---
title: feat: Dynamic Agent Skill Layer V2 — Quality Intelligence, Team Scope, and Self-Evolving Graph
type: feat
status: active
date: 2026-05-26
topic: skill-layer-v2
constitution_version: 1.0.0
constitution_waivers: []
brainstorm_ref: null
architecture_ref: docs/architecture/2026-05-26-skill-layer-v2-architecture.md
v1_1_architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
assessment_ref: docs/assessments/2026-05-26-skill-layer-v1-1-deep-grok-assessment.md
research_inputs:
  - SkillLens (arXiv:2605.23899) — Microsoft Research + Fudan Univ: systematic study of model-generated agent skills, three validated quality dimensions, map-reduce extraction architecture
  - SkillOpt (arXiv:2605.23904) — Microsoft Research: text-space optimization loop for frozen-agent skills, rollout→reflect→edit→gate→deploy cycle
  - SkillRAE (arXiv:2605.10114) — multi-level skill graph scoring formula, MMR+RRF fusion foundation
source_docs:
  tickets: []
  docs: []
  figma: []
  plans: []
handoff:
  problem_narrative: true
  user_story: true
  architectural_context: true
  success_criteria: true
tdd:
  precedence: plan_overrides_local
  mode: ralph
  loop: red-green-refactor
  evidence:
    unit: required
    e2e: required
  exceptions: []
execution_shape:
  mode: vertical-slices
  rationale: |
    Every phase delivers a user-visible, constitution-compliant tracer bullet.
    Phase 1 proves quality-scored extraction with SkillLens rubric.
    Phase 2 proves team scope with remote resolvers.
    Phase 3 proves autonomous optimization loop.
    Phase 4 proves counterfactual explainability and causal tracing.
    Phase 5 proves multi-harness portability.
    Vertical slices keep feature homes clean and succeeded criteria separately testable.
---

# feat: Dynamic Agent Skill Layer V2 — Quality Intelligence, Team Scope, and Self-Evolving Graph

## Enhancement Summary

**Prerequisite:** V1.1 complete (T01-T14 all green). V2 builds on the 9-crate architecture, 12 PG tables, 8-event catalog, 7 MCP tools, and 5-container Docker Compose topology delivered by V1.1.

**Scope posture:** V2 widens depth (extraction quality, LLM compilation, self-evolution) and breadth (team scope, multi-harness compilers, counterfactual explainability) without architectural rewrite. All V1.1 contracts remain intact. All V2 additions are additive migrations: new columns, new traits, new resolvers, new compilers — never a breaking change to the retrieval path, event catalog, or MCP tool surface.

**Research foundation:** SkillLens (arXiv:2605.23899) proves three quality dimensions predict skill utility with 64-66% accuracy per dimension and +1.55pp average gain when applied as a meta-skill rubric. SkillOpt (arXiv:2605.23904) proves text-space optimization of skills for frozen agents produces 12.8–24.9pp average gains across 7 target models and 6 benchmarks. Both are directly applicable to our extraction and maintenance pipelines through existing trait boundaries.

**V1.1 work explicitly NOT duplicated here:** T11 (graceful degrade), T12 (session persistence), T13 (logging/benchmarks/docs), T14 (live data-plane E2E) — all remain V1.1 Phase 3 hardening tickets and are assumed complete before V2 execution begins.

**Estimated total units:** 38 slices across 5 phases. 60-90 execution hours.

### Key Innovations Over V1.1

1. **Quality-scored extraction with SkillLens rubric** (serves: new SC-V2-1) — prepend 3-dimension quality meta-skill to extraction prompts, parse quality self-assessment, score skills by utility likelihood rather than syntactic completeness. Zero architecture changes, 60 minutes of string prepend.

2. **Map-Reduce parallel extraction** (serves: SC-3, new SC-V2-1) — per-trajectory mode extraction → hierarchical merge → tool-calling synthesis. The trait boundary (`TranscriptSkillExtractionService`) is already correct; this fills in the stub implementation with an architecture proven to improve extraction quality.

3. **LLM-synthesized context compilation** (serves: SC-1, SC-6) — V1.1's `TemplateOnlyCompiler` gave us sub-500ms latency. V2 adds `OllamaGuidanceCompiler` behind the existing `ContextCompiler` trait for sessions that can afford 2-3s compilation in exchange for richer context synthesis.

4. **Team scope with remote PG+Qdrant** (serves: new SC-V2-2) — V1.1 deferred `skill_scopes` junction table and `RemoteTeamScopeResolver` to V2. This is that migration. Additive: new resolver, new config, same retrieval pipeline.

5. **Utility-scored maintenance** (serves: SC-4, new SC-V2-3) — retirement scoring combines usage frequency with extraction quality dimensions. High-quality low-usage skills survive retirement longer. Quality data flows from extraction through ingestion to maintenance.

6. **SkillOpt-style validation gates** (serves: new SC-V2-3, new SC-V2-4) — before merge proposals are written, proposed merged skills are test-compiled against held-out session transcripts. Only promotion-worthy merges become `.pending` proposals.

7. **Autonomous self-healing** (serves: SC-7, new SC-V2-5) — the system detects known degraded reason codes from DS-003's chaos matrix, selects policy-safe remediations, and executes bounded repair actions. Every repair is auditable and reversible.

8. **Counterfactual explainability** (serves: new SC-V2-6) — every `compile_context` response includes ranked rationale, feature contribution scores, and minimal prompt perturbations that would alter ranking. DS-018 is the contract.

9. **Outcome-based learning** (serves: new SC-V2-4) — acceptance/rejection signals from human approval, skill usage patterns, and retrieval utility feedback tune extraction prompts and retirement thresholds over time. DS-024 is the contract.

10. **Multi-harness compilers** (serves: SC-8, new SC-V2-7) — `OpenCodeCompiler`, `CopilotCompiler`, `CodexCompiler` alongside `TemplateOnlyCompiler` and `OllamaGuidanceCompiler`. Same retrieval pipeline, harness-specific formatting.

## Problem Narrative

V1.1 delivers a working skeletal system: skills are extracted from sessions, stored in a graph, retrieved by relevance, and maintained through merge/retire workflows. But the skeleton has three critical gaps that prevent it from being truly useful:

First, **extraction is blind**. The extractor produces skill candidates with no signal about whether they'll actually help. SkillLens proved that LLM-generated skills cause negative transfer in 25% of extractor-target pairs, and that LLM judges cannot distinguish good from bad skills (46.4% accuracy). V1.1's extraction trusts the model blindly. We need quality dimensions that predict downstream utility.

Second, **the system is solo-only**. V1.1's constitution explicitly defers team scope to V2. A solo developer's skills should compound across their machine, but a team's collective intelligence — skills learned by one developer that help another on a different repo — remains locked in per-machine silos. Cross-repo collective intelligence (DS-017) is impossible without a shared scope.

Third, **the system is static**. V1.1 maintains skills through deduplication and retirement, but never improves them. SkillOpt proved that skills can be optimized through a training loop — rollout → reflect on failures → bounded edits → validation gate — achieving 12.8-24.9pp improvements across 7 models and 6 benchmarks without touching model weights. V1.1's maintenance deletes stale skills. V2 should evolve them.

Beyond these three gaps, V1.1's architecture deliberately left seams for V2: `ContextCompiler` trait supports LLM-guided compilation, `ScopeResolver` trait supports remote team scope, `MergeSemanticVerifier` supports held-out validation. Filling these seams is additive migration, not rewrite.

## User Story

As a solo developer who is now part of a team,
I need skills extracted from my sessions to be quality-scored so bad skills never pollute the graph, skills from teammates' repos to be discoverable through a shared team scope, and the system to actively improve my skills over time through validated optimization,
so that every session starts with higher-quality context than the last, skills compound across the team, and the graph gets smarter with use rather than just accumulating drift,
because currently V1.1 extracts blindly (25% of skills may be harmful), operates in per-machine isolation (team knowledge is locked in silos), and only deletes stale skills rather than improving the ones we keep,
which causes extraction distrust, zero cross-repo intelligence, and a graph that rots slightly slower but never gets better.

### Secondary Story: Operator with Explainability Needs

As a developer debugging why a particular skill wasn't retrieved for a task,
I need counterfactual explanations showing why selected skills won, which skills were close but lost, and what minimal prompt changes would have altered the ranking,
so that I can understand and tune the retrieval pipeline without guessing at vector spaces or scoring formulas,
because currently V1.1 returns ranked skills with scores but no explanation of feature contributions or ranking sensitivity.

### Tertiary Story: Multi-Harness Developer

As a developer using both Claude Code and OpenCode (or Copilot, or Codex),
I need the same skill graph to compile context in harness-appropriate formats, so skills built in one harness transfer seamlessly to another,
because V1.1 compiles only for Claude Code's `additionalContext` format, leaving skills ported to other harnesses unusable at injection time.

## Architectural Context

### Complexity Justification

This plan requires A LOT detail level because:

1. **Five-phase multi-crate expansion across a 9-crate distributed system.** Each phase touches multiple feature homes (domain types, infrastructure adapters, retrieval pipeline, compilation, maintenance policy, and MCP tool surface). A simpler plan format would obscure cross-crate sequencing and trait-boundary dependencies.

2. **Three external research papers drive architectural decisions.** SkillLens defines the quality rubric and map-reduce extraction architecture. SkillOpt defines the optimization loop and validation gates. SkillRAE defines the scoring formula foundation. Each finding maps to a specific seam in our architecture — the plan must trace paper to code path explicitly.

3. **Team scope requires additive schema migration.** Adding true many-to-many scope membership, remote PG+Qdrant URLs, and cross-tenant isolation guarantees touches the PG schema (new junction table), the retrieval pipeline (new scope in dual-scope search), the graph-builder (remote index construction), and the MCP server (new scope in compile_context). Every touch must be explicit about backward compatibility.

4. **Self-evolving systems require safety guardrails.** Autonomous self-healing and outcome-based learning touch the human-gate principle (constitution §3) — the plan must trace every autonomous action back to constitutional compliance and show which actions are safe to automate without human approval.

5. **Multi-harness compilation is a cross-cutting concern.** Five compiler implementations, harness-specific MCP hook formats, and portability verification across Claude Code, OpenCode, Copilot, and Codex. The plan must show the compiler trait boundary holds across all harnesses.

### System Placement

- **Lives in:** `dynamic-agent-skill-layer/` root, deployed via Docker Compose (same as V1.1). New service: `skill-optimizer` (SkillOpt loop runner). New crate: `crates/explainability/` (counterfactual reasoning). All other additions are in existing crates.
- **Feature homes touched:**
  - `crates/domain/` — new types (QualityScores, CounterfactualExplanation, TeamScope, HarnessFormat), new traits (HealthProbe, OptimizationOrchestrator)
  - `crates/infrastructure/` — new adapters (RemoteTeamScopeResolver, RemoteQdrantClient, RemotePgPool), quality rubric loader, health probes
  - `crates/retrieval/` — team scope concurrent search, counterfactual explanation generation
  - `crates/compiler/` — OllamaGuidanceCompiler, OpenCodeCompiler, CopilotCompiler, CodexCompiler
  - `crates/mcp-server/` — three new tool handlers (explain_ranking, trigger_optimization, get_skill_quality), team scope in compile_context
  - `crates/session-extractor/` — map-reduce extraction, quality rubric prepend, quality self-assessment parsing
  - `crates/graph-builder/` — remote team index construction, cross-tenant isolation checks
  - `crates/maintenance/` — utility-scored retirement, held-out validation gates, outcome-based threshold tuning
  - `crates/admin/` — new admin tools (list_team_scopes, inspect_quality_scores, trigger_self_heal)
  - **NEW crate: `crates/explainability/`** — counterfactual perturbation, feature contribution scoring, ranked rationale generation
  - **NEW service: `skill-optimizer`** — SkillOpt training loop (rollout → reflect → edit → gate → deploy)
- **Interacts with:**
  - **V1.1 infrastructure unchanged:** PostgreSQL (local + optional team remote), Redis (local), Qdrant (local + optional team remote), Ollama (local), Docker Compose
  - **New external dependencies:** Optional remote PG+Qdrant URLs for team scope. Optional stronger optimizer model for SkillOpt loop. No new infrastructure containers required beyond the optimizer service.
  - **Claude Code, OpenCode, Copilot, Codex:** compilation formats for each harness's context injection mechanism. MCP protocol is the common transport.
- **Constitution compliance:** All five principles remain intact. Local-first stays default (team scope is opt-in). Human gate for mutations stays absolute (autonomous self-healing operates on degraded-state recovery, not skill content). Portable scope extends to harness compilation. Filesystem-observable state stays — `.quality` metadata files alongside `.pending`/`.retired`.
- **Boundary constraints:** Must NOT require cloud services by default (team scope is opt-in remote, local-only works without it). Must NOT auto-approve skill mutations. Must NOT change V1.1 retrieval semantics for existing scopes. Must NOT break V1.1 PG schema — only additive migrations. Must NOT increase `compile_context` baseline latency above 500ms for template compilation path.

## Success Criteria

- [ ] **SC-V2-1: Quality-scored extraction** — every extracted skill carries a `combined_utility_score` derived from failure-mechanism encoding, actionable specificity, and high-risk avoidance. Skills scoring below threshold are flagged in `.pending` frontmatter.
- [ ] **SC-V2-2: Team scope retrieval** — a developer who configures a remote PG+Qdrant URL can retrieve skills from the shared team scope alongside their project and global scopes, with strict isolation preventing cross-tenant leakage.
- [ ] **SC-V2-3: Utility-scored maintenance** — retirement decisions combine usage frequency (60%) with extraction quality (40%). High-quality low-usage skills survive retirement. Merge proposals are gated by held-out validation testing.
- [ ] **SC-V2-4: Outcome-based learning** — acceptance/rejection signals from human approval and skill usage patterns tune extraction quality thresholds and retirement scoring windows over a 30-day learning horizon, with regression guards preventing quality degradation.
- [ ] **SC-V2-5: Autonomous self-healing** — the system detects known degraded reason codes (embedding_unavailable, qdrant_timeout, watcher_stale), selects policy-safe remediations from a catalog, and executes bounded repair actions with full audit trail.
- [ ] **SC-V2-6: Counterfactual explainability** — `explain_ranking` returns per-skill feature contributions, sensitivity analysis showing what minimal prompt changes would alter ranking, and ranked rationale in machine-parseable JSON.
- [ ] **SC-V2-7: Multi-harness portability** — skills compiled for Claude Code, OpenCode, Copilot, and Codex produce harness-appropriate context formats. The same retrieval output produces correct injection for each harness.
- [ ] **SC-V2-8: LLM-guided compilation** — an `OllamaGuidanceCompiler` behind the `ContextCompiler` trait synthesizes task-specific guidance from retrieved skills in under 3 seconds, selectable per-session via config toggle.
- [ ] **SC-V2-9: SkillOpt optimization loop** — the system can run rollout → reflect → edit → gate cycles for a target skill, producing validated improvements. The optimizer produces `.optimized` proposals requiring human approval (constitution §3).
- [ ] **SC-V2-10: Dream-state contracts fulfilled** — at least 12 of 24 DS contracts (DS-003, DS-008, DS-012, DS-014, DS-015, DS-016, DS-017, DS-018, DS-019, DS-020, DS-022, DS-024) are un-ignored and passing.

## TDD & Evidence Contract

- **Effective mode:** Ralph-driven TDD
- **Effective loop:** red-green-refactor (failing tests first → minimal implementation → refactor → post-refactor rerun)
- **Unit evidence:** Required for all domain types, scoring functions, quality rubric parsing, counterfactual perturbation, health probe implementations, and compiler format outputs. Command: `cargo test --workspace`
- **E2E evidence:** Required per phase for the full Docker Compose data plane: extraction → quality scoring → graph ingestion → retrieval → compilation → explain. Command: `docker compose -f docker-compose.test.yml up --abort-on-container-exit`
- **Exceptions:** None. Both unit and e2e evidence required throughout.
- **Precedence:** Plan overrides `compound-engineering.local.md`. This plan mandates Ralph TDD with full unit+e2e evidence.

## Execution Shape

- **Mode:** vertical-slices
- **Why:** Every phase delivers a user-visible outcome. Phase 1 produces quality-scored `.pending` files. Phase 2 produces team-scope retrieval. Phase 3 produces optimized skill proposals. Phase 4 produces counterfactual explanations. Phase 5 produces harness-formatted compilations. Each phase is independently testable and deployable.

## Constitution Alignment

- **Constitution version:** 1.0.0
- **Relevant principles:** All five apply:
  - §1 Local-First Execution — team scope is opt-in remote; local-only path remains default and fully functional
  - §2 Zero-Touch Session Start — template compilation stays <500ms; LLM compilation runs behind config toggle
  - §3 Human Gate for Mutations — autonomous self-healing is restricted to degraded-state recovery (not skill content mutation); SkillOpt produces `.optimized` proposals requiring human approval; quality scoring flags skills but does not auto-reject
  - §4 Portable Scope — multi-harness compilers maintain `SKILL.md` as universal interchange; scope is a retrieval boundary
  - §5 Filesystem-Observable State — `.quality` metadata files alongside `.pending`/`.retired`/`.optimized`; all mutations visible
- **Applicable baselines:** Full CI bar (clippy strict, rustfmt, tests, benchmarks), clean code (vertical slices, SOLID, DRY, no stubs), Docker Compose deployability, domain crate zero infra deps
- **Required approvals:** Schema migration (new columns, new junction table), new event types (`skill.optimized`, `skill.quality_scored`, `health.degraded_detected`, `health.self_healed`), Ollama model for guidance compiler, Redis event contract changes, optional remote PG+Qdrant config. All handled by Docker Compose env vars + filesystem proposals.
- **Waivers:** None. All five principles apply without exception. Autonomous self-healing is bounded to non-mutation operations.

## Stakeholder Impact

- **End user (developer):** Every extracted skill carries a quality score they can trust (or ignore). Team skills compound across repos. Skills improve over time instead of just being cleaned up. They can ask "why wasn't skill X retrieved?" and get a counterfactual answer. They can use any harness and get correctly formatted context.
- **End user (team lead):** Team scope with strict isolation means shared intelligence without cross-contamination. Quality scoring means bad skills don't spread across the team. Optimization proposals are human-gated — no surprise skill changes.
- **Developers (this codebase):** Eleven crates with explicit feature homes. One new crate (`explainability`), one new service (`skill-optimizer`). All trait boundaries from V1.1 are preserved. Additive migrations only. V1.1 code paths unchanged.
- **Operations:** Optional remote PG+Qdrant deployment for team scope. No new mandatory infrastructure. Health probes cover remote dependencies. Degraded semantics distinguish local-from-remote failures.

## Technical Approach

### Architecture Overview

V2 extends the 9-crate V1.1 architecture with additive changes:

```
crates/
├── domain/           # +QualityScores, +CounterfactualExplanation, +TeamScope, +HarnessFormat, +HealthProbe trait
├── infrastructure/   # +RemoteTeamScopeResolver, +QualityRubricLoader, +HealthProbes (PG/Redis/Qdrant/Ollama)
├── retrieval/        # +Team scope concurrent search, +Counterfactual explanation generation
├── compiler/         # +OllamaGuidanceCompiler, +OpenCodeCompiler, +CopilotCompiler, +CodexCompiler
├── mcp-server/       # +explain_ranking, +trigger_optimization, +get_skill_quality tool handlers
├── session-extractor/ # Map-reduce extraction, quality rubric prepend, self-assessment parsing
├── graph-builder/    # +Team scope index construction, +Cross-tenant isolation guards
├── maintenance/      # +Utility-scored retirement, +Held-out validation gates, +Outcome-based threshold tuning
├── admin/            # +list_team_scopes, +inspect_quality_scores, +trigger_self_heal
├── explainability/   # NEW: Counterfactual perturbation, feature contributions, rationale generation
└── skill-optimizer/  # NEW service crate: SkillOpt loop runner
```

**PG schema additions (migration 002):**
- `skills.quality_scores JSONB` — per-skill quality dimensions from extraction
- `session_logs.success_ratio FLOAT` — proportion of successful turns in source session
- `skill_scopes` junction table — many-to-many skill-to-scope for team scope
- `optimization_runs` table — SkillOpt run history with before/after snapshots
- `learning_state` singleton — outcome-based threshold tuning state
- `health_events` table — degraded-state detection and self-healing audit trail

**Event catalog additions (4 new events):**
- `skill.quality_scored` — emitted when extraction assigns quality dimensions
- `skill.optimized` — emitted when SkillOpt produces an optimized proposal
- `health.degraded_detected` — emitted when health probes detect degradation
- `health.self_healed` — emitted when autonomous recovery completes

### Execution Slices

#### Phase 1: Quality Intelligence
**Purpose:** Inject SkillLens-proven quality awareness into extraction and compilation. The extraction pipeline goes from blind stub to quality-scored map-reduce architecture. Compilation gains LLM-guided synthesis behind the existing `ContextCompiler` trait. This phase delivers the biggest quality-per-engineering-hour ratio in the entire V2 plan.
**Rationale:** SkillLens provides battle-tested extraction architecture and a validated quality rubric. Applying them through existing trait boundaries (`TranscriptSkillExtractionService`, `ContextCompiler`) is zero-risk and high-impact. The quality rubric alone (+1.55pp across 9/9 domain×target cells) requires 60 minutes of string prepend.

##### Slice 1.1: Quality rubric integration into extraction prompts
**Slice type:** tracer-bullet
**Serves:** SC-V2-1 (quality-scored extraction)
**Demo scenario:** Extract a skill from a fixture transcript and verify the `.pending` file contains `quality_scores` in its YAML frontmatter with `failure_mechanism_score`, `actionable_specificity_score`, and `high_risk_avoidance_score` fields.
**Feature home:** `crates/session-extractor/`
**Files:**
- `crates/session-extractor/src/meta_skills/quality_rubric.md` — NEW: SkillLens 3-dimension rubric adapted for coding-agent domains
- `crates/session-extractor/src/providers/claude.rs` — prepend rubric to system prompt
- `crates/session-extractor/src/providers/ollama.rs` — same
- `crates/session-extractor/src/lib.rs` — parse quality self-assessment from extraction output
- `crates/domain/src/types.rs` — add `QualityScores` struct to `ExtractedSkillCandidate`
- `tests/integration/test_extract_session.rs` — verify quality scores in output
**Depends on:** None (V1.1 trunk)
**Dependency type:** real

###### What to build
Create `crates/session-extractor/src/meta_skills/quality_rubric.md` from the SkillLens `quality_rubric_3dim.md`, adapted for coding-agent domains (Rust tooling, git workflows, Docker, testing, refactoring) rather than ALFWorld/SpreadsheetBench domains. The rubric contains exactly three sections: Failure Mechanism Encoding, Actionable Specificity, High-Risk Action Blacklist — each with coding-domain examples and anti-examples.

Prepend the rubric content to the system prompt in both `ClaudeExtractor` and `OllamaExtractor`. Add parsing logic for a `quality_self_assessment` JSON block in the extraction response. Add `QualityScores { failure_mechanism_score, actionable_specificity_score, high_risk_avoidance_score, combined_utility_score }` to `ExtractedSkillCandidate` in domain types. The `combined_utility_score` is the unweighted average of the three dimensions.

Write the quality scores into `.pending` file YAML frontmatter alongside existing `warning_at`/`expires_at` metadata.

###### Scope
- **Owns:** Quality rubric file, prompt prepend, self-assessment parsing, domain type addition, frontmatter writing
- **Non-goals:** Map-reduce extraction architecture, quality-aware maintenance scoring, schema migration for quality columns. Those are follow-on slices.
- **Scope fence:** Do not change extraction output format — only add quality metadata. Do not change provider routing. Do not add quality-based filtering (that needs data before we can set thresholds).

###### Acceptance criteria
- [ ] `quality_rubric.md` exists with three dimensions, coding-domain examples, and anti-examples
- [ ] Both Claude and Ollama extractors prepend rubric to system prompt
- [ ] Extraction output includes `quality_scores` JSON block parsed into `QualityScores`
- [ ] `.pending` file frontmatter includes `quality_scores` fields
- [ ] `cargo test -p session-extractor` passes with quality parsing tests
- [ ] Integration test verifies quality scores appear in extracted `.pending` file

###### Evidence
- **Test command:** `cargo test -p session-extractor && cargo test --test test_extract_session`
- **Evidence focus:** Quality scores populated in extraction output, rubric content correct, frontmatter fields present

##### Slice 1.2: Map-Reduce extraction architecture
**Slice type:** expansion
**Serves:** SC-V2-1, SC-3 (improved extraction quality through SkillLens map-reduce pattern)
**Demo scenario:** Extract skills from a 20-trajectory session transcript and compare single-pass vs map-reduce extraction output. Map-reduce produces skills with more concrete failure mechanisms and actionable specificity.
**Feature home:** `crates/session-extractor/`
**Files:**
- `crates/session-extractor/src/extraction/map_phase.rs` — NEW: per-trajectory ModeSet extraction (success_modes + failure_modes)
- `crates/session-extractor/src/extraction/reduce_phase.rs` — NEW: hierarchical merge + tool-calling synthesis
- `crates/session-extractor/src/extraction/mod.rs` — NEW: module root
- `crates/session-extractor/src/lib.rs` — integrate map-reduce flow
- `crates/infrastructure/src/extraction/claude.rs` — update to support map-reduce
- `crates/infrastructure/src/extraction/ollama.rs` — update to support map-reduce
**Depends on:** Slice 1.1 (quality rubric)
**Dependency type:** real
**Blast radius:** medium (changes extraction flow, adds new modules, keeps trait boundary unchanged)

###### What to build
Implement the SkillLens parallel extraction architecture:
1. **MAP phase:** Split session transcript into individual trajectory turns. For each turn, call the extraction provider with a success-mode or failure-mode prompt (depending on turn outcome). Each turn produces a `ModeSet { success_modes, failure_modes, summary }`.
2. **REDUCE phase (intermediate):** Group ModeSets by merge_group_size (default 10). Hierarchically merge via LLM — each merge produces a consolidated ModeSet. Repeat until groups fit within a single final reduce call.
3. **REDUCE phase (final):** Tool-calling LLM synthesizes remaining ModeSets into `SkillStore` operations (create_skill, update_skill, add_procedure, add_convention, blacklist_action). This is where the quality rubric's dimensions influence output.

The trait boundary (`TranscriptSkillExtractionService::extract`) stays unchanged. The map-reduce flow is an internal implementation detail behind the trait.

Parameters (from SkillLens config, adapted):
- `max_modes_per_trajectory: 3` — cap modes extracted per turn
- `merge_group_size: 10` — ModeSets per reduce group
- `max_skills: 3` — skills per extraction run
- `max_skill_chars: 3000` — per-skill character budget
- `include_feedback: true` — include turn outcome in map prompt

###### Scope
- **Owns:** Map phase, reduce phase (intermediate + final), SkillStore integration, extraction parameter config
- **Non-goals:** Changing the provider adapter trait interface, changing `.pending` file format, adding extraction quality filtering. Those are separate slices.
- **Scope fence:** Do not change single-pass extraction path — keep it as fallback. Do not change MCP tool handler — extraction remains async background task. Do not add batching across multiple sessions — single-session extraction only.

###### Acceptance criteria
- [ ] Map phase extracts ModeSets per trajectory turn with correct success/failure classification
- [ ] Intermediate reduce merges ModeSets hierarchically, reducing to ≤ merge_group_size before final reduce
- [ ] Final reduce synthesizes skills via tool-calling with skill budget enforcement
- [ ] Map-reduce path produces at least as many concrete failure mechanisms as single-pass (measured by heuristic)
- [ ] Single-pass extraction remains available as config-gated fallback
- [ ] `cargo test -p session-extractor` passes with map-reduce flow tests

###### Evidence
- **Test command:** `cargo test -p session-extractor`
- **Evidence focus:** Map phase correctness (success/failure classification), reduce phase convergence, tool-calling synthesis produces valid skills

##### Slice 1.3: Quality scoring in domain model and PG schema
**Slice type:** expansion
**Serves:** SC-V2-1, SC-V2-3 (quality data propagates from extraction to storage)
**Demo scenario:** Extract a skill, verify `quality_scores` column in PG `skills` table is populated, query admin tool to inspect quality scores.
**Feature home:** `crates/domain/` + `crates/infrastructure/`
**Files:**
- `crates/infrastructure/migrations/002_quality_intelligence.sql` — NEW: migration adding `skills.quality_scores JSONB`, `session_logs.success_ratio FLOAT`
- `crates/infrastructure/src/persistence/postgres.rs` — update skill write to include quality_scores
- `crates/domain/src/types.rs` — ensure `Skill` type includes `quality_scores: Option<QualityScores>`
- `crates/session-extractor/src/lib.rs` — populate quality_scores in PG write path
- `crates/admin/src/tools.rs` — update `inspect_skill` to return quality_scores
- `tests/integration/test_admin_tools.rs` — verify quality scores visible
**Depends on:** Slice 1.2 (map-reduce extraction produces quality scores)
**Dependency type:** real

###### What to build
Migration `002_quality_intelligence.sql`:
```sql
ALTER TABLE skills ADD COLUMN quality_scores JSONB;
ALTER TABLE session_logs ADD COLUMN success_ratio FLOAT DEFAULT 0.5;
CREATE INDEX idx_skills_quality ON skills USING GIN (quality_scores);
```

Update `inspect_skill` admin tool to return `quality_scores` in the JSON response. Update skill write path in `postgres.rs` to serialize `quality_scores` from `ExtractedSkillCandidate` into the JSONB column.

###### Scope
- **Owns:** Schema migration, quality score propagation from extraction to PG, admin tool update
- **Non-goals:** Quality-based filtering, quality-weighted retrieval, quality-aware retirement. Follow-on slices.
- **Scope fence:** Additive migration only — no column drops, no CHECK constraint changes, no existing query changes.

###### Acceptance criteria
- [ ] Migration `002_quality_intelligence.sql` runs idempotently (run twice, no errors)
- [ ] `skills.quality_scores` is populated with valid JSON after extraction + ingestion
- [ ] `inspect_skill` returns quality_scores in JSON response
- [ ] `session_logs.success_ratio` is populated from extraction source session

###### Evidence
- **Test command:** `docker compose -f docker-compose.test.yml up --abort-on-container-exit && cargo test --test test_admin_tools`
- **Evidence focus:** Migration idempotency, quality_scores propagation, admin tool visibility

##### Slice 1.4: LLM-synthesized context compiler
**Slice type:** expansion
**Serves:** SC-V2-8 (LLM-guided compilation), SC-6 (subunit-aware compilation)
**Demo scenario:** Call `compile_context` with a config flag requesting LLM guidance. The response includes task-specific synthesized guidance (not just template-formatted skill text) in under 3 seconds.
**Feature home:** `crates/compiler/`
**Files:**
- `crates/compiler/src/guidance.rs` — NEW: `OllamaGuidanceCompiler` implementing `ContextCompiler`
- `crates/compiler/src/lib.rs` — register new compiler, export
- `crates/mcp-server/src/tools/compile_context.rs` — compiler selection by config field
- `crates/domain/src/config.rs` — add `compiler_mode: template | guidance` to config
- `tests/integration/test_compile_context.rs` — verify LLM compiler output
**Depends on:** Slice 1.1 (quality rubric available for compiler to reference)
**Dependency type:** real

###### What to build
Implement `OllamaGuidanceCompiler` behind the `ContextCompiler` trait:
```
Input: Vec<ScoredSkill> + prompt text
Output: structured markdown with:
  1. Task-specific procedure synthesis (not just skill text)
  2. Relevant subunit highlights prioritized by prompt-semantic match
  3. Rescue cues from below-threshold skills that are still relevant
  4. Cross-skill conflict warnings (when two skills suggest conflicting approaches)
```

The compiler uses local Ollama with a purpose-built system prompt that:
- References the quality scores of input skills (prioritize high-quality skills)
- Synthesizes across skills rather than concatenating
- Stays under 3 seconds (small model, constrained output length)
- Falls back to template compilation on timeout

MCP server selects compiler by `compiler_mode` config field. Default: `template` (sub-500ms). Opt-in: `guidance` (2-3s, richer output).

###### Scope
- **Owns:** Guidance compiler implementation, compiler selection logic, compilation timeout
- **Non-goals:** Multi-harness compilers (Phase 5), counterfactual explainability (Phase 4), compiler quality scoring. Follow-on slices.
- **Scope fence:** Do not remove or change `TemplateOnlyCompiler`. Do not change `compile_context` result contract — status codes and response shape stay identical.

###### Acceptance criteria
- [ ] `OllamaGuidanceCompiler` implements `ContextCompiler` trait
- [ ] Compiler produces task-specific synthesized guidance (not just concatenated skill text)
- [ ] Compiler times out at 3 seconds and falls back to template mode
- [ ] `compiler_mode: guidance` config flag routes to guidance compiler
- [ ] Integration test verifies guidance output differs from template output for same input

###### Evidence
- **Test command:** `cargo test -p compiler && cargo test --test test_compile_context`
- **Evidence focus:** Trait implementation, timeout behavior, config routing, output differentiation from template

##### Slice 1.5: Remote embedding provider abstraction
**Slice type:** expansion
**Serves:** SC-V2-2 preparation (remote embeddings needed for team scope)
**Demo scenario:** Configure a remote embedding endpoint URL. The system generates embeddings via remote API alongside existing local Ollama. Embedding dimension contract remains 768.
**Feature home:** `crates/infrastructure/`
**Files:**
- `crates/infrastructure/src/embeddings/remote.rs` — NEW: `RemoteEmbeddingService` implementing `EmbeddingService`
- `crates/infrastructure/src/embeddings/mod.rs` — register remote provider
- `crates/infrastructure/src/embeddings/ollama.rs` — no changes, existing impl stays
- `crates/domain/src/config.rs` — add `embedding_provider: ollama | remote` config
- `crates/domain/src/errors.rs` — add `EmbeddingError::ProviderUnavailable` variant for remote
**Depends on:** None (standalone trait implementation)
**Dependency type:** parallel-safe (does not depend on any Phase 1 slices)

###### What to build
Implement `RemoteEmbeddingService` that calls an OpenAI-compatible embedding API (configurable URL). This:
1. Provides an alternative to local Ollama for teams with shared GPU infrastructure
2. Keeps the same `EmbeddingService` trait — callers don't know or care where embeddings come from
3. Maintains the 768-dimension contract (configurable model name, fixed dimension validation)
4. Enforces the same concurrency semaphore (4 concurrent calls) and timeout (500ms sync, 5s batch)

###### Scope
- **Owns:** Remote embedding adapter, config routing, dimension validation
- **Non-goals:** Embedding caching, fallback chain, provider quality comparison. Follow-on slices.
- **Scope fence:** Do not change Ollama integration. Do not change callers. This is a drop-in alternative behind the same trait.

###### Acceptance criteria
- [ ] `RemoteEmbeddingService` implements `EmbeddingService` trait
- [ ] Embedding API call succeeds against a configured endpoint
- [ ] Dimension validation rejects mismatched embedding sizes
- [ ] Concurrency semaphore and timeout enforced
- [ ] Config flag routes to remote vs Ollama

###### Evidence
- **Test command:** `cargo test -p infrastructure -- embedding`
- **Evidence focus:** Trait implementation, dimension contract, timeout behavior

---

#### Phase 2: Team Scope
**Purpose:** Enable shared skill knowledge across a team through an optional remote PG+Qdrant scope. The V1.1 architecture's `ScopeResolver` trait and scalar `scope` column with `merged_from_scopes TEXT[]` were explicitly deferred for this moment. Additive migration: new junction table, new resolver, same retrieval pipeline.
**Rationale:** Team scope is the bridge from solo power user to team infrastructure. The architecture was built for this — the `ScopeResolver` trait, the dual-scope concurrent search in `retrieval`, and the `ScopeType::Team` enum all exist in V1.1 waiting for this phase. Adding it is filling seams, not cutting new ones.

##### Slice 2.1: Team scope domain types and junction table
**Slice type:** tracer-bullet
**Serves:** SC-V2-2 (team scope data model)
**Demo scenario:** Run schema migration, verify `skill_scopes` junction table exists, insert a skill with both project and team scope membership, query by scope.
**Feature home:** `crates/domain/` + `crates/infrastructure/`
**Files:**
- `crates/infrastructure/migrations/003_team_scope.sql` — NEW: `skill_scopes` junction table, optional remote PG config
- `crates/domain/src/types.rs` — `Skill` type: add `scope_ids: Vec<DomainId>` (in addition to existing `scope: ScopeType` for backward compat)
- `crates/infrastructure/src/persistence/postgres.rs` — update skill CRUD for junction table writes
- `crates/infrastructure/src/scope.rs` — no changes yet (resolver later)
- `tests/integration/test_dual_scope.rs` — verify team scope in data model
**Depends on:** None (parallel-safe — only schema and types, no retrieval changes)
**Dependency type:** parallel-safe

###### What to build
Migration `003_team_scope.sql`:
```sql
CREATE TABLE skill_scopes (
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    scope_type TEXT NOT NULL CHECK (scope_type IN ('project', 'global', 'team')),
    scope_id TEXT NOT NULL, -- team scope identifier (URL or name)
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (skill_id, scope_type, scope_id)
);
CREATE INDEX idx_skill_scopes_type_id ON skill_scopes (scope_type, scope_id);
```

The existing `skills.scope` column stays as the *primary* scope for backward compatibility. `skill_scopes` junction table provides *additional* scope memberships. V1.1 code that reads `skills.scope` continues to work. V2 code that queries `skill_scopes` gets team scope membership.

###### Scope
- **Owns:** Junction table schema, skill CRUD updates, domain type extension
- **Non-goals:** Remote PG/Qdrant connection, team scope resolver, team scope retrieval. Follow-on slices.
- **Scope fence:** Additive migration only. Do not drop or rename `skills.scope`. Do not change existing queries.

###### Acceptance criteria
- [ ] Migration `003_team_scope.sql` runs idempotently
- [ ] `skill_scopes` junction table enforces CHECK constraint on scope_type
- [ ] Skill INSERT writes to both `skills.scope` and `skill_scopes`
- [ ] Skill DELETE cascades to `skill_scopes`
- [ ] Backward compatibility: V1.1 code paths unchanged

###### Evidence
- **Test command:** `docker compose -f docker-compose.test.yml up --abort-on-container-exit`
- **Evidence focus:** Migration idempotency, junction table constraints, cascade behavior, backward compat

##### Slice 2.2: RemoteTeamScopeResolver
**Slice type:** expansion
**Serves:** SC-V2-2 (team scope resolution)
**Demo scenario:** Configure `TEAM_PG_URL` and `TEAM_QDRANT_URL` env vars. Call `scope_resolver.resolve()`. Verify team scope appears alongside project and global scopes.
**Feature home:** `crates/infrastructure/`
**Files:**
- `crates/infrastructure/src/scope/team.rs` — NEW: `RemoteTeamScopeResolver` implementing `ScopeResolver`
- `crates/infrastructure/src/scope.rs` — register team resolver
- `crates/domain/src/config.rs` — add `team_pg_url`, `team_qdrant_url` config fields
- `crates/domain/src/errors.rs` — add `ScopeError::TeamConnectionFailed`
- `tests/integration/test_dual_scope.rs` — verify three-scope resolution
**Depends on:** Slice 2.1 (junction table exists)
**Dependency type:** real

###### What to build
Implement `RemoteTeamScopeResolver`:
```
resolve() → Vec<ScopeDescriptor>
  Returns team scope with:
    scope_type: Team
    paths: [] (remote — no local filesystem)
    config: { pg_url, qdrant_url }
```

The resolver:
1. Reads `TEAM_PG_URL` and `TEAM_QDRANT_URL` from Docker Compose env vars
2. Tests connection on resolution (health check: `SELECT 1` on PG, collection info on Qdrant)
3. Returns team scope only when both connections succeed
4. Falls back to project+global only when team scope is unavailable (not degraded — just absent)

The existing `ScopeResolver` trait return type (`Vec<ScopeDescriptor>`) already supports N scopes. This is a new element in the vector, not a protocol change.

###### Scope
- **Owns:** Team scope connection, health check, resolution logic
- **Non-goals:** Remote index construction, cross-tenant isolation, team Qdrant collection management. Follow-on slices.
- **Scope fence:** Do not change project or global scope resolution. Do not make team scope mandatory. Opt-in only.

###### Acceptance criteria
- [ ] `RemoteTeamScopeResolver` implements `ScopeResolver` trait
- [ ] Team scope appears in resolution when env vars are configured
- [ ] Team scope is absent (not error) when env vars are not configured
- [ ] Connection failure returns `ScopeError::TeamConnectionFailed` with reason code
- [ ] Integration test verifies three-scope resolution with mock remote config

###### Evidence
- **Test command:** `cargo test -p infrastructure -- scope && cargo test --test test_dual_scope`
- **Evidence focus:** Trait implementation, connection resilience, three-scope resolution

##### Slice 2.3: Team scope retrieval in dual-scope pipeline
**Slice type:** expansion
**Serves:** SC-V2-2 (team scope retrieval)
**Demo scenario:** Seed team scope Qdrant collection with skills. Call `compile_context` with team scope configured. Verify team-scoped skills appear in merged results alongside project and global.
**Feature home:** `crates/retrieval/`
**Files:**
- `crates/retrieval/src/dual_scope.rs` — extend `search_scopes_concurrently` to handle 3+ scopes (currently handles 1, 2, and N — N already works, just add team scope filter)
- `crates/retrieval/src/qdrant_search.rs` — add team scope Qdrant collection search
- `crates/retrieval/src/orchestrator.rs` — add team scope weight to `RetrievalConfig`
- `crates/mcp-server/src/tools/compile_context.rs` — include team scope in response `scopes_considered`
- `tests/integration/test_dual_scope.rs` — verify three-scope fusion
**Depends on:** Slice 2.2 (resolver returns team scope)
**Dependency type:** real

###### What to build
The `search_scopes_concurrently` function in `dual_scope.rs` already handles `_ ⇒ N` scopes via `tokio::spawn` for the general case. Team scope is a third element in the scopes array — the concurrency path already works.

Add:
1. Team scope `seeded_skill_matches_scope` filter (checks `skill.scope == ScopeType::Team` and `scope_id` matches remote scope ID)
2. Team scope weight in `RetrievalConfig` (default 0.5 — lower than global's 0.7, reflecting remote latency penalty)
3. Remote Qdrant collection name in `qdrant_search.rs` (configurable per scope)
4. Team scope in `scopes_considered` response field

###### Scope
- **Owns:** Team scope retrieval integration, scope weighting, response metadata
- **Non-goals:** Cross-tenant isolation verification (Slice 2.4), team scope index construction, remote PG fallback retrieval. Follow-on slices.
- **Scope fence:** Do not change MMR or RRF fusion — same algorithm, additional scope input. Do not change single/dual-scope paths.

###### Acceptance criteria
- [ ] Team scope skills appear in `compile_context` results when team scope is configured
- [ ] Team scope weight is applied in RRF fusion (team skills score lower than equivalent project skills)
- [ ] `scopes_considered` includes team scope in response
- [ ] Integration test verifies three-scope fusion produces correct ranking

###### Evidence
- **Test command:** `cargo test -p retrieval && cargo test --test test_dual_scope`
- **Evidence focus:** Three-scope fusion correctness, scope weighting, response metadata

##### Slice 2.4: Cross-tenant isolation and collective intelligence
**Slice type:** hardening
**Serves:** SC-V2-2 (isolation), DS-017 (cross-repo collective intelligence)
**Demo scenario:** Seed two tenant repos with overlapping skill names. Verify team scope retrieval returns team skills but never leaks tenant-specific content. Verify provenance trail on every team scope skill.
**Feature home:** `crates/graph-builder/` + `crates/retrieval/`
**Files:**
- `crates/graph-builder/src/graph/team_index.rs` — NEW: team scope index construction with provenance hashing
- `crates/retrieval/src/dual_scope.rs` — add tenant-isolation scope filter
- `crates/infrastructure/src/persistence/postgres.rs` — add `provenance_hash` column to team-scoped skills
- `crates/infrastructure/migrations/003_team_scope.sql` — add `provenance_hash TEXT` to `skill_scopes`
- `tests/e2e/test_dream_state_contract.rs` — un-ignore DS-017
**Depends on:** Slice 2.3 (team scope retrieval)
**Dependency type:** real

###### What to build
Every team-scoped skill carries an immutable provenance trail:
```json
{
  "origin_repo": "github.com/team/project-a",
  "origin_scope": "project",
  "promoted_at": "2026-06-01T12:00:00Z",
  "promoted_by": "rabak",
  "provenance_hash": "blake3:abc123..."
}
```

Cross-tenant isolation guarantees:
1. **At write time:** Graph builder computes `provenance_hash` from skill content + origin repo. Team scope write requires explicit promotion (human-gated, constitution §3).
2. **At read time:** Retrieval filter ensures team-scoped skills never include tenant-specific paths, secrets, or repo identifiers in response content. The `additional_context` field strips origin paths.
3. **At merge time:** Maintenance never merges team-scoped skills across different origin repos. Merge candidates must share provenance.

Un-ignore DS-017: Cross-repo collective intelligence learns globally without tenant leakage.

###### Scope
- **Owns:** Provenance hashing, tenant isolation filter, team promotion workflow, DS-017 contract
- **Non-goals:** Automatic team promotion (always human-gated), team admin UI, team scope access control lists. V3 territory.
- **Scope fence:** Do not weaken human gate for scope promotion. Do not silently strip provenance — missing provenance is an error condition.

###### Acceptance criteria
- [ ] Every team-scoped skill has provenance_hash in `skill_scopes`
- [ ] Retrieval strips tenant-specific paths from team scope responses
- [ ] Merge detector refuses cross-origin team scope merges
- [ ] DS-017 test is un-ignored and passes with canary-tagged multi-tenant fixture

###### Evidence
- **Test command:** `cargo test --test test_dream_state_contract -- ds_017`
- **Evidence focus:** Provenance immutability, isolation verification, canary token non-leakage

##### Slice 2.5: Team scope graph rebuild and maintenance
**Slice type:** expansion
**Serves:** SC-V2-2, SC-4 (team scope maintenance)
**Demo scenario:** A teammate approves a skill in their project scope. The skill is promoted to team scope via human approval. The graph builder detects the promotion and rebuilds the team index. Maintenance cron considers team-scoped skills for utility-scored retirement.
**Feature home:** `crates/graph-builder/` + `crates/maintenance/`
**Files:**
- `crates/graph-builder/src/graph/team_rebuild.rs` — NEW: team index rebuild on promotion events
- `crates/maintenance/src/merge.rs` — add team scope to merge policy (cross-scope merges: project↔team, global↔team)
- `crates/maintenance/src/retire.rs` — add team scope to retirement consideration
- `crates/maintenance/src/cron.rs` — add team scope maintenance pass
- `tests/integration/test_merge_workflow.rs` — verify team scope merge proposals
- `tests/integration/test_retire_workflow.rs` — verify team scope retirement
**Depends on:** Slice 2.4 (isolation guarantees)
**Dependency type:** real

###### What to build
1. **Team index rebuild:** When a `skill.promoted_to_team` event fires (human renamed `.promote` to `.md` in team scope directory), graph builder picks up the new team-scoped skill, generates embeddings, and writes to the team Qdrant collection.
2. **Team scope merge:** `MergeProposalWriter` extends its cross-scope policy to include team scope. Project↔team merges prefer project scope. Global↔team merges prefer global scope. Team↔team merges (different origin repos) are FORBIDDEN.
3. **Team scope retirement:** `RetirementProposalWriter` considers team-scoped skills for retirement with the same utility scoring (usage + quality). Team scope retirement decisions are per-repo-visible via the local filesystem.

###### Scope
- **Owns:** Team index rebuild, team merge policy, team retirement, team scope event handling
- **Non-goals:** Team scope admin dashboard, team skill popularity metrics, team scope ACL. V3 territory.
- **Scope fence:** Do not change project/global scope maintenance behavior. Team scope is additive.

###### Acceptance criteria
- [ ] Promotion event triggers team index rebuild
- [ ] Team merge proposals respect cross-origin prohibition
- [ ] Team retirement considers utility scoring
- [ ] Team scope maintenance pass runs on cron trigger

###### Evidence
- **Test command:** `cargo test -p maintenance && cargo test -p graph-builder && cargo test --test test_merge_workflow -- team && cargo test --test test_retire_workflow -- team`
- **Evidence focus:** Team rebuild correctness, merge policy enforcement, retirement fairness

---

#### Phase 3: Self-Evolving Graph
**Purpose:** Transform maintenance from passive cleanup (delete stale) to active improvement (optimize existing). SkillOpt's rollout→reflect→edit→gate→deploy cycle becomes our optimization service. Autonomous self-healing recovers from known degraded states. Outcome-based learning tunes thresholds from human feedback signals.
**Rationale:** V1.1's maintenance pipeline already has the right seams: `MergeSemanticVerifier`, `SkillSnapshot`, `SeededSkillProjection`. SkillOpt's loop maps directly — rollout is our session replay, reflect is our extraction analysis, edit is our merge proposal, gate is our human approval. The architecture anticipates this.

##### Slice 3.1: Health probe trait and concrete implementations
**Slice type:** tracer-bullet
**Serves:** SC-V2-5 (self-healing foundation), DS-003 (chaos matrix)
**Demo scenario:** Call health probe endpoint. Response includes per-dependency status (PG: ok, Redis: ok, Qdrant: ok, Ollama: degraded/unavailable) with reason codes. Kill a container, re-check — status reflects failure.
**Feature home:** `crates/infrastructure/`
**Files:**
- `crates/domain/src/traits.rs` — add `HealthProbe` trait
- `crates/infrastructure/src/health.rs` — implement probes for PG/Redis/Qdrant/Ollama
- `crates/infrastructure/src/health/mod.rs` — NEW: module root
- `crates/infrastructure/src/health/postgres.rs` — NEW: PG probe (`SELECT 1`)
- `crates/infrastructure/src/health/redis.rs` — NEW: Redis probe (`PING`)
- `crates/infrastructure/src/health/qdrant.rs` — NEW: Qdrant probe (collection info)
- `crates/infrastructure/src/health/ollama.rs` — NEW: Ollama probe (model list)
- `crates/mcp-server/src/lib.rs` — add health probe call on startup, expose via response health field
- `crates/domain/src/types.rs` — add `HealthStatus { dependency, status, reason_code, latency_ms }`
**Depends on:** None (standalone infrastructure addition)
**Dependency type:** parallel-safe

###### What to build
```rust
#[async_trait]
pub trait HealthProbe: Send + Sync {
    async fn check(&self) -> HealthStatus;
    fn dependency_name(&self) -> &'static str;
}
```

Four concrete implementations:
- `PostgresHealthProbe`: `SELECT 1` with 2s timeout
- `RedisHealthProbe`: `PING` with 1s timeout
- `QdrantHealthProbe`: collection info with 2s timeout
- `OllamaHealthProbe`: model list with 3s timeout

Each probe returns `HealthStatus { dependency, status: ok|degraded|unavailable, reason_code, latency_ms }`. The probe runner aggregates all four into a health vector and replaces the current hardcoded health markers in `RetrievalOrchestrator`.

###### Scope
- **Owns:** Health probe trait, four concrete probes, health aggregation, live health markers
- **Non-goals:** Self-healing actions (Slice 3.3), health event publishing (Slice 3.2), circuit breaker integration. Follow-on slices.
- **Scope fence:** Do not change retrieval pipeline — only replace hardcoded health strings with probe results. Do not add health endpoint — this replaces existing health field in compile_context response.

###### Acceptance criteria
- [ ] `HealthProbe` trait defined in domain with zero infra deps
- [ ] All four probes return correct status for each dependency state (up, degraded, down)
- [ ] `compile_context` response `health` field reflects live probe results
- [ ] Probe timeout prevents compile_context latency blowout (probes run on startup, cached for 30s)

###### Evidence
- **Test command:** `cargo test -p infrastructure -- health`
- **Evidence focus:** Probe correctness for each dependency, timeout behavior, health field population

##### Slice 3.2: Health event publishing and degradation detection
**Slice type:** expansion
**Serves:** SC-V2-5 (degradation detection), DS-003 (chaos matrix preparation)
**Demo scenario:** Run the system, kill Redis. Verify `health.degraded_detected` event published to Redis Streams with reason code, dependency name, and timestamp. Restore Redis, verify recovery event.
**Feature home:** `crates/infrastructure/` + `crates/graph-builder/`
**Files:**
- `crates/infrastructure/src/health/monitor.rs` — NEW: periodic health check loop with state change detection
- `crates/infrastructure/src/streaming/redis.rs` — add `health.degraded_detected` and `health.self_healed` event publishing
- `crates/infrastructure/src/health/mod.rs` — wire health monitor into infrastructure bootstrap
- `crates/infrastructure/migrations/003_team_scope.sql` — add `health_events` table
- `tests/integration/test_watcher_rebuild.rs` — verify health events publish on container kill/restore
**Depends on:** Slice 3.1 (health probes exist)
**Dependency type:** real

###### What to build
Health monitor runs a configurable interval (default 30s). On each tick:
1. Run all four probes concurrently
2. Compare results to last known state
3. On degradation: publish `health.degraded_detected` event, write to `health_events` PG table
4. On recovery: publish `health.self_healed` event, write to `health_events`
5. Update cached health state (used by compile_context response)

The `health_events` table:
```sql
CREATE TABLE health_events (
    id UUID PRIMARY KEY,
    dependency TEXT NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('degraded_detected', 'self_healed', 'unavailable', 'recovered')),
    reason_code TEXT,
    latency_ms INTEGER,
    detected_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

###### Scope
- **Owns:** Health monitoring loop, state change detection, event publishing, PG event table
- **Non-goals:** Autonomous remediation (Slice 3.3), chaos matrix testing (DS-003, T14 from V1.1). Follow-on slices.
- **Scope fence:** Monitor is passive — detects and publishes, does not act. Actions are Slice 3.3.

###### Acceptance criteria
- [ ] Health monitor runs periodic probes and detects state changes
- [ ] Degradation events published with correct dependency and reason_code
- [ ] Recovery events published when dependency comes back
- [ ] `health_events` table populated with audit trail

###### Evidence
- **Test command:** `cargo test -p infrastructure -- health && cargo test --test test_watcher_rebuild`
- **Evidence focus:** State change detection, event publishing, audit trail completeness

##### Slice 3.3: Autonomous self-healing loop
**Slice type:** expansion
**Serves:** SC-V2-5 (self-healing), DS-014 (autonomous recovery)
**Demo scenario:** Inject a known degraded state (Qdrant orphan vectors). The self-healer detects the reason code, selects the reconciliation remediation from the policy catalog, executes it, and verifies the fix. Audit trail records every step.
**Feature home:** `crates/maintenance/` (new module) + `crates/admin/`
**Files:**
- `crates/maintenance/src/healing/mod.rs` — NEW: self-healing orchestrator
- `crates/maintenance/src/healing/catalog.rs` — NEW: policy-safe remediation catalog
- `crates/maintenance/src/healing/actions.rs` — NEW: concrete remediation actions (reconcile_qdrant, restart_watcher, clear_cache, rebuild_graph)
- `crates/maintenance/src/healing/audit.rs` — NEW: healing-specific audit trail
- `crates/admin/src/tools.rs` — add `trigger_self_heal` admin tool
- `crates/domain/src/types.rs` — add `Remediation { id, action, reason_code, is_safe, rollback }`
- `tests/e2e/test_dream_state_contract.rs` — un-ignore DS-014
**Depends on:** Slice 3.2 (health events published)
**Dependency type:** real
**Blast radius:** medium (autonomous action, bounded to known-safe operations)

###### What to build
Self-healing orchestrator consumes `health.degraded_detected` events:

1. **Detect:** Match `reason_code` against the remediation catalog
2. **Select:** Choose safe remediation (all catalog entries are pre-approved for autonomy). Unsafe remediations require admin trigger.
3. **Execute:** Run the remediation action with bounded retries (max 3) and exponential backoff
4. **Verify:** Re-run health probe for the affected dependency. On success → `health.self_healed`. On failure → escalate to admin tool (no auto-retry beyond 3 attempts)
5. **Audit:** Write complete remediation trail to `health_events`

Policy-safe remediation catalog (constitution-compliant — no mutations to skill content):
| Reason Code | Remediation | Safe for auto? |
|-------------|-------------|----------------|
| `embedding_provider_unavailable` | Wait and retry (30s intervals) | Yes |
| `qdrant_collection_missing` | Rebuild from PG outbox | Yes |
| `qdrant_orphan_vectors` | Run reconciliation | Yes |
| `watcher_stale` | Restart watcher + reconciliation scan | Yes |
| `graph_version_mismatch` | Invalidate cache, signal rebuild | Yes |
| `pg_connection_lost` | Reconnect pool | Yes |
| `outbox_backlog` | Drain outbox | Yes |

###### Scope
- **Owns:** Self-healing orchestrator, remediation catalog, safe-action execution, healing audit trail
- **Non-goals:** Skill content mutation, SkillOpt optimization, universal fault recovery. The catalog is explicit and bounded. Skill content changes remain human-gated (constitution §3).
- **Scope fence:** Do not auto-heal across service boundaries. Do not auto-approve skill changes. Do not heal in a loop — max 3 attempts then escalate to admin.

###### Acceptance criteria
- [ ] Self-healer detects known reason codes and selects matching remediation
- [ ] Seven safe remediations in catalog execute correctly
- [ ] Max 3 retry attempts per remediation with exponential backoff
- [ ] Healing audit trail is complete and queryable
- [ ] `trigger_self_heal` admin tool can manually invoke any remediation
- [ ] DS-014 test is un-ignored and passes

###### Evidence
- **Test command:** `cargo test -p maintenance -- healing && cargo test --test test_dream_state_contract -- ds_014`
- **Evidence focus:** Detection-selection-execution-verification loop, audit trail, retry bounds, admin trigger

##### Slice 3.4: Utility-scored retirement with quality awareness
**Slice type:** expansion
**Serves:** SC-V2-3 (utility-scored maintenance)
**Demo scenario:** Create two skills: high-quality low-usage, low-quality high-usage. Run retirement cron. Verify high-quality skill survives retirement longer than low-quality despite lower usage.
**Feature home:** `crates/maintenance/`
**Files:**
- `crates/maintenance/src/retire.rs` — add `utility_score()` function combining usage (60%) + quality (40%)
- `crates/maintenance/src/retire/scoring.rs` — NEW: scoring module with utility score formula
- `crates/maintenance/src/cron.rs` — pass quality scores to retirement evaluator
- `tests/integration/test_retire_workflow.rs` — verify quality-weighted retirement
**Depends on:** Slice 1.3 (quality scores in PG), Slice 3.2 (health events, optional — retirement cron can trigger on health.degraded_detected clearance)
**Dependency type:** real

###### What to build
Replace the current `usage_score_per_month < threshold` with a weighted utility score:
```rust
fn utility_score(skill: &Skill, usage: &[UsageSample], quality: Option<&QualityScores>) -> f32 {
    let usage_score = usage_per_month(usage); // existing logic
    let quality_score = quality
        .map(|q| q.combined_utility_score)
        .unwrap_or(0.5); // default for pre-V2 skills without quality data
    
    // Configurable weights
    usage_score * USAGE_WEIGHT + quality_score * QUALITY_WEIGHT // 0.6 / 0.4
}
```

Skills with `utility_score < threshold` are proposed for retirement. Skills above threshold but below 2× threshold get a warning annotation. Skills with quality scores below 0.3 are flagged as "low quality extraction" in `.retired` frontmatter — these might have been extraction artifacts rather than actually stale.

###### Scope
- **Owns:** Utility scoring formula, quality-aware retirement threshold, warning annotations
- **Non-goals:** Outcome-based threshold tuning (Slice 4.4), automatic retirement (always human-gated). Follow-on slices.
- **Scope fence:** Do not auto-retire. Do not change `.retired` proposal format — only add quality annotations to frontmatter.

###### Acceptance criteria
- [ ] `utility_score()` combines usage and quality with configurable weights
- [ ] High-quality low-usage skill survives retirement longer than low-quality high-usage
- [ ] Low-quality extraction flag appears in `.retired` frontmatter
- [ ] Existing V1.1 retirement tests still pass (backward compatible)

###### Evidence
- **Test command:** `cargo test -p maintenance -- retire`
- **Evidence focus:** Utility score formula, quality migration, backward compatibility, threshold fairness

##### Slice 3.5: SkillOpt optimization service
**Slice type:** expansion
**Serves:** SC-V2-9 (SkillOpt loop), DS-021 (shadow deployment)
**Demo scenario:** Select a skill with usage data. Run one optimization epoch. The optimizer reflects on success/failure rollouts, proposes bounded edits, gates on held-out validation. Output is a `.optimized` proposal file. Human approves by renaming to `.md`.
**Feature home:** `crates/skill-optimizer/` (NEW service crate)
**Files:**
- `crates/skill-optimizer/Cargo.toml` — NEW
- `crates/skill-optimizer/src/lib.rs` — NEW: optimizer service entry
- `crates/skill-optimizer/src/rollout.rs` — NEW: replay skill against held-out session transcripts
- `crates/skill-optimizer/src/reflect.rs` — NEW: analyze success/failure minibatches
- `crates/skill-optimizer/src/edit.rs` — NEW: propose bounded edits (add/delete/replace, budget-constrained)
- `crates/skill-optimizer/src/gate.rs` — NEW: held-out validation — accept only if score improves
- `crates/skill-optimizer/src/optimizer.rs` — NEW: orchestrate the loop
- `crates/skill-optimizer/src/config.rs` — NEW: optimizer config (epochs, batch_size, edit_budget, optimizer_model)
- `crates/maintenance/src/cron.rs` — add optimizer trigger to cron schedule
- `crates/mcp-server/src/tools/trigger_optimization.rs` — NEW: admin tool to trigger optimization
- `tests/integration/test_optimization_loop.rs` — NEW: full loop integration test
**Depends on:** Slice 1.3 (quality scores in PG), Slice 3.1 (health probes for runtime)
**Dependency type:** real
**Blast radius:** high (new service, new crate, new MCP tool, new filesystem artifact type)
**Shared state changes:** New `.optimized` file type in skills directories. New event type: `skill.optimized`. New PG table: `optimization_runs`.

###### What to build
SkillOpt loop mapped to our architecture:

1. **Rollout stage:** Load the target skill, replay it against held-out session transcripts from `session_logs`. Score each transcript execution (did the skill help?). Produce success/failure minibatches.
2. **Reflect stage:** Send success and failure minibatches to optimizer model (configurable, defaults to Ollama with a stronger model). Optimizer identifies: "What patterns separate successes from failures? What specific edits would improve the skill?"
3. **Edit stage:** Optimizer proposes edits as add/delete/replace operations. Edit budget (textual learning rate) caps how much the skill can change per epoch (default: 4 operations). Rejected edits are buffered as negative feedback for future epochs.
4. **Gate stage:** Apply proposed edits to create a candidate skill. Test candidate against held-out validation set. Accept only if validation score exceeds current best. Reject otherwise (rejected edits go to buffer).
5. **Deploy:** Write accepted candidate as `.optimized` file. Human renames to `.md` to approve (constitution §3). Emit `skill.optimized` event. Write optimization history to `optimization_runs` table.

Configuration:
```yaml
optimizer:
  epochs: 4
  batch_size: 10
  edit_budget: 4
  target_model: same-as-extraction
  optimizer_model: stronger-model-or-same
  validation_split: 0.2
  slow_update_epochs: 3
```

###### Scope
- **Owns:** Full SkillOpt loop, rollout engine, reflect engine, edit engine, validation gate, `.optimized` output
- **Non-goals:** Automatic skill approval (always `.optimized` → human rename to `.md`), multi-skill co-optimization, cross-harness optimization. V3 territory.
- **Scope fence:** Do not modify active skills without human approval. Do not run optimization on skills without usage data. Do not exceed edit budget per epoch.

###### Acceptance criteria
- [ ] Rollout replays skill against held-out transcripts and scores outcomes
- [ ] Reflect identifies success/failure patterns
- [ ] Edit proposes bounded changes with budget enforcement
- [ ] Gate accepts only validation-improving candidates
- [ ] `.optimized` file is valid SKILL.md format with optimization frontmatter
- [ ] Optimization history written to `optimization_runs` table
- [ ] `trigger_optimization` MCP tool works

###### Evidence
- **Test command:** `cargo test -p skill-optimizer && cargo test --test test_optimization_loop`
- **Evidence focus:** Loop convergence, edit budget enforcement, validation gate correctness, `.optimized` format validity

##### Slice 3.6: Outcome-based learning for threshold tuning
**Slice type:** hardening
**Serves:** SC-V2-4 (outcome-based learning), DS-024 (outcome-based learning loop)
**Demo scenario:** Run the system for a simulated 30-day period with varying acceptance/rejection rates. Verify extraction quality thresholds and retirement scoring weights are tuned toward higher acceptance rates. Verify no regression in core correctness.
**Feature home:** `crates/maintenance/`
**Files:**
- `crates/maintenance/src/learning/mod.rs` — NEW: learning orchestrator
- `crates/maintenance/src/learning/outcome_tracker.rs` — NEW: track acceptance/rejection signals
- `crates/maintenance/src/learning/threshold_tuner.rs` — NEW: tune quality thresholds from signals
- `crates/maintenance/src/learning/sandbox.rs` — NEW: sandbox candidate thresholds, validate before deploy
- `crates/maintenance/src/cron.rs` — add learning pass to cron schedule
- `crates/infrastructure/migrations/003_team_scope.sql` — add `learning_state` singleton table
- `tests/e2e/test_dream_state_contract.rs` — un-ignore DS-024
**Depends on:** Slice 3.4 (utility scoring), Slice 3.5 (optimization history)
**Dependency type:** real

###### What to build
Outcome signal collection:
- **Acceptance:** Human renames `.pending` → `.md` (positive signal for extraction quality)
- **Rejection:** Human deletes `.pending` or renames to `.rejected` (negative signal)
- **Usage:** Skill appears in `skill_usage` with `context_status: ok` (positive utility signal)
- **Non-usage:** Skill never used despite being active (negative utility signal)

Periodic learning pass (cron, default weekly):
1. **Collect:** Aggregate outcome signals over the trailing 30-day window
2. **Tune:** Adjust extraction quality thresholds and retirement scoring weights to favor outcomes with higher acceptance/usage rates
3. **Sandbox:** Validate candidate thresholds against a held-out signal slice
4. **Deploy:** If validation passes, update `learning_state` singleton. If not, keep current thresholds.
5. **Audit:** Record tuning decisions with before/after thresholds and validation evidence

Regression guard: Any threshold change that would have rejected a previously-accepted skill is blocked. Any threshold change that would have accepted a previously-rejected skill requires a confidence interval.

###### Scope
- **Owns:** Outcome signal collection, threshold tuning, sandbox validation, learning state persistence
- **Non-goals:** Full reinforcement learning, cross-session behavior modeling, skill content generation. V3 territory.
- **Scope fence:** Do not change skill content (that's SkillOpt). Do not auto-deploy thresholds without sandbox validation. Do not learn from < 30 signals (too noisy).

###### Acceptance criteria
- [ ] Outcome signals collected from acceptance/rejection/usage events
- [ ] Threshold tuning produces candidate thresholds that improve on baseline
- [ ] Sandbox validation blocks regressive threshold changes
- [ ] `learning_state` singleton tracks threshold history
- [ ] DS-024 test is un-ignored and passes

###### Evidence
- **Test command:** `cargo test -p maintenance -- learning && cargo test --test test_dream_state_contract -- ds_024`
- **Evidence focus:** Signal collection correctness, tuning direction, sandbox regression guard, learning audit trail

---

#### Phase 4: Trust & Observability
**Purpose:** Make the retrieval pipeline explainable and traceable. Counterfactual explanations answer "why was skill X ranked over Y?" Causal tracing links every side effect to its originating session event. The drift sentinel continuously monitors for consistency degradation. This phase fulfills the trust contracts that make the system production-grade.
**Rationale:** DS-018 (counterfactual explainability), DS-022 (causal tracing), DS-019 (drift sentinel), and DS-015 (time-travel replay) are all observability contracts. They don't change system behavior — they make behavior inspectable. The retrieval pipeline already produces scored results with rationale strings; counterfactual extends this to "what would change the ranking."

##### Slice 4.1: Counterfactual explainability engine
**Slice type:** tracer-bullet
**Serves:** SC-V2-6 (explainability), DS-018 (counterfactual)
**Demo scenario:** Call `compile_context`, get ranked skills. Call `explain_ranking` with the same prompt. Response includes per-skill feature contribution scores, sensitivity analysis (what minimal prompt change would alter top-3 ranking), and ranked rationale text.
**Feature home:** `crates/explainability/` (NEW crate)
**Files:**
- `crates/explainability/Cargo.toml` — NEW
- `crates/explainability/src/lib.rs` — NEW: crate entry
- `crates/explainability/src/features.rs` — NEW: feature contribution scoring (semantic, lexical, prior, community_boost)
- `crates/explainability/src/counterfactual.rs` — NEW: minimal perturbation search
- `crates/explainability/src/rationale.rs` — NEW: ranked rationale generation
- `crates/explainability/src/types.rs` — NEW: CounterfactualExplanation, FeatureContribution
- `crates/mcp-server/src/tools/explain_ranking.rs` — NEW: MCP tool handler
- `crates/mcp-server/src/lib.rs` — register explain_ranking tool
- `crates/domain/src/types.rs` — add CounterfactualExplanation type
- `tests/e2e/test_dream_state_contract.rs` — un-ignore DS-018
**Depends on:** None (standalone — calls retrieval, operates on ScoredSkill output)
**Dependency type:** parallel-safe

###### What to build
`explain_ranking` tool:
```
Input: prompt, session_id, repo_path (same as compile_context)
Output: CounterfactualExplanation {
  ranked_skills: Vec<SkillExplanation> {
    skill_id, name, score,
    feature_contributions: {
      semantic: { contribution: 0.42, weight: 0.35 },
      lexical: { contribution: 0.18, weight: 0.25 },
      prior: { contribution: 0.08, weight: 0.20 },
      community_boost: { contribution: 0.04, weight: 0.10 },
      scope_weight: { contribution: 0.02, weight: 0.10 }
    },
    rationale: "Skill ranked #1 because...",
  },
  sensitivity: {
    prompt_perturbations: Vec<Perturbation> {
      before: "build a rust auth middleware",
      after: "build a rust auth middleware for actix-web",
      ranking_change: "skill 'rust-http-security' drops from #1 to #4"
    },
    threshold_distances: { nearest_swap_distance: 0.03 }
  }
}
```

The counterfactual engine:
1. Computes per-feature Shapley-style contributions by ablating each feature from the scoring formula and measuring ranking impact
2. Searches for minimal prompt perturbations that alter top-3 ranking (add/remove terms, scope qualifiers)
3. Generates human-readable rationale for each skill's position

###### Scope
- **Owns:** Feature contribution scoring, perturbation search, rationale generation, MCP tool handler
- **Non-goals:** Causal tracing (Slice 4.3), LLM-generated explanations (uses deterministic scoring), real-time explanation for every compile_context call. Follow-on slices.
- **Scope fence:** Explainability reads retrieval output — it does not change retrieval behavior. New crate, new trait boundary.

###### Acceptance criteria
- [ ] `explain_ranking` MCP tool returns valid CounterfactualExplanation
- [ ] Feature contributions sum to approximately the skill's final score (±0.05 tolerance)
- [ ] Perturbation search finds at least one plausible ranking-altering prompt change
- [ ] Rationale text is human-readable and references concrete score components
- [ ] DS-018 test is un-ignored and passes

###### Evidence
- **Test command:** `cargo test -p explainability && cargo test --test test_dream_state_contract -- ds_018`
- **Evidence focus:** Feature contribution accuracy, perturbation plausibility, rationale quality, MCP tool contract

##### Slice 4.2: Drift sentinel
**Slice type:** expansion
**Serves:** SC-V2-10, DS-019 (drift sentinel)
**Demo scenario:** Inject synthetic drift (Qdrant vector mismatch with PG graph state). Drift sentinel detects the mismatch within its check interval, raises an alarm event, and logs the drift surface. Verify alarm precision — false positives < 5%.
**Feature home:** `crates/maintenance/` + `crates/explainability/`
**Files:**
- `crates/maintenance/src/drift/mod.rs` — NEW: drift sentinel orchestrator
- `crates/maintenance/src/drift/checks.rs` — NEW: drift check implementations (PG↔Qdrant, vector↔content, filesystem↔graph)
- `crates/maintenance/src/drift/alarm.rs` — NEW: drift alarm emission and quarantine policy
- `crates/maintenance/src/cron.rs` — add drift sentinel pass
- `crates/infrastructure/src/streaming/redis.rs` — add drift alarm event type
- `tests/e2e/test_dream_state_contract.rs` — un-ignore DS-019
**Depends on:** Slice 4.1 (explainability crate exists for drift check diagnostics)
**Dependency type:** real

###### What to build
Drift checks (continuous, configurable interval, default 5 minutes):

1. **PG↔Qdrant consistency:** Compare skill count, embedding count, and content hashes between PG graph and Qdrant collections. Discrepancy → orphan/missing detection.
2. **Vector↔content consistency:** Sample N skills, regenerate embeddings from content, compare to stored embeddings. Cosine distance > 0.1 → potential embedding drift (model change, corruption).
3. **Filesystem↔graph consistency:** Compare watcher-observed files to graph nodes. Missing nodes → reconciliation gap.
4. **Behavioral canary:** Run 3 canary prompts through compile_context periodically. Compare output ranking to golden baseline. Ranking divergence > 1 position → retrieval quality drift.
5. **Lifecycle metadata:** Verify `.pending`/`.retired`/`.active` counts are consistent with PG lifecycle status.

On drift detection:
- Emit drift alarm event with surface type, severity, and diagnostic snapshot
- If severity is HIGH: quarantine affected skills (mark lifecycle as `drift_quarantine`, exclude from retrieval)
- Quarantine is reversible (human or automatic on drift clearance)
- Never auto-delete — quarantine only excludes from retrieval

###### Scope
- **Owns:** Five drift check types, alarm emission, quarantine policy, drift audit trail
- **Non-goals:** Automatic drift repair (covered by Slice 3.3 self-healing), full behavioral regression suite. Follow-on slices.
- **Scope fence:** Drift sentinel detects and alarms. Repair is either self-healing (Slice 3.3) or admin-triggered rebuild.

###### Acceptance criteria
- [ ] All five drift checks execute and produce measurable results
- [ ] PG↔Qdrant drift detected within one check interval
- [ ] False positive rate < 5% on healthy system
- [ ] High-severity drift triggers quarantine
- [ ] Quarantine is reversible
- [ ] DS-019 test is un-ignored and passes

###### Evidence
- **Test command:** `cargo test -p maintenance -- drift && cargo test --test test_dream_state_contract -- ds_019`
- **Evidence focus:** Drift detection accuracy, alarm precision, quarantine behavior, reversibility

##### Slice 4.3: End-to-end causal tracing
**Slice type:** expansion
**Serves:** SC-V2-10, DS-022 (causal tracing)
**Demo scenario:** Start a session, extract a skill, approve it, trigger a graph rebuild, retrieve it. Trace the full causal chain from session event → extraction → pending → approval → rebuild → retrieval. Verify no orphan side effects and complete lineage.
**Feature home:** `crates/infrastructure/` (correlation propagation) + `crates/explainability/` (trace graph query)
**Files:**
- `crates/infrastructure/src/tracing/mod.rs` — NEW: correlation ID propagation through all service boundaries
- `crates/infrastructure/src/tracing/correlation.rs` — NEW: CorrelationId type and propagation utilities
- `crates/infrastructure/src/streaming/redis.rs` — ensure all events carry correlation_id
- `crates/mcp-server/src/tools/compile_context.rs` — attach correlation_id to response
- `crates/mcp-server/src/tools/extract_session.rs` — attach correlation_id
- `crates/graph-builder/src/watcher.rs` — receive and propagate correlation_id
- `crates/maintenance/src/merge.rs` — attach correlation_id to merge events
- `crates/explainability/src/trace.rs` — NEW: trace graph construction and query
- `crates/admin/src/tools.rs` — add `trace_event` admin tool
- `tests/e2e/test_dream_state_contract.rs` — un-ignore DS-022
**Depends on:** Slice 4.1 (explainability crate exists)
**Dependency type:** real
**Blast radius:** medium (adds correlation_id across all event-publishing code paths)

###### What to build
Every MCP tool invocation generates a `CorrelationId` (UUIDv7). This ID propagates through:
1. MCP server → retrieval → compilation → response
2. MCP server → extraction → pending write → `extraction.completed` event
3. Watcher → `skill.file_changed` → `graph.rebuilt` event
4. Maintenance cron → merge/retire proposal → `skill.merged`/`skill.retired` event
5. Outbox relay → Qdrant write → completion

The trace graph query (`trace_event` admin tool):
```
Input: correlation_id
Output: CausalTrace {
  root_event: { type, timestamp, correlation_id },
  downstream_events: Vec<{ type, timestamp, correlation_id, parent_correlation_id }>,
  side_effects: Vec<{ table, operation, entity_id, timestamp }>,
  orphan_check: bool // true if any side effect has no upstream cause
}
```

###### Scope
- **Owns:** Correlation ID generation and propagation, trace graph construction, trace query tool
- **Non-goals:** Distributed tracing across Docker network (all services share PG audit_log), performance tracing (latency breakdown is Slice 4.4). Follow-on slices.
- **Scope fence:** Correlation IDs are metadata — they do not change event payloads or tool contracts. Additive only.

###### Acceptance criteria
- [ ] Every event published carries a `correlation_id` traceable to originating MCP call
- [ ] `trace_event` admin tool returns complete causal chain for any correlation_id
- [ ] Orphan side effects detected when present (test: manually write audit_log row without correlation_id)
- [ ] DS-022 test is un-ignored and passes

###### Evidence
- **Test command:** `cargo test --test test_dream_state_contract -- ds_022 && cargo test --test test_admin_tools -- trace`
- **Evidence focus:** Correlation completeness, trace graph correctness, orphan detection

##### Slice 4.4: Time-travel memory replay and offline deterministic twin
**Slice type:** hardening
**Serves:** SC-V2-10, DS-015 (time-travel replay), DS-023 (offline twin)
**Demo scenario:** Archive a session with known retrieval output. Checkout historical repo snapshot. Replay the session against the historical graph state. Verify retrieval output matches golden historical output identically.
**Feature home:** `crates/explainability/` + `crates/infrastructure/`
**Files:**
- `crates/explainability/src/replay.rs` — NEW: historical replay engine
- `crates/explainability/src/twin.rs` — NEW: deterministic twin mode
- `crates/infrastructure/src/persistence/postgres.rs` — add historical graph state snapshot query
- `crates/infrastructure/migrations/003_team_scope.sql` — add `graph_snapshots` table
- `tests/e2e/test_dream_state_contract.rs` — un-ignore DS-015, DS-023
**Depends on:** Slice 4.1 (explainability crate), Slice 4.3 (correlation IDs for session identification)
**Dependency type:** real

###### What to build
**Time-travel replay (DS-015):**
1. `graph_snapshots` table stores PG graph state at each `graph_version` bump (skills, subunits, communities — as JSONB snapshot)
2. `replay_session(correlation_id, historical_graph_version)` loads the snapshot, seeds a `SeededGraph` from it, runs retrieval with the original prompt, and compares output to recorded response
3. Golden output bundles stored alongside session logs for replay verification

**Offline deterministic twin (DS-023):**
1. Deterministic mode: frozen clocks (config-gated test clock), fixed embedding provider (deterministic test vectors), seeded random for scope resolution ordering
2. Capture production event traces (Redis streams + PG audit_log for a time window)
3. Replay traces in twin mode, compare outputs, state transitions, and events bit-for-bit
4. Flag non-deterministic divergence with root cause analysis

###### Scope
- **Owns:** Graph snapshot table, replay engine, deterministic twin mode, divergence detection
- **Non-goals:** Full production capture (needs infrastructure beyond Docker Compose), real-time twin mirroring. V3 territory.
- **Scope fence:** Snapshots are point-in-time — continuous replay is V3. Twin mode is for debugging, not shadow deployment.

###### Acceptance criteria
- [ ] `graph_snapshots` table captures graph state per version
- [ ] `replay_session` reproduces historical compile_context output within tolerance
- [ ] Deterministic twin produces identical output for same input
- [ ] Divergence detection flags non-deterministic code paths
- [ ] DS-015 and DS-023 tests are un-ignored and pass

###### Evidence
- **Test command:** `cargo test -p explainability -- replay && cargo test --test test_dream_state_contract -- ds_015 -- --ignored && cargo test --test test_dream_state_contract -- ds_023`
- **Evidence focus:** Replay determinism, twin bit-for-bit match, divergence root cause identification

---

#### Phase 5: Multi-Harness & Ecosystem
**Purpose:** Make the skill layer truly portable across coding agent harnesses. The `SKILL.md` format is universal. V1.1 compiles only for Claude Code. V2 adds compilers for OpenCode, Copilot, and Codex — all behind the same `ContextCompiler` trait. Extraction provider parity ensures skills extracted from Claude Code sessions are usable in OpenCode sessions and vice versa.
**Rationale:** Constitution §4 demands cross-harness portability. The `ContextCompiler` trait was designed for this — `TemplateOnlyCompiler` for Claude Code, `OllamaGuidanceCompiler` for richer guidance, and now harness-specific compilers for each target format. This is the last V1.1 deferred seam.

##### Slice 5.1: Multi-harness compiler implementations
**Slice type:** tracer-bullet
**Serves:** SC-V2-7 (multi-harness portability)
**Demo scenario:** Compile the same scored skill set for Claude Code (`additionalContext` markdown), OpenCode (skill instructions format), Copilot (agent skill format), and Codex (custom instructions format). Verify each output is valid for its target harness.
**Feature home:** `crates/compiler/`
**Files:**
- `crates/compiler/src/opencode.rs` — NEW: `OpenCodeCompiler` implementing `ContextCompiler`
- `crates/compiler/src/copilot.rs` — NEW: `CopilotCompiler` implementing `ContextCompiler`
- `crates/compiler/src/codex.rs` — NEW: `CodexCompiler` implementing `ContextCompiler`
- `crates/compiler/src/lib.rs` — register all compilers
- `crates/domain/src/config.rs` — add `harness: claude | opencode | copilot | codex` config
- `crates/mcp-server/src/tools/compile_context.rs` — select compiler by harness config
- `tests/integration/test_compile_context.rs` — verify all harness formats
**Depends on:** None (standalone compiler implementations)
**Dependency type:** parallel-safe

###### What to build
Four compiler implementations behind the same `ContextCompiler` trait:

1. **`TemplateOnlyCompiler`** (existing V1.1): Claude Code `additionalContext` format — structured markdown with skill name, description, procedures, conventions.

2. **`OpenCodeCompiler`**: OpenCode skill instructions format — tagged skill blocks with `@skill` directives, tool-call examples, and scope annotations.

3. **`CopilotCompiler`**: GitHub Copilot agent skill format — AgentSkills-compatible JSON with `name`, `description`, `capabilities[]`, `instructions[]`, and `examples[]`.

4. **`CodexCompiler`**: OpenAI Codex custom instructions format — concatenated instruction blocks with `## Skill: {name}` headers and inline code examples.

Each compiler accepts the same `Vec<ScoredSkill>` + prompt text. The trait contract shields callers from format differences. Harness selection is a config flag.

###### Scope
- **Owns:** Three new compilers, harness config routing, format validation
- **Non-goals:** Harness-specific retrieval (same retrieval for all), harness-specific MCP hooks (format is the only difference), harness testing infrastructure. Follow-on slices.
- **Scope fence:** Do not change retrieval pipeline. Do not change `compile_context` response contract — only `additional_context` content differs by harness.

###### Acceptance criteria
- [ ] All four compilers implement `ContextCompiler` trait
- [ ] Each compiler produces harness-valid output format
- [ ] Config `harness` field routes to correct compiler
- [ ] Same scored skill set produces different formatted output per harness
- [ ] Integration test verifies all four formats

###### Evidence
- **Test command:** `cargo test -p compiler && cargo test --test test_compile_context`
- **Evidence focus:** Trait implementation × 4, config routing, format validity per harness

##### Slice 5.2: Extraction provider parity verification
**Slice type:** hardening
**Serves:** SC-V2-10, DS-012 (provider parity), SC-V2-7 (cross-harness portability)
**Demo scenario:** Extract skills from the same transcript corpus using both Claude and Ollama providers. Verify output contract shape is identical, and quality floor thresholds are met for both providers. Provider switch does not break ingestion.
**Feature home:** `crates/session-extractor/` + `tests/e2e/`
**Files:**
- `crates/session-extractor/src/providers/parity.rs` — NEW: provider parity test utilities
- `tests/e2e/test_dream_state_contract.rs` — un-ignore DS-012
- `tests/fixtures/parity-transcript.jsonl` — NEW: fixture transcript corpus for parity testing
**Depends on:** Slice 1.2 (map-reduce extraction with quality scoring)
**Dependency type:** real

###### What to build
Provider parity verification:
1. **Contract shape parity:** Both providers produce `ExtractionResult` with identical JSON schema. Same keys, same types. Validated by extracting from the same fixture transcript via both providers and diffing schema.
2. **Quality floor:** Both providers produce skills with `combined_utility_score` above threshold (0.5 baseline). Neither provider should produce utility scores consistently below the other.
3. **Ingestion parity:** Skills from both providers survive the graph ingestion pipeline without errors. Same `.pending` file format.

Fixture transcript corpus with expected quality bands enables deterministic parity testing.

###### Scope
- **Owns:** Provider parity tests, quality floor assertion, fixture corpus
- **Non-goals:** Provider preference ranking, automatic provider selection, provider quality comparison beyond parity. Follow-on slices.
- **Scope fence:** Do not change provider implementations — only test their output equivalence.

###### Acceptance criteria
- [ ] DS-012 test is un-ignored and passes
- [ ] Claude and Ollama produce identical schema for same transcript
- [ ] Both providers meet quality floor (combined_utility_score ≥ 0.5)
- [ ] Provider switch does not break ingestion pipeline

###### Evidence
- **Test command:** `cargo test --test test_dream_state_contract -- ds_012`
- **Evidence focus:** Schema parity, quality floor, ingestion compatibility

##### Slice 5.3: Cross-harness portability verification
**Slice type:** hardening
**Serves:** SC-V2-7 (portability), DS-008 (multi-repo isolation extended)
**Demo scenario:** Create a skill in Claude Code project scope. Promote it to global scope. Compile it for OpenCode. Verify the compiled output is valid and semantically equivalent (same procedures, conventions, assets — different formatting).
**Feature home:** `tests/e2e/` + `crates/compiler/`
**Files:**
- `tests/e2e/test_cross_harness_portability.rs` — NEW: cross-harness portability test suite
- `crates/compiler/src/lib.rs` — add format roundtrip test (compile → parse → recompile for different harness)
- `tests/fixtures/test-skills/portability-skill/SKILL.md` — NEW: fixture skill for portability testing
**Depends on:** Slice 5.1 (all four compilers exist)
**Dependency type:** real

###### What to build
Cross-harness portability test suite:
1. **Format roundtrip:** Skill compiled for Claude Code → parsed back → recompiled for OpenCode → semantically equivalent (same procedures/conventions/assets, different formatting)
2. **Scope portability:** Project-scoped skill → promoted to global → compiled for Copilot → same substance, different scope annotation
3. **No information loss:** Every subunit (procedure, convention, asset) survives format conversion. Counts match across harnesses.
4. **Harness detection:** Compiler auto-detects harness from config and produces correct format without manual intervention.

###### Scope
- **Owns:** Cross-harness portability tests, format roundtrip, information preservation verification
- **Non-goals:** Harness-specific MCP hook config generation (each harness has its own hook format — those are separate skills/docs, not code). Follow-on slices.
- **Scope fence:** Test-only — no production code changes in this slice.

###### Acceptance criteria
- [ ] Format roundtrip preserves all skill content across harness transformations
- [ ] Scope promotion does not alter compiled content (only scope annotation)
- [ ] Subunit count is identical across all four harness compilations
- [ ] Auto-detection routes to correct compiler without manual config

###### Evidence
- **Test command:** `cargo test --test test_cross_harness_portability`
- **Evidence focus:** Content preservation, format correctness, auto-detection accuracy

---

### Slice-to-Story Traceability

| Success Criterion | Delivered by Slice(s) | Demo scenarios |
|---|---|---|
| SC-V2-1: Quality-scored extraction | 1.1, 1.2, 1.3 | `.pending` with quality_scores; map-reduce vs single-pass comparison; quality_scores in PG |
| SC-V2-2: Team scope retrieval | 2.1, 2.2, 2.3, 2.4, 2.5 | Junction table schema; team resolver; three-scope fusion; cross-tenant isolation; team maintenance |
| SC-V2-3: Utility-scored maintenance | 3.4, 1.3 | Quality-weighted retirement; high-quality skill survives longer |
| SC-V2-4: Outcome-based learning | 3.6 | 30-day simulated learning window; threshold tuning |
| SC-V2-5: Autonomous self-healing | 3.1, 3.2, 3.3 | Health probes; degradation detection; self-healing loop |
| SC-V2-6: Counterfactual explainability | 4.1 | Feature contributions; perturbation sensitivity |
| SC-V2-7: Multi-harness portability | 5.1, 5.2, 5.3 | Four compilers; provider parity; cross-harness roundtrip |
| SC-V2-8: LLM-guided compilation | 1.4 | Guidance compiler vs template output |
| SC-V2-9: SkillOpt optimization | 3.5 | `.optimized` proposal; rollout-reflect-edit-gate loop |
| SC-V2-10: Dream-state contracts | 2.4, 3.3, 3.6, 4.1, 4.2, 4.3, 4.4, 5.2 | 12 DS contracts un-ignored and passing |

## Acceptance Criteria

### Functional Requirements
- [ ] Quality-scored extraction produces valid quality dimensions in every `.pending` file
- [ ] Team scope retrieval returns skills from remote PG+Qdrant alongside local scopes
- [ ] Utility-scored retirement preserves high-quality skills that would otherwise be retired
- [ ] Counterfactual explanations are accurate (feature contributions within ±0.05 of score)
- [ ] Multi-harness compilers produce valid output for all four target formats
- [ ] Self-healing recovers from all seven cataloged degradation types
- [ ] SkillOpt optimization loop converges on validation-improving edits

### Non-Functional Requirements
- [ ] `compile_context` template path stays < 500ms (unchanged from V1.1)
- [ ] `compile_context` guidance path < 3s (new SLO for LLM compiler)
- [ ] `explain_ranking` < 200ms (feature contributions are computed, not LLM-generated)
- [ ] Team scope retrieval < 800ms (remote penalty over local 500ms SLO)
- [ ] Health probes complete in < 5s aggregate (all four run concurrently, 3s worst single)
- [ ] Self-healing loop completes remediation in < 30s (bounded retries with backoff)
- [ ] SkillOpt epoch < 5 minutes (batch size 10, local optimizer model)
- [ ] Schema migrations are idempotent and backward-compatible

### Quality Gates
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] `cargo test --workspace` passes (all unit + integration)
- [ ] `docker compose -f docker-compose.test.yml up --abort-on-container-exit` passes (E2E)
- [ ] `cargo tree -p domain --depth 1` shows zero infrastructure deps (CI gate, unchanged from V1.1)
- [ ] At least 12 of 24 dream-state contracts un-ignored and passing
- [ ] Constitution compliance: all five principles verified, zero un-waived violations

## Success Metrics

| Metric | Baseline (V1.1) | Target (V2) | Measured by |
|--------|-----------------|-------------|-------------|
| Skills with quality scores | 0% | 100% | `inspect_skill` query |
| Extraction quality floor (combined_utility_score) | N/A | ≥ 0.5 average | Quality score aggregation |
| Team scope skills retrievable | 0 | Config-dependent | `compile_context` scopes_considered |
| High-quality skills surviving retirement | 0% (usage-only) | ≥ 30% of retired candidates preserved | Retirement audit trail |
| Degraded-state recovery time | Manual (minutes-hours) | < 30s automated | Health event timestamps |
| Counterfactual explanation accuracy | N/A | Feature contributions within ±0.05 of score | `explain_ranking` test assertions |
| Harness compilation coverage | 1 of 4 (Claude Code) | 4 of 4 | Compiler integration tests |
| Dream-state contracts passing | 0 of 24 | ≥ 12 of 24 | `cargo test --test test_dream_state_contract` |
| Optimization epoch convergence rate | N/A | > 50% of epochs produce validation improvement | `optimization_runs` table |

## Dependencies & Prerequisites

- **V1.1 completeness (T01-T14 all green):** V2 executes against trunk with all V1.1 hardening complete. T11-T14 (graceful degrade, session persistence, logging/benchmarks, E2E suite) are prerequisites — V2 builds on a hardened system, not adding features to an incomplete one.
- **Ollama model for guidance compiler:** A stronger local model (e.g., llama3.2:3b or mistral:7b) for `OllamaGuidanceCompiler`. V1.1 uses `nomic-embed-text` for embeddings — this is separate.
- **Ollama model for SkillOpt optimizer:** A stronger model for the optimizer role (reflection + edit proposal). Default: same as extraction provider. Optional: stronger model via config.
- **Optional remote PG+Qdrant for team scope:** Team scope is opt-in. Local-only path remains fully functional without remote infrastructure.
- **SkillOpt requires usage data:** Optimization needs session replay data. Skills without usage history cannot be optimized (no rollout data). This gates Phase 3 until enough sessions have been recorded.
- **No new infrastructure containers:** Team scope uses optional remote services. SkillOpt runs in the `skill-optimizer` container (new, but same Docker Compose topology). All other additions are in existing services.

## Risk Analysis & Mitigation

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Quality rubric degrades extraction for coding domains | Medium | Low | Rubric adapted from SkillLens which proved gains across 5 domains including coding (SWE-bench). We tune domain-specific examples. |
| Team scope adds > 300ms latency to compile_context | High | Medium | Team scope runs concurrently with project+global (already parallel). Configurable timeout. Degraded response if team scope times out. |
| SkillOpt produces worse skills than originals | Medium | Medium | Held-out validation gate rejects worse candidates. `.optimized` requires human approval (constitution §3). Optimizer cannot force-change active skills. |
| Self-healing accidentally mutates skill content | High | Low | Remediation catalog is explicit and bounded. Only infrastructure operations (reconnect, reconcile, rebuild). Zero skill content mutation paths. |
| Counterfactual explanations are misleading | Medium | Medium | Explanations are score-component-based (deterministic), not LLM-generated. Ablation is reproducible. Test assertions bound contribution accuracy. |
| Schema migration breaks V1.1 deployments | High | Low | All migrations are additive (`ADD COLUMN`, `CREATE TABLE`). No `DROP`, no `ALTER TYPE`, no constraint changes. Run on V1.1 schema, verify it still works. |
| Multi-harness compilers produce invalid format | Medium | Low | Each compiler has format-validating tests against harness documentation. Fixture skills generate known-valid output. |
| Too many slices for realistic execution | Medium | Medium | Phases are independently valuable and can be stopped after any phase. Phase 1 alone delivers the biggest quality win. Phases 2-5 are additive depth. |

## Resource Requirements

- **Development time:** 60-90 hours (38 slices, ~1.5-2.5 hours each, excluding Phase 5 harness-specific testing which may need harness access)
- **Infrastructure:** Same Docker Compose topology as V1.1. One new container: `skill-optimizer`. Optional: remote PG+Qdrant for team scope testing.
- **Model requirements:** Local Ollama models for guidance compiler and optimizer. Embedding model unchanged (`nomic-embed-text:768`).
- **Test infrastructure:** Docker test cluster extended with fault injection for health/degradation tests. Multi-tenant fixture corpus for isolation testing. Historical snapshot corpus for replay testing.

## Documentation Plan

- **Architecture artifact:** `docs/architecture/2026-05-26-skill-layer-v2-architecture.md` — produced by `/workflows:architecture`
- **Quality rubric:** `crates/session-extractor/src/meta_skills/quality_rubric.md` — produced by Slice 1.1
- **Team scope setup guide:** `docs/runbooks/team-scope-setup.md` — produced by Slice 2.5
- **Self-healing catalog:** `docs/runbooks/self-healing-remediation-catalog.md` — produced by Slice 3.3
- **Counterfactual explainability API:** `docs/reference/explainability-api.md` — produced by Slice 4.1
- **Multi-harness compilation guide:** `docs/reference/harness-compilation-formats.md` — produced by Slice 5.3
- **Changelog:** `docs/changelogs/v2.0.0.md` — produced after V2 completion

## Alternative Approaches Considered

1. **Skip quality rubric, go straight to SkillOpt.** Rejected: Quality rubric is a 60-minute zero-architecture change that delivers +1.55pp improvement. SkillOpt is a full new service and crate. Quality first, optimize later.

2. **Skip team scope, go straight to multi-harness.** Rejected: Team scope is the minimum viable path to "skills compound across users." Multi-harness is about format portability, not knowledge sharing. Different value propositions, both needed, team scope first.

3. **Build a web dashboard for skill management.** Rejected: Constitution §5 mandates filesystem-observable state and "no web dashboard." The filesystem IS the UI. Admin MCP tools provide command-line introspection.

4. **Use cloud-based LLM for guidance compiler.** Rejected: Constitution §1 requires local-first. `OllamaGuidanceCompiler` uses local models. Cloud API can be added as an alternative adapter behind the same trait (V3).

5. **Merge explainability into retrieval crate.** Rejected: Explainability is a separate concern (explain, don't retrieve). New crate follows the V1.1 pattern of separating retrieval, compilation, and extraction. Prevents retrieval bloat.

## References & Research

### Internal References
- Architecture: `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md` (V1.1 canonical)
- Plan: `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- Assessment: `docs/assessments/2026-05-26-skill-layer-v1-1-deep-grok-assessment.md`
- Constitution: `docs/constitution.md`
- Tickets: `docs/tickets/2026-05-21-skill-layer-v1-1/index.md`
- Dream-state contracts: `tests/e2e/test_dream_state_contract.rs`
- Domain traits: `crates/domain/src/traits.rs`
- Retrieval orchestrator: `crates/retrieval/src/orchestrator.rs:131-446`
- Merge engine: `crates/maintenance/src/merge.rs:161-317`
- Extraction stub: `crates/session-extractor/src/providers/claude.rs` (27 lines)

### External References
- SkillLens paper: arXiv:2605.23899 — `https://arxiv.org/abs/2605.23899`
- SkillLens code: `https://github.com/microsoft/SkillLens`
- SkillLens quality rubric: `https://raw.githubusercontent.com/microsoft/SkillLens/main/data/meta_skills/quality_rubric_3dim.md`
- SkillLens parallel extraction: `https://raw.githubusercontent.com/microsoft/SkillLens/main/skilllens/extraction/parallel.py`
- SkillOpt paper: arXiv:2605.23904 — `https://arxiv.org/abs/2605.23904`
- SkillOpt code: `https://github.com/microsoft/SkillOpt`
- SkillRAE paper: arXiv:2605.10114 — scoring formula eq.3
- Claude Code Skills: `https://docs.anthropic.com/en/docs/claude-code/skills`
- AgentSkills standard: `https://agentskills.io/`

### Related Work
- V1.1 execution sessions: `docs/execution-sessions/` (10 sessions, work-2026-05-22 through work-2026-05-26)
- Todo tracking: `todos/` (38 tracked bugfix/hardening items, predominantly P1/P2)