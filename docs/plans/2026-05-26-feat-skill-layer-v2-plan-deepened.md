---
title: feat: Dynamic Agent Skill Layer V2 — Quality Intelligence, Team Scope, and Self-Evolving Graph
type: feat
status: active
date: 2026-05-26
deepened_date: 2026-05-26
topic: skill-layer-v2
constitution_version: 1.0.0
constitution_waivers: []
brainstorm_ref: null
architecture_ref: docs/architecture/2026-05-26-skill-layer-v2-architecture.md
v1_1_architecture_ref: docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md
plan_ref: docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md
assessment_ref: docs/assessments/2026-05-26-skill-layer-v1-1-deep-grok-assessment.md
research_inputs:
  - SkillLens (arXiv:2605.23899) — 25% negative transfer rate, extractor≠consumer asymmetry, 3-dim quality rubric
  - SkillOpt (arXiv:2605.23904) — 12.8-24.9pp gains, harness-dependent (+24.8 Codex vs +19.1 Claude), cross-harness transfer
  - SkillRAE (arXiv:2605.10114) — scoring formula foundation
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

**Deepened on:** 2026-05-26
**Sections enhanced:** 12 (all phases + cross-harness deep-dive + SkillOpt params + extraction architecture)
**Research agents used:** 10 (SkillLens deep-dive, SkillOpt deep-dive, cross-harness synthesis, multi-harness format research, Phase 1-5 research agents, architecture/security/performance reviews)

### WHY Integrity Check
- Problem Narrative: preserved
- User Story: preserved
- Architectural Context: preserved + expanded (cross-harness asymmetry)
- Success Criteria: preserved
- Execution shape: preserved
- Packet tracing: all packets still trace to user story or enabling outcome: yes

### TDD Contract Check
- Precedence: plan_overrides_local → Ralph TDD enforced
- Effective loop: red-green-refactor
- Evidence: unit required, e2e required
- Exceptions: none

### Key Improvements (from research)

1. **Cross-harness extractor-consumer asymmetry quantified** (serves: SC-V2-7, SC-V2-1): SkillLens proves self-extraction (same model extracts + consumes) is safe. Cross-model pairs risk negative transfer up to -1.60pp on SWE-bench. Added `extraction_source_harness` metadata field to every skill, cross-harness canary test for Phase 5, and per-consumer validation gates in the optimization loop.

2. **SkillOpt parameter corrections** (serves: SC-V2-9): Paper defaults differ from our initial plan. batch_size: 10→40, slow_update_epochs: 3→2, optimizer_model must be stronger-than-extraction (56-74% degradation with weak optimizer). Added cosine schedule for edit budget decay and rejected-edit buffer prompt injection mechanism.

3. **Agentskills.io convergence confirmed** (serves: SC-V2-7): Claude Code, OpenCode, and GitHub Copilot all speak the AgentSkills open standard as of May 2026. Only Codex uses AGENTS.md. This means 3/4 harness compilers share format — the code delta is minimal (frontmatter field stripping). Codex AGENTS.md export deferred to when Codex users exist.

4. **Quality rubric: 3 dimensions + Environment/Tool Semantics** (serves: SC-V2-1): The SkillLens paper's 3-dim rubric works on coding tasks at +0.83pp (SWE-bench). A 4th coding-domain dimension (63.2% better-rate, narrowly below 64% cutoff) addresses tool semantics. Plausibility-based dimensions actively harmful (-0.59pp) — excluded.

5. **Behavioral validation mandate** (serves: SC-V2-3, SC-V2-9): SkillLens proves LLM judges are worse than random on high-Δ pairs (15.8%). All gates must be behavioral (rollout outcomes), not rubric-judgment-based. This affects merge proposal validation, SkillOpt gates, and provider parity testing.

6. **Map-reduce: G=5 for coding transcripts** (serves: SC-V2-1): Coding transcripts are 5-10x longer than SkillLens domains. Smaller merge groups (G=5) reduce information loss. Added rare-pattern preservation override.

7. **Cross-harness minimum viable additions** (serves: SC-V2-7): +1 metadata field, +1 canary test, +N validation gates in optimization. ~4 engineering hours. Everything else (per-harness extraction, per-harness optimization runs) waits for data.

### New Considerations Discovered

- **Extractor-consumer asymmetry**: Claude→Codex-mini pair maps to paper's -1.0pp SWE-bench pair. Must behavioral-validate cross-model pairs before enabling cross-harness skill sharing.
- **Optimizer model strength**: If extraction uses local 8B Ollama, that same model cannot produce quality optimization diagnoses. Need stronger optimizer model (remote frontier or strongest local 14B+).
- **Cross-consumer optimization gates**: A skill optimized for Claude may regress Ollama. Gate must test against ALL consumer models.
- **Self-assessment unreliability**: Separate evaluator model recommended over in-prompt self-assessment.
- **AgentSkills convergence reality**: Multi-harness is mostly frontmatter field stripping. The compilers are thin adapters, not content transformers.
- **Team scope isolation architecture**: Confirmed defense-in-depth pattern with read-time sanitization, Blake3 provenance hashing, canary token verification, and cross-origin merge blocking. Top-3 leak vectors: file paths, env vars, origin metadata. Remote Qdrant uses single collection with `scope_id` payload filter.

### Scope Warnings

- **Per-harness extraction pipelines**: Deferred to V3. No cross-harness utility data exists to justify N extraction pipelines. Wait for canary test results.
- **Per-harness SkillOpt optimizer models**: Deferred to V3. One optimizer + N validation gates is sufficient until data shows systematic harness-specific regression.
- **Codex AGENTS.md export**: Deferred. Build for Claude/OpenCode/Copilot first. Codex users can manually convert.
- **Automatic cross-harness skill promotion**: Never. Human gate (constitution §3) applies. Cross-harness auto-promotion before utility data exists is premature.

### Simplifications Applied

- **batch_size: 10→40** (matches paper default; 10 was below minimum effective batch)
- **slow_update_epochs: 3→2** (paper starts slow update at epoch 2)
- **Added cosine edit budget schedule** (paper's best stable schedule, 4→2 decay)
- **optimizer_model: requires explicit stronger model** (not same-as-extraction default)
- **Removed any implication of LLM-judge-based gating** (paper proves harmful)

---

## Cross-Harness Compatibility Deep-Dive (Papers Analysis + Synthesis)

### SkillLens: Extractor-Consumer Asymmetry

**Finding:** A model can be a strong extractor yet a weak consumer — and vice versa. Skill utility is independent of model scale or baseline task strength. On SWE-bench, self-extraction (same model extracts + consumes) is always safe. Cross-model pairs carry risk.

| Extractor | Target | SWE-bench Δ | Risk |
|-----------|--------|-------------|------|
| GPT-5.4 → GPT-5.4 | Self-extract | +4.67 | Safe |
| Qwen3.5-35B → Qwen3.5-35B | Self-extract | +2.00 | Safe |
| GPT-5.4 → Qwen3.5-9B | Strong→Weak | -1.07 | Negative transfer |
| Qwen3.5-35B → Gem-3.1-Pro | Weak→Strong | -1.60 | Negative transfer |
| GPT-5.4-mini → Qwen3.5-9B | Counterexample | +2.40 | Unpredictable |

**Our risk mapping:**
| Our pair | Risk | Reason |
|----------|------|--------|
| Claude→Claude | LOW | Self-extraction always positive |
| Ollama→Ollama | LOW | Self-extraction always positive |
| Claude→Ollama | HIGH | Maps to GPT-5.4→Qwen3.5-9B (-1.07) |
| Ollama→Claude | HIGH | Maps to Qwen3.5-35B→Gem-3.1-Pro (-1.60) |
| Claude→Codex | LOW-MED | Maps to cross-harness but same model family (+4.0) |
| Claude→Codex-mini | HIGH | Maps to GPT-5.4→GPT-5.4-mini (-1.0) |

**Key implication:** Self-extraction is the safe default. Cross-model pairs require behavioral validation before enabling. Cross-harness pairs (different injection mechanisms, same underlying model) fare better than cross-model pairs.

### SkillOpt: Harness-Dependent Gains

**Finding:** +23.5 (direct chat), +24.8 (Codex), +19.1 (Claude Code) — 5.7pp spread. Explained by baseline anchoring: Codex starts lower, leaves more headroom. Procedural edits transfer well; format-specific edits degrade.

**Cross-harness transfer:** Codex→Claude Code skills retain +59.7pp (procedural domains) vs +1.6pp (format-sensitive domains). Substance transfers, format doesn't.

**Optimizer model strength:** Weak optimizer (target-matched model) = 56-74% of strong-optimizer gains. The optimizer model must be explicitly stronger than the extraction provider.

### Decisions

| | Extract | Optimize | Quality score | Format compile |
|---|---|---|---|---|
| **V2** | ONE pipeline + `extraction_source_harness` tag | ONE loop + N validation gates | 3 universal dims + tool semantics | 4 format compilers |
| **V3** | Per-harness prompts (if data warrants) | Per-harness optimizer models (if data warrants) | Harness-specific dims (if data warrants) | Content adaptation (if data warrants) |
| **Never** | Per-harness extraction without utility data | Auto-approval for any harness | Quality dims without validation | Assuming format alone solves portability |

---

## Problem Narrative

_(Unchanged from original plan — preserved as WHY anchor)_

V1.1 delivers a working skeletal system: skills are extracted from sessions, stored in a graph, retrieved by relevance, and maintained through merge/retire workflows. But the skeleton has three critical gaps that prevent it from being truly useful:

First, **extraction is blind**. The extractor produces skill candidates with no signal about whether they'll actually help. SkillLens proved that LLM-generated skills cause negative transfer in 25% of extractor-target pairs, and that LLM judges cannot distinguish good from bad skills (46.4% accuracy). V1.1's extraction trusts the model blindly. We need quality dimensions that predict downstream utility.

Second, **the system is solo-only**. V1.1's constitution explicitly defers team scope to V2. A solo developer's skills should compound across their machine, but a team's collective intelligence — skills learned by one developer that help another on a different repo — remains locked in per-machine silos. Cross-repo collective intelligence (DS-017) is impossible without a shared scope.

Third, **the system is static**. V1.1 maintains skills through deduplication and retirement, but never improves them. SkillOpt proved that skills can be optimized through a training loop — rollout → reflect on failures → bounded edits → validation gate — achieving 12.8-24.9pp improvements across 7 models and 6 benchmarks without touching model weights. V1.1's maintenance deletes stale skills. V2 should evolve them.

Beyond these three gaps, V1.1's architecture deliberately left seams for V2: `ContextCompiler` trait supports LLM-guided compilation, `ScopeResolver` trait supports remote team scope, `MergeSemanticVerifier` supports held-out validation. Filling these seams is additive migration, not rewrite.

---

## User Story

_(Unchanged from original plan — preserved as WHY anchor)_

As a solo developer who is now part of a team,
I need skills extracted from my sessions to be quality-scored so bad skills never pollute the graph, skills from teammates' repos to be discoverable through a shared team scope, and the system to actively improve my skills over time through validated optimization,
so that every session starts with higher-quality context than the last, skills compound across the team, and the graph gets smarter with use rather than just accumulating drift,
because currently V1.1 extracts blindly (25% of skills may be harmful), operates in per-machine isolation (team knowledge is locked in silos), and only deletes stale skills rather than improving the ones we keep,
which causes extraction distrust, zero cross-repo intelligence, and a graph that rots slightly slower but never gets better.

### Secondary Story: Operator with Explainability Needs

_(Unchanged)_

### Tertiary Story: Multi-Harness Developer

_(Unchanged)_

---

## Architectural Context

_(Unchanged from original plan — preserved as WHERE map)_

[...system placement, feature homes, interactions, constitution compliance, boundary constraints unchanged...]

### Research Insights: Cross-Harness Architecture

**Serves:** SC-V2-7 (multi-harness portability)

**AgentSkills Convergence (2026):**
Claude Code, OpenCode, and GitHub Copilot all converge on the `agentskills.io` open standard as of May 2026. This is not speculative — it's documented and live. A `SKILL.md` written for one harness structurally works in all three. Only OpenAI Codex uses `AGENTS.md` as its sole instruction format (no YAML frontmatter, no `SKILL.md` concept).

**Per-Harness Format Differences:**

| Feature | Claude Code | OpenCode | Copilot | Codex |
|---------|-------------|----------|---------|-------|
| AgentSkills standard | Full | Core | Extended | None |
| Format | SKILL.md + frontmatter | SKILL.md + frontmatter | SKILL.md + frontmatter | AGENTS.md (plain MD) |
| Progressive loading | 2-level | name+desc then full | 3-level | Always-on |
| Fork isolation (`context: fork`) | Yes | No | Yes (experimental) | No |
| Shell injection (`` !`cmd` ``) | Yes | No | No | No |
| Tool permissions (`allowed-tools`) | Yes | No | Yes | No |
| Path-scoped skills (`paths`) | Yes | No | No | No |
| Description max | 1536 chars | 1024 chars | 1024 chars | N/A |

**Minimum Portable Format (works on 3/4 harnesses):**
```yaml
---
name: skill-name
description: What this does and when to use it
---
# Markdown body
```

**Compiler Architecture Decision:** Generate per-harness output from a single canonical skill representation. Do NOT try to produce one universal format — the union of features is too small (name + description + markdown). Each harness needs the skill in slightly different wrapping:

```
Canonical Skill (in graph, SKILL.md)
         │
    ┌────┼────┬─────────┐
    ▼    ▼    ▼         ▼
 Claude  OpenCode  Copilot  Codex
(MCP hook  (<skills>  (agent  (AGENTS.md
 inj format)  XML)    context)  section)
```

The compilers are thin adapters: same skill body, different frontmatter wrapping. Claude-specific extensions (fork, shell injection, paths) are stripped for other harnesses.

---

## Success Criteria

_(Unchanged from original plan — preserved as DONE definition)_

[...10 criteria unchanged...]

### Research Insights: Success Criteria Risk Assessment

**SC-V2-1 (Quality-scored extraction):** SkillLens predicts +0.5-1.0pp in coding (vs +1.55pp across all domains). The SWE-bench cells averaged +0.83pp. Quality rubric is beneficial but don't over-claim.

**SC-V2-7 (Multi-harness portability):** 3 of 4 harnesses speak AgentSkills. Compilers are mostly frontmatter stripping — format is the easy part. The hard part is skill content quality across consumer models (see Cross-Harness Deep-Dive above). Add behavioral validation test.

**SC-V2-9 (SkillOpt optimization):** Paper proves +12.8-24.9pp with STRONG optimizer model. With local Ollama (weaker model), expect 56-74% of theoretical ceiling. Realistic: +7-18pp, not +20pp. Manage expectations.

---

## TDD & Evidence Contract

_(Unchanged from original plan)_

---

## Execution Shape

_(Unchanged from original plan)_

---

## Constitution Alignment

_(Unchanged from original plan)_

---

## Stakeholder Impact

_(Unchanged from original plan)_

---

## Technical Approach

### Architecture Overview

_(Unchanged from original plan)_

### Research Insights: Overall Architecture

**Serves:** SC-V2-1 through SC-V2-10

**Safety Architecture — Behavioral Validator Trait:**

Based on SkillLens finding that LLM judges are 46.4% accurate (and 15.8% on high-stakes decisions), the plan needs one architectural addition: a `BehavioralValidator` trait that standardizes utility verification across all gating decisions:

```rust
#[async_trait]
pub trait BehavioralValidator: Send + Sync {
    async fn validate(&self, skill: &Skill, held_out_sessions: &[Session]) -> ValidationResult;
}

struct ValidationResult {
    rollout_utility: f32,          // measured Δ on held-out set
    consumer_utilities: HashMap<ConsumerModel, f32>,  // per-consumer breakdown
    is_improvement: bool,          // improvement over no-skill baseline
    degradation_warnings: Vec<ConsumerModel>,  // models that regressed
}
```

This trait should back: SkillOpt gates (Slice 3.5), merge proposal validation, provider parity (Slice 5.2), and cross-harness portability (Slice 5.3).

**AgentSkills Convergence Impact:**

The multi-harness compiler architecture simplifies dramatically: 3 of 4 harnesses share the core `SKILL.md` format. The `CopilotCompiler` and `OpenCodeCompiler` are essentially Claude Code compiler minus Claude-specific frontmatter fields. Implementation effort drops from 3 full compilers to 1 baseline + 2 strippers + 1 AGENTS.md converter (deferred).

### Execution Slices

#### Phase 1: Quality Intelligence

_(Phase 1 Purpose and Rationale — unchanged)_

### Research Insights: Quality Intelligence

**Serves:** SC-V2-1 (quality extraction), SC-V2-8 (LLM compilation)

**Quality Rubric Design:**

The 3-dimension rubric (Failure Mechanism Encoding, Actionable Specificity, High-Risk Action Blacklist) is validated across 9 domain×target cells at 64-66% per-dimension better-rate. The rubric-guided judge reaches 73.8% pairwise accuracy (vs 46.4% unguided).

Critical additions for coding domain:

1. **Add Environment/Tool Semantics dimension** (63.2% better-rate in SkillLens, excluded from validated set by 0.8% margin). For coding: "Does the skill encode how `cargo test`, `docker compose`, `git rebase`, or `rustc --check` actually behave vs what the agent might assume?" This is closer to actionable specificity for coding than for ALFWorld.

2. **Explicitly exclude plausibility-based dimensions.** SkillLens proves the plausibility rubric HURTS extraction (-0.59pp). Do NOT include dimensions about "clarity," "conciseness," "formatting quality," or "completeness" — they are anti-correlated with utility.

3. **Self-assessment vs separate evaluator.** Don't rely on in-prompt self-assessment — LLMs overestimate their own output quality (self-enhancement bias). Use a separate evaluator model (Ollama with a smaller model like llama3.2:3b) to score quality post-extraction.

**Map-Reduce Group Size:**

SkillLens used G=10. For coding transcripts (5-10x longer than ALFWorld trajectories), reduce to G=5 to prevent information loss. Three failure modes identified:

1. **Overgeneralization in merge**: A rare-but-critical failure mechanism appears in one trajectory and gets dropped as "too narrow" in the first merge. Fix: `min_pattern_frequency=1` override for patterns with quality dimension >0.7.

2. **Success-bias in synthesis**: Consolidation prompt favors patterns seen in multiple trajectories — this keeps generic patterns and drops rare-but-critical ones.

3. **Model capability cliff**: SkillLens explicitly notes Qwen3.5-9B "cannot reliably follow the structured extraction protocol." Local Ollama models (7B range) are at risk. Verify before deploying: (a) produces valid JSON for mode extraction, (b) merges at G=5 without hallucinating, (c) executes tool-calling synthesis. If not: single-pass with quality rubric is the fallback.

**LLM Guidance Compiler:**

Recommended local model: `llama3.2:3b` Q4_K_M with `num_ctx: 2048`, `num_predict: 512`. Timeout: 2.5s (not 3s) with template fallback. Routing: 1-2 no-conflict fragments → template path; 3+ or conflicts → LLM synthesis path.

##### Slice 1.1: Quality rubric integration into extraction prompts

_(Slice definition unchanged)_

### Research Insights: Quality Rubric

**Serves:** SC-V2-1

**Tool-calling for structured extraction:**

| Provider | Mechanism | Notes |
|----------|-----------|-------|
| Claude | Tool use with `strict: true` | Guaranteed JSON schema conformance |
| Ollama | `format: "json"` | Best-effort JSON, post-processing required |

Always post-process LLM output for budget enforcement — LLMs cannot count tokens reliably.

**Quality score propagation:**

Add `quality_scores` to `.pending` YAML frontmatter as display-only signal. Do NOT build an automated quality-based gate at V2 launch. Start with display-only, then after 30+ sessions of behavioral validation data, analyze: "What quality threshold would have rejected skills that eventually got retired?" Set the threshold based on outcome data, not rubric scores alone.

##### Slice 1.2: Map-Reduce extraction architecture

_(Slice definition unchanged)_

### Research Insights: Map-Reduce

**Serves:** SC-V2-1, SC-3

**Merge group size adjustment:**

| Parameter | Original Plan | Research-Adjusted | Reason |
|-----------|--------------|-------------------|--------|
| `merge_group_size` | 10 | **5** | Coding transcripts 5-10x longer; smaller groups reduce info loss |
| `max_modes_per_trajectory` | 3 | 3 (keep) | Matches SkillLens |
| `max_skills` | 3 | 3 (keep) | Matches SkillLens |
| `max_skill_chars` | 3000 | 3000 (keep) | Budget constraint |

**Rare-pattern preservation:**
Add `min_pattern_frequency: 1` override for failure patterns with any quality dimension >0.7. The consolidation prompt currently "drops vague or low-value patterns" — add: "preserve at least one instance of each unique failure class even if it appears in only one trajectory."

**Model capability verification:**
Before deploying map-reduce on a local Ollama model, run a capability verification test: extract from a 5-trajectory fixture, verify merge output validity, verify tool-calling synthesis produces valid SkillStore operations. If any phase fails: fall back to single-pass extraction with quality rubric.

##### Slice 1.3: Quality scoring in domain model and PG schema

_(Slice definition unchanged)_

### Research Insights: Schema

**Serves:** SC-V2-1, SC-V2-3

**JSONB GIN indexing:**
```sql
ALTER TABLE skills ADD COLUMN quality_scores JSONB;
CREATE INDEX idx_skills_quality ON skills USING GIN (quality_scores);
```
GIN indexing on JSONB enables querying quality score ranges for maintenance and analytics. Add `extraction_source_harness` and `extraction_source_model` columns to `session_logs` for cross-harness tracking.

**Additional tracking columns:**
```sql
ALTER TABLE session_logs ADD COLUMN success_ratio FLOAT DEFAULT 0.5;
ALTER TABLE session_logs ADD COLUMN extraction_source_harness TEXT; -- 'claude_code', 'opencode', 'copilot', 'codex'
ALTER TABLE session_logs ADD COLUMN extraction_source_model TEXT;   -- 'claude-sonnet-4-20250514', 'llama3.2:3b', etc.
ALTER TABLE skill_usage ADD COLUMN consumer_harness TEXT;           -- which harness consumed this skill
ALTER TABLE skill_usage ADD COLUMN consumer_model TEXT;             -- which model consumed this skill
```

This enables per-consumer-model utility tracking (critical for cross-harness validation).

##### Slice 1.4: LLM-synthesized context compiler

_(Slice definition unchanged)_

### Research Insights: LLM Compilation

**Serves:** SC-V2-8

**System prompt structure:**
```
You are a skill synthesis compiler. Given a set of scored skills and a task prompt, 
produce guidance that:
1. Synthesizes across skills, don't concatenate
2. Prioritizes high-quality skills (use quality_scores field)
3. Highlights cross-skill conflicts
4. Includes rescue cues from below-threshold skills that are still relevant

Output format: Task-specific guidance in markdown. Stay under 500 tokens.
```

**Timeout and fallback:**
Timeout at 2.5s (leaves 500ms margin for the 3s SLO). On timeout: log warning, fall back to `TemplateOnlyCompiler`, return response with `compiler: "template"` metadata. The MCP client sees the same response shape — only `additional_context` content differs.

**Quality-aware routing:**
- 1-2 skills, no quality conflicts → template path (sub-500ms)
- 3+ skills OR quality conflicts detected → guidance path (1-3s)
- Config toggle: `compiler_mode: auto | template | guidance`

##### Slice 1.5: Remote embedding provider abstraction

_(Slice definition unchanged)_

### Research Insights: Embeddings

**Serves:** SC-V2-2 preparation

**Fallback chain:** Local Ollama → Remote endpoint → `DeterministicEmbeddingGenerator` (last resort). Each fallback logs the transition with reason code. Circuit breaker: after 3 consecutive remote failures, skip remote for 5 minutes.

**Dimension validation:**
```rust
fn validate_dimensions(embedding: &[f32], expected: usize) -> Result<(), EmbeddingError> {
    if embedding.len() != expected {
        return Err(EmbeddingError::DimensionMismatch {
            expected,
            got: embedding.len(),
        });
    }
    Ok(())
}
```

---

#### Phase 2: Team Scope

_(Phase 2 Purpose and Rationale — unchanged)_

### Research Insights: Team Scope

**Serves:** SC-V2-2, DS-017

**Cross-tenant isolation — Defense in depth:**

The top-3 leak vectors in shared knowledge systems are: file paths (e.g., `/home/alice/projects/secret-project/`), environment variable patterns (API keys, connection strings), and origin metadata (`promoted_by`, `origin_repo` details). Defense in depth across all access layers:

1. **At write time:** Blake3 provenance hash computed from `content + origin_repo + promoted_at`. Hash is versioned: `blake3:v1:abc123...` to prevent future hash algorithm breakage. Immutable after promotion — any content change invalidates the hash.

2. **At read time (sanitization):** Read-time path stripping via regex matching on patterns like `(/home/[\w-]+/|/Users/[\w-]+/|/root/)`. Environment variable patterns stripped (`[A-Z][A-Z0-9_]{4,}=`). `origin_repo` and `promoted_by` stripped from team scope responses. The `provenance_hash` field is preserved (non-leaking, verifiable). Content body stripped of repository-specific paths via known path regexes.

3. **At merge time:** Cross-origin team merges BLOCKED. Merge candidates must share `origin_repo` hash in provenance. This is a security boundary, not a convenience feature — cross-origin merges would silently blend different teams' domain knowledge.

4. **Canary token detection (DS-017):** Test fixtures embed double-blind canary tokens — unique strings per tenant, salted per test run. The token format: `CANARY_{tenant_id}_{uuid}` embedded in skill bodies. Verification test asserts zero canary tokens appear in retrieval output across all tenant queries. This proves negative: no cross-tenant leakage. Per-tenant salt prevents false positives from token collision.

**Sanitization is read-time, not write-time.** Skills stored in team scope retain full fidelity. Sanitization applies at retrieval response construction — the `additional_context` field is cleaned before delivery. This preserves skill accuracy while preventing information leakage.

**Top-3 tenant-specific patterns to strip:**

```rust
const PATH_PATTERNS: &[&str] = &[
    r"/home/[\w.-]+/",           // Linux home dir
    r"/Users/[\w.-]+/",           // macOS home dir
    r"/root/",                    // Root home
    r"C:\\Users\\[\w.-]+\\",     // Windows home
];

const ENV_PATTERNS: &[&str] = &[
    r"\b[A-Z][A-Z0-9_]{4,}=[^\s]{8,}",  // ENV_VAR=value (min 8-char value)
];

const METADATA_STRIP_FIELDS: &[&str] = &[
    "origin_repo", "promoted_by", "promoted_at",
];
```

**Remote connection pooling and degradation:**

Two separate PG connection pools — never shared:
- **Local pool:** 20 connections (V1.1 default, unchanged)
- **Remote pool:** 5 connections (team scope is opt-in, lower expected load)
- Health check: `SELECT 1` with 2s timeout
- Connection failure → team scope **absent** (not degraded). Returns `Ok(vec![])` not `Err(...)`. Team scope absence is NOT a system error — it's a configuration choice.

**Per-scope timeout architecture:**

| Scope | Timeout | Weight in RRF | Rationale |
|-------|---------|---------------|-----------|
| Project | 400ms | 1.0 (baseline) | Local Qdrant, fastest |
| Global | 400ms | 0.7 | Local Qdrant, broader scope |
| Team | 800ms | 0.5 | Remote Qdrant, highest latency |
| Any scope timeout | Omit from results | N/A | Degraded (missing scope), not failed |

Each scope runs concurrently via `tokio::spawn`. Team scope timeout does not block project/global results. If team scope times out: `compile_context` returns `degraded` with `scopes_considered: [project, global]`. If ALL scopes time out: returns `degraded` with empty results.

**Qdrant multi-collection architecture:**

Hybrid approach — not purely collection-per-scope:

| Collection | Location | Content | Config |
|-----------|----------|---------|--------|
| `project_skills` | Local Qdrant | Project-scoped vectors | V1.1 default |
| `global_skills` | Local Qdrant | Global-scoped vectors | V1.1 default |
| `skills_team` | Remote Qdrant | Team-scoped vectors | Single collection with `scope_id` payload filter |

**Why remote hybrid:** Remote Qdrant uses a single collection with `scope_id` payload filter rather than collection-per-team. This avoids collection proliferation as teams grow. The `scope_id` field in payload differentiates which team's skills to retrieve. `on_disk_payload: true` for team collection (read-heavy, write-rare — promotions are infrequent events). Expected remote latency: ~40ms typical, ~800ms worst-case (network + Qdrant search).

**Junction table design:**

```sql
CREATE TABLE skill_scopes (
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    scope_type TEXT NOT NULL CHECK (scope_type IN ('project', 'global', 'team')),
    scope_id TEXT NOT NULL,       -- team: "github.com/org/team-name"
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    provenance_hash TEXT,          -- Blake3 hash for team scope entries
    origin_repo TEXT,              -- source repo (stripped at read time)
    promoted_by TEXT,              -- developer who promoted (stripped at read time)
    PRIMARY KEY (skill_id, scope_type, scope_id)
);

-- Covering index for hot scope query path
CREATE INDEX idx_skill_scopes_type_id ON skill_scopes (scope_type, scope_id);
```

The `skills.scope` column stays as **primary scope** for V1.1 backward compatibility. V1.1 code reads `skills.scope` — unchanged. V2 code queries `skill_scopes` for additional scope memberships. DELETE cascades: removing a skill from the junction table removes its team scope membership but NOT the skill itself.

**Cross-scope merge and retirement policies:**

Merge priority when duplicates exist across scopes:

| Duplicate across | Winner | Rationale |
|-----------------|--------|-----------|
| Team + Project | **Team** | Team scope has been human-reviewed for promotion |
| Team + Global | **Team** | Team is collective intelligence; global is personal |
| Project + Global | **Project** | Project scope is freshest, most context-specific |

Cross-tenant merges (different `origin_repo` hashes in team scope): **BLOCKED permanently.** This is a security boundary, not a convenience feature.

Team scope retirement: proposals-only (constitution §3). Grace periods:
- Default: 90 days of zero usage across all team members → `.retired` proposal
- Low-quality skills (combined_utility_score < 0.3): 30 days
- High-quality skills (combined_utility_score > 0.7): 180 days

"Zero usage" means: no team member's `compile_context` included this skill in results for 90+ days. Per-team-member usage tracking via `skill_usage` table with `consumer_harness` and `scope_id` aggregation.

**Slices 2.1-2.5:** _(Slice definitions from original plan unchanged — research confirms architecture is sound)_

---

#### Phase 3: Self-Evolving Graph

_(Phase 3 Purpose and Rationale — unchanged)_

### Research Insights: Self-Evolving Graph

**Serves:** SC-V2-3, SC-V2-4, SC-V2-5, SC-V2-9

**Health Probe Pattern:**

```rust
#[async_trait]
pub trait HealthProbe: Send + Sync {
    async fn check(&self) -> HealthStatus;
    fn dependency_name(&self) -> &'static str;
}

struct HealthStatus {
    dependency: String,
    status: ServiceHealth, // Healthy | Degraded | Unavailable
    reason_code: Option<String>,
    latency_ms: u64,
}
```

FSM for health state: `Healthy → Degraded → Unavailable → Degraded (via probe recovery) → Healthy`. State change detection triggers events but health is cached for 30s to prevent compile_context latency blowout.

**Error classification before retry:**
Classify errors as `retryable` / `not_retryable` / `transient` before retry decision. Auth/permission failures never retried. `graph_version_mismatch` circuit-breaks immediately (structural state inconsistency, not recoverable via retry).

**Self-healing — Idempotent remediation:**
All 7 cataloged actions must be idempotent (safe to execute N times). For example: `qdrant_collection_missing` → drop-if-exists + create fresh (drop of non-existent is no-op). Every remediation audited: `remediation_events` table with correlation_id.

**Utility-Scored Retirement:**
Start with conservative quality weight:
```
Phase 1 (V2 launch): usage = 80%, quality = 20%
Phase 2 (V2 + 30 days): usage = 70%, quality = 30%
Phase 3 (V2 + 90 days): usage = 60%, quality = 40%
```
The shift happens only when behavioral canary tests confirm quality scores predict utility in our domain. Confidence calculation: `n / (n + 30)` scaling where n is behaviorally-validated skill count.

**SkillOpt Parameter Corrections:**

| Parameter | Original Plan | Research-Adjusted | Reason |
|-----------|--------------|-------------------|--------|
| `batch_size` | 10 | **40** | Paper minimum effective is 24, default is 40 |
| `edit_budget` (start) | 4 | 4 (keep) | Matches paper optimal |
| `edit_budget` (schedule) | static | **cosine decay to floor 2** | Paper's best stable schedule |
| `slow_update_epochs` | 3 | **2** | Paper starts slow update at epoch 2 |
| `epochs` | 4 | 4 (keep) | Matches paper |
| `optimizer_model` | same-as-extraction | **stronger-than-extraction** | Weak optimizer = 56-74% of ceiling |
| `validation_strictness` | "exceeds" | **"strictly exceeds"** (ties rejected) | Paper: ties are noise |
| `reflection_minibatch` | not specified | **8** | Paper default |
| `refinement_rounds` | not specified | **3** | Teacher reflection rounds |

**SkillOpt Minimum Prerequisites:**
| Prerequisite | Minimum | Our equivalent |
|---|---|---|
| Scored trajectories | 1 (gives 47.5 on SpreadsheetBench, 61% of ceiling) | Skills with ≥5 usage samples |
| Held-out validation split | 20% (4:1 train:test) | validation_split: 0.2 (matches) |
| Rollout batch size | 40 (8 minimum) | batch_size: 40 (corrected) |
| Epochs | 4 | 4 (matches) |
| Optimizer model | Separate frontier model | Must be explicitly stronger than extraction |

**SkillOpt Architecture Additions:**

1. **Rejected-edit buffer as prompt injection:** Buffer stores failed edits + score drops. `reflect.rs` injects them as "Previously rejected edits (avoid these):" into optimizer prompt. Buffer resets per epoch.

2. **Protected slow-update sections:** Skill document gets `<!-- PROTECTED_START -->` / `<!-- PROTECTED_END -->` markers. Step-edits cannot overwrite protected sections. Only epoch-end slow/meta update writes to them.

3. **Cross-consumer validation gate:** When optimizing a skill, test candidate against ALL consumer models. Accept criterion: "improves at least one consumer without degrading any other." This is stricter than SkillOpt (which optimizes for one target) but necessary for multi-harness.

4. **Cosine edit budget schedule:**
```rust
fn edit_budget(epoch: usize, total_epochs: usize, max_budget: usize, floor: usize) -> usize {
    let progress = epoch as f64 / (total_epochs - 1) as f64;
    let cosine = 0.5 * (1.0 + (progress * std::f64::consts::PI).cos());
    floor + ((max_budget - floor) as f64 * cosine).round() as usize
}
```

##### Slice 3.1: Health probe trait and concrete implementations

_(Slice definition unchanged)_

##### Slice 3.2: Health event publishing and degradation detection

_(Slice definition unchanged)_

##### Slice 3.3: Autonomous self-healing loop

_(Slice definition unchanged)_

### Research Insights: Self-Healing

**Serves:** SC-V2-5

**Idempotent remediation catalog (constitutional compliance verified):**

| Reason Code | Remediation | Safe for auto? | Idempotent? |
|-------------|-------------|----------------|-------------|
| `embedding_provider_unavailable` | Wait and retry + fallback chain | Yes | Yes (stateless retry) |
| `qdrant_collection_missing` | Drop-if-exists + create fresh | Yes | Yes (drop non-existent = no-op) |
| `qdrant_orphan_vectors` | Purge by filter | Yes | Yes (delete non-existent = no-op) |
| `watcher_stale` | Force heartbeat + reconciliation | Yes | Yes (idempotent signal) |
| `graph_version_mismatch` | **ESCALATE ONLY** | **No** | N/A (structural inconsistency) |
| `pg_connection_lost` | Reconnect pool + verify | Yes | Yes (connection retry) |
| `outbox_backlog` | Drain at reduced rate | Yes | Yes (consumer retry) |

Escalation triggers (to human/admin):
- `graph_version_mismatch` → immediate P1
- Same entity fails >3x in 24 hours
- Correlated failures across >30% of nodes
- Any skill content mutation attempt → P0 constitutional violation

**Redis Stream topology:**
```
health:events        → degradation_detected, system_recovered
remediation:actions  → remediation_attempted, succeeded, failed, escalated
remediation:dlq      → dead letter after max retries
```

##### Slice 3.4: Utility-scored retirement with quality awareness

_(Slice definition unchanged)_

### Research Insights: Utility Retirement

**Serves:** SC-V2-3

**Quality weight phasing:**
Don't start at 40% quality weight. Phase it in as behavioral validation data accumulates:

```
Phase 1 (V2 launch):     usage = 0.80, quality = 0.20  (conservative)
Phase 2 (V2 + 30 days):  usage = 0.70, quality = 0.30
Phase 3 (V2 + 90 days):  usage = 0.60, quality = 0.40  (plan target)
```

Shift gates: only advance to next phase when behavioral canary confirms quality scores predict utility in our coding domain with ≥0.3 Pearson r.

**Confidence-scaling formula:**
```rust
fn confidence_weight(n_samples: usize) -> f32 {
    let n = n_samples as f32;
    (n / (n + 30.0)).sqrt()  // asymptotically approaches 1.0
}
```

##### Slice 3.5: SkillOpt optimization service

_(Slice definition — parameters updated per Research Insights table above)_

##### Slice 3.6: Outcome-based learning for threshold tuning

_(Slice definition unchanged)_

### Research Insights: Outcome-Based Learning

**Serves:** SC-V2-4

**Signal collection:**
- Acceptance: `.pending` → `.md` (positive signal)
- Rejection: `.pending` → `.rejected` (negative signal)
- Usage: skill appears in `skill_usage` with `context_status: ok` (positive utility signal)
- Non-usage: skill never used despite being active for 30+ days (negative utility signal)

**Minimum signal floor:** 30 samples before any tuning. For proper statistical power (Cohen's h=0.2, power=0.8): ~393 samples. Use binomial test + Wilson score CI for proportions. Regression guard is automatic: any threshold change that would reject a previously-accepted skill is blocked without human override.

---

#### Phase 4: Trust & Observability

##### Slice 4.1: Counterfactual explainability engine

### Research Insights: Counterfactuals

**Serves:** SC-V2-6

**Shapley computation strategy:**
With 4 features (semantic, lexical, prior, community_boost), exact Shapley values via full enumeration: 2^4 = 16 marginal evaluations. Deterministic score function → exact values, zero sampling error. Computation time: <1ms.

**Value function (ablation semantics):**
```rust
fn marginal_score(features: &FeatureVec, coalition: u8) -> f64 {
    let mut masked = FeatureVec::default();
    if coalition & 0b1000 != 0 { masked.semantic = features.semantic; }
    if coalition & 0b0100 != 0 { masked.lexical = features.lexical; }
    if coalition & 0b0010 != 0 { masked.prior = features.prior; }
    if coalition & 0b0001 != 0 { masked.cb = features.community_boost; }
    scoring_function(&masked)
}
```

**Counterfactual perturbation search:**
Greedy beam search with A*-style heuristic. Edit operations: token deletion, token insertion (domain vocabulary), synonym swap, intent shift (negation/qualifier). Bounded by ≤5 words changed, ≤20 iterations, ≤150ms budget. Guided by Shapley values: focus perturbations on features with most negative contribution to the target skill's score.

**Performance budget (all Rust, no LLM):**

| Step | Time | Mechanism |
|------|------|-----------|
| Exact Shapley (16 evals) | <1ms | Enumeration over 4 features |
| Rank delta computation | <0.1ms | Vector comparison |
| Greedy beam search | <150ms | ≤20 iterations, bounded edits |
| Replay verification | <5ms | Snapshot lookup + assertion |
| **Total** | **<200ms** | Within SLO |

**Deterministic twin implementation:**

```rust
pub struct DeterministicTwin {
    frozen_now: i64,           // fixed timestamp
    rng: ChaCha8Rng,           // seeded, reproducible
    embeddings: Arc<EmbeddingStore>,  // fixed version, read-only
}
```

Seed everything: clock value, RNG state, embedding vectors, scoring config. After each graph rebuild, replay last N sessions through twin and assert bit-for-bit match on ranking output. Any divergence = non-determinism bug.

**Canary testing for retrieval quality:**
Maintain 50-100 fixed "canary queries" with known expected top-3 skill rankings. After each graph version bump, run all canaries and compare. Any rank deviation >1 position for expected skills → alert.

**Crate structure verification:**
The `explainability` crate must NOT import `sqlx` or `qdrant-client`. It receives `Vec<ScoredSkill>` as input — pure computation. Verifiable via `cargo tree -p explainability --depth 1`.

##### Slice 4.2: Drift sentinel

_(Slice definition unchanged)_

##### Slice 4.3: End-to-end causal tracing

_(Slice definition unchanged)_

##### Slice 4.4: Time-travel memory replay and offline deterministic twin

_(Slice definition unchanged)_

---

#### Phase 5: Multi-Harness & Ecosystem

_(Phase 5 Purpose and Rationale — unchanged)_

### Research Insights: Multi-Harness Compilation

**Serves:** SC-V2-7

**AgentSkills Convergence Impact:**

As of May 2026, Claude Code, OpenCode, and GitHub Copilot all converge on the `agentskills.io` open standard. This simplifies Slice 5.1 dramatically:

1. **`TemplateOnlyCompiler` (existing):** Claude Code output — `additionalContext` markdown via MCP hook. Full AgentSkills frontmatter.

2. **`OpenCodeCompiler`:** Same as Claude Code minus `context: fork`, `allowed-tools`, `paths`, `model`, `effort`, `hooks`, shell injection blocks. OpenCode ignores unknown frontmatter — stripping is optional but recommended for cleanliness.

3. **`CopilotCompiler`:** Same as Claude Code minus shell injection, fork context. Keep `allowed-tools` (Copilot supports it). Keep `argument-hint` (Copilot-specific).

4. **`CodexCompiler`:** DEFERRED. Codex uses `AGENTS.md` (plain markdown, no frontmatter). Different format entirely. Build when Codex users exist.

**Format roundtrip verification:**
Measure by subunit count preservation, not byte-identical output. Semantic equivalence:
```
Input: Vec<ScoredSkill> + prompt
Compile for Claude → parse back → extract name/description/subunit counts
Recompile for OpenCode → parse back → same counts
Recompile for Copilot → parse back → same counts
```

Assert: procedure count, convention count, asset count equal across all transformations.

##### Slice 5.1: Multi-harness compiler implementations

_(Slice definition unchanged — simplified by AgentSkills convergence)_

### Research Insights: Compiler Implementation

**Serves:** SC-V2-7

**Compiler effort adjustment:**

| Compiler | Original estimate | Adjusted estimate | Reason |
|----------|------------------|-------------------|--------|
| `OpenCodeCompiler` | Full implementation | **Frontmatter stripper (~50 lines)** | Agentskills.io convergence |
| `CopilotCompiler` | Full implementation | **Frontmatter adapter (~80 lines)** | Agentskills.io convergence |
| `CodexCompiler` | Full implementation | **Deferred to V3** | AGENTS.md format; no Codex users |

**Compilation output formats per harness:**

| Harness | Injection method | Compiler output |
|---------|-----------------|-----------------|
| Claude Code | MCP `UserPromptSubmit` hook → `additionalContext` | Markdown with frontmatter + shell blocks |
| OpenCode | Native `skill` tool → `<available_skills>` XML | XML block with name+description |
| Copilot | Agent context injection | SKILL.md body + allowed-tools |
| Codex | AGENTS.md concatenation | Plain markdown section (deferred) |

##### Slice 5.2: Extraction provider parity verification

_(Slice definition unchanged)_

### Research Insights: Provider Parity

**Serves:** SC-V2-7, DS-012

**Behavioral parity — beyond schema parity:**

Schema parity (identical JSON keys/types) is necessary but insufficient. Two extractions can be schema-identical yet produce skills with different downstream utility. Add behavioral parity test:

"A held-out fixture transcript set, skills extracted by different providers, when consumed by the SAME target model, must produce utility scores within ±2pp of each other on a behavioral evaluation."

This maps to SkillLens finding: same skill text on different consumers yields different utility (Appendix F, Table 10). But same transcript should yield similarly-useful skills when consumed by the same model.

**Cross-pair risk detection:**

| Provider Pair | Risk | Test |
|---------------|------|------|
| Claude → Claude (consume) | LOW | Self-extraction always positive |
| Ollama → Ollama (consume) | LOW | Self-extraction always positive |
| Claude → Ollama (consume) | HIGH | Behavioral: must be within ±2pp |
| Ollama → Claude (consume) | HIGH | Behavioral: must be within ±2pp |

##### Slice 5.3: Cross-harness portability verification

_(Slice definition unchanged)_

### Research Insights: Cross-Harness Portability

**Serves:** SC-V2-7

**Subunit preservation (the real test):**
Format roundtrip must preserve all subunits: procedure count, convention count, asset count equal across harness transformations. Content may differ in wrapping but substance must survive.

**Cross-harness canary test (NEW — critical addition):**
Add to Slice 5.3: One fixture skill, compiled for all 4 harnesses, injected into each harness's context, behavioral outcome measured on a fixture task. The test asserts: skill improves task completion rate by ≥ baseline in its source harness, and does not regress in other harnesses by >5%.

This is the minimum viable cross-harness validation — without it, we assume format portability equals utility portability, which SkillLens/SkillOpt prove is false.

**What transfers well vs poorly:**

| Skill feature | Cross-harness transfer |
|---------------|----------------------|
| Procedural rules (tool policies, verification steps) | Survives well |
| Output format constraints | Survives well |
| Code snippets and examples | Survives well |
| Harness-specific tool permissions | Does not apply (stripped) |
| Dynamic shell injection | Does not apply (stripped) |
| Harness-specific file paths | Degrades (harness-specific) |
| Model-specific prompting tricks | Degrades or harmful |

---

## Cross-Cutting: Extraction Source Harness Tracking

### Research Insight: Metadata Over Architecture

**Serves:** SC-V2-1, SC-V2-7, cross-harness quality analysis

Rather than building per-harness extraction pipelines (over-engineering before data exists), add ONE metadata field to every extracted skill:

```yaml
---
name: deploy-staging
description: Deploy to staging environment
extraction_source_harness: claude_code
extraction_source_model: claude-sonnet-4-20250514
quality_scores:
  failure_mechanism_score: 0.72
  actionable_specificity_score: 0.65
  high_risk_avoidance_score: 0.81
  combined_utility_score: 0.73
---
```

This single field enables:
- Cross-harness utility correlation analysis
- Per-harness quality score distribution monitoring
- "Did skills extracted by Claude help OpenCode users?" — answerable with data
- Future per-harness extraction prompt tuning (if data warrants)

Effort: ~60 minutes (parse one extra field from extraction output, store in `.pending` frontmatter, write to PG). Zero architecture changes. Zero new code paths.

---

### Slice-to-Story Traceability

_(Unchanged from original plan)_

---

## Acceptance Criteria

_(Unchanged from original plan)_

### Research Insights: Acceptance Criteria Additions

**Additional acceptance criteria from research:**

- [ ] Cross-harness canary test verifies skills compiled for different harnesses produce equivalent behavioral outcomes (±5% tolerance) on fixture tasks
- [ ] Every extracted skill carries `extraction_source_harness` and `extraction_source_model` metadata in `.pending` frontmatter and PG
- [ ] SkillOpt optimizer uses explicitly stronger model than extraction provider (config-enforced, not defaulted)
- [ ] SkillOpt cross-consumer validation gate tests against ALL consumer models before accepting edits
- [ ] Quality rubric excludes plausibility-based dimensions (clarity, conciseness, formatting)
- [ ] Map-reduce uses G=5 merge group for coding transcripts; G=10 kept as configurable default
- [ ] Self-healing catalog: all 7 remediations idempotent; graph_version_mismatch escalates to human

---

## Non-Functional Requirements

_(Unchanged from original plan)_

---

## Quality Gates

_(Unchanged from original plan)_

---

## Success Metrics

_(Unchanged from original plan)_

### Research Insights: Updated Metric Targets

| Metric | Original Target | Research-Adjusted Target | Reason |
|--------|----------------|--------------------------|--------|
| Optimization epoch convergence | >50% | >30% (realistic with local Ollama) | 56-74% of paper ceiling |
| Counterfactual explanation accuracy | ±0.05 of score | ±0.05 (keep — exact Shapley guarantees this) | Deterministic computation |
| Degraded-state recovery time | <30s | <30s (keep) | Bounded retries 3× (1s+2s+4s) = 21s max |

---

## Risk Analysis & Mitigation

_(Unchanged from original plan)_

### Research Insights: Additional Risks

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Cross-model skill transfer causes negative transfer | High | Medium | Self-extraction is safe. Cross-model pairs require behavioral validation. `extraction_source_harness` metadata enables post-hoc analysis. |
| Local Ollama optimizer produces worse skills | High | Medium | Validation gate rejects worse candidates. Optimizer model config enforces minimum strength. Human approval required (constitution §3). |
| Map-reduce overgeneralizes in coding domain | Medium | Medium | G=5 group size. Rare-pattern preservation. Single-pass fallback. |
| Quality rubric over-claims coding domain gains | Low | Medium | Expected +0.5-1.0pp on coding (vs +1.55pp paper). Don't market +1.55pp. |
| Cross-harness compilation assumes format = portability | High | Low | Cross-harness canary test added. Behavioral validation not just format roundtrip. |
| AgentSkills standard diverges between harnesses | Medium | Low | 3-way convergence as of May 2026. Monitor agentskills.io changelog. |

---

## Dependencies & Prerequisites

_(Unchanged from original plan)_

### Research Insights: Updated Dependencies

**SkillOpt data floor:** Paper shows 1 training example + 4 epochs = +47.5 on SpreadsheetBench (61% of ceiling). Even thin data produces gains. The gate protects against noise. Revise "skills without usage history cannot be optimized" to "skills with ≥5 usage samples can be optimized." Lower barrier.

**Optimizer model requirement:** Must be explicitly stronger than extraction provider. Two options:
1. Remote frontier model (GPT-5.5/Claude) as optimizer — even if extraction uses local Ollama
2. Strongest available local model (14B+) with acceptance of degraded optimization quality (56-74% of ceiling)

**AgentSkills convergence:** No new dependencies. The standard is already what we use for Claude Code. OpenCode and Copilot adoption means multi-harness compilation is mostly stripping fields, not building new formats.

---

## Alternative Approaches Considered

_(Unchanged from original plan)_

### Research Insights: Additional Alternatives

6. **Per-harness extraction pipelines:** Rejected for V2. SkillLens shows extractor-consumer asymmetry, but we have zero cross-harness utility data. Adding N extraction pipelines before measuring what one pipeline delivers is architecture without evidence. Revisit in V3 after cross-harness canary data exists.

7. **Per-harness SkillOpt optimizer models:** Rejected for V2. One optimizer + per-harness validation gates (N gates) is sufficient. Running N_harnesses × 4_epochs × K_skills is N× compute for unproven gain. Wait for systematic harness-specific regression data.

8. **Automatic cross-harness skill promotion:** Rejected permanently. Constitution §3 (human gate) + SkillLens negative transfer risk (+5.7pp harness spread in SkillOpt) = auto-promotion is premature and dangerous.

9. **Plausibility-based quality dimensions:** Rejected permanently. SkillLens proves plausibility rubric HURTS extraction (-0.59pp). Including "clarity," "conciseness," or "completeness" dimensions would actively degrade skill quality.

10. **Codex AGENTS.md compiler:** Rejected for V2. Different format entirely. No Codex users currently. Build when demand exists.

---

## References & Research

_(Unchanged from original plan — paper references at top of document)_

### Deepening Research Sources

- SkillLens (arXiv:2605.23899): Full paper analysis — cross-extractor/consumer pairs, SWE-bench-specific utility data, map-reduce failure modes, rubric dimension validation, LLM judge accuracy
- SkillOpt (arXiv:2605.23904): Full paper analysis — harness-dependent gains, batch size sweeps, edit budget diminishing returns, optimizer model strength impact, transfer experiments
- AgentSkills.io: Claude Code + OpenCode + Copilot compatibility verified May 2026
- Rust crate ecosystem (2026): `failsafe` for circuit breakers, `fred` for Redis streams, `sqlx` for async PG, `rand_chacha` for deterministic RNG, `bincode`/`postcard` for snapshot serialization
