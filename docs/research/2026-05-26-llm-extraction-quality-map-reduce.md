# LLM Extraction Quality & Map-Reduce Research

**Date:** 2026-05-26
**Scope:** Research for skill extraction quality scoring, map-reduce patterns, tool-calling, embedding abstraction, and local LLM compilation
**Decision:** Architecture recommendations for V2 skill extraction pipeline

---

## 1. Quality Rubrics for LLM Output

### What Makes a Quality Dimension Predict Downstream Utility?

Validated frameworks converge on 4-6 dimensions. For skill extraction, the SkillLens paper (arXiv:2605.23899) validates 3 dimensions that predict utility with 64-66% accuracy on coding domain data:

| Dimension | Definition | Why It Predicts Utility |
|---|---|---|
| **Failure Mechanism Encoding** | Names concrete failure modes with executable remedies | Generic skills ("use git") are useless; skills that encode what-can-go-wrong-and-how-to-fix are reusable |
| **Actionable Specificity** | A developer can act without additional context | Skills requiring external context add cognitive load; self-contained skills reduce it |
| **High-Risk Action Blacklist** | Explicitly warns against dangerous operations | Safety: skills that encode guardrails prevent bug propagation |

Additional validated dimensions from the literature (use selectively):

| Dimension | Source | Use for Skill Extraction? |
|---|---|---|
| Faithfulness | Ragas, DeepEval, G-Eval | **Yes** — skill must stay true to source transcript |
| Correctness | Ragas, DeepEval | **Yes** — factual accuracy of procedures |
| Conciseness | Ragas | **Yes** — respects max_skill_chars budget |
| Completeness | Check-Eval | **Conditional** — only for final SKILL.md, not per-fragment |

### Scoring Formula: Hybrid Gate + Weighted Sum

Recommended for Rust implementation — combines safety (gates) with ranking (weighted):

```
SC-V2-1 Formula (proposed):
  gate_fme >= 0.6 AND gate_actionable >= 0.6 → continue scoring, else REJECT
  overall = 0.30*FME + 0.25*actionable + 0.20*correctness + 0.15*conciseness + 0.10*blacklist
  ACCEPT if overall >= 0.65
```

Rationale: FME + actionable specificity carry most weight because they directly predict downstream reuse. Blacklist is a gate (missing = fail-fast) but contributes less to ranking.

### Implementation Sketch (insert into `crates/domain/src/types.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityScores {
    pub failure_mechanism_encoding: f32,  // 0.0-1.0
    pub actionable_specificity: f32,
    pub correctness: f32,
    pub conciseness: f32,
    pub high_risk_blacklist: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QualityDecision {
    Accepted { overall: f32, scores: QualityScores },
    Rejected { reason: String, failed_dimension: String },
}

pub fn evaluate_quality(scores: &QualityScores, config: &QualityConfig) -> QualityDecision {
    if scores.failure_mechanism_encoding < config.gate_fme {
        return QualityDecision::Rejected {
            reason: "Failure mechanism encoding below threshold".into(),
            failed_dimension: "failure_mechanism_encoding".into(),
        };
    }
    if scores.actionable_specificity < config.gate_actionable {
        return QualityDecision::Rejected {
            reason: "Not sufficiently actionable".into(),
            failed_dimension: "actionable_specificity".into(),
        };
    }
    let overall = 0.30 * scores.failure_mechanism_encoding
        + 0.25 * scores.actionable_specificity
        + 0.20 * scores.correctness
        + 0.15 * scores.conciseness
        + 0.10 * scores.high_risk_blacklist;
    if overall >= 0.65 {
        QualityDecision::Accepted { overall, scores: scores.clone() }
    } else {
        QualityDecision::Rejected {
            reason: format!("Overall score {} below threshold 0.65", overall),
            failed_dimension: "overall".into(),
        }
    }
}
```

### Prompt Template for Rubric-Based Scoring

```
You are a skill quality evaluator. Score this skill candidate on 3 dimensions.

SCORING RUBRIC:

1. FAILURE MECHANISM ENCODING (0-5):
   - 0-1: Generic advice, no failure modes mentioned
   - 2-3: Mentions some failure modes but no remedies
   - 4-5: Names concrete failure modes WITH executable remedies

2. ACTIONABLE SPECIFICITY (0-5):
   - 0-1: Vagueness, requires significant external context
   - 2-3: Somewhat specific but gaps remain
   - 4-5: A developer can immediately act on this skill

3. HIGH-RISK ACTION BLACKLIST (0-5):
   - 0-1: No warnings about dangerous operations
   - 2-3: Implicit cautions
   - 4-5: Explicitly warns against specific dangerous operations

Skill to evaluate:
{skill_content}

Output ONLY valid JSON:
{
  "failure_mechanism_encoding": <0-5>,
  "actionable_specificity": <0-5>,
  "high_risk_blacklist": <0-5>,
  "reasoning": "<1-sentence per dimension>"
}
```

---

## 2. Self-Assessment Reliability

### Reliability Assessment

LLM self-assessment is **moderately reliable but biased**. Key findings:

| Bias | Severity | Mitigation |
|---|---|---|
| **Position bias** | High (2:1 preference for first option in pairwise) | Use direct assessment (absolute scoring), not pairwise |
| **Verbosity bias** | High (longer = higher score) | Include conciseness as explicit dimension |
| **Self-enhancement bias** | Medium (own outputs scored higher) | Use separate evaluator model when possible |
| **Calibration** | Varies by model (Claude better calibrated than Ollama) | Multi-model agreement |

### Recommended Architecture: Separate Lightweight Evaluator

**Do not self-assess.** Use a dedicated evaluator model:

```
┌─────────────────────────────────────────────────────────┐
│  Claude (primary extractor)                             │
│    ↓                                                    │
│  Ollama llama3.2:3b (evaluator)  ← separate model      │
│    ↓                                                    │
│  Quality decision                                       │
└─────────────────────────────────────────────────────────┘
```

Why:
1. Claude extracts skills (high quality, structured output via `strict: true`)
2. Ollama llama3.2:3b scores them (cost-effective, <500ms per skill)
3. No self-enhancement bias — evaluator model != extractor model
4. Matches SLMEval findings (Daynauth et al., 2025): 5-30x cost reduction over GPT-4 evaluators

### Scoring Placement in Map-Reduce Pipeline

```
Map phase (per-chunk extraction) → NO scoring (expensive, noisy at fragment level)
Reduce phase (post-merge)        → Quality scoring on merged skills
Final synthesis (SKILL.md)       → Final validation scoring
```

Rationale: Per-fragment scoring is noisy because fragments are incomplete. Score after merge when skills are coherent.

---

## 3. Map-Reduce Patterns for Transcript Extraction

### Recommended Topology: Tree-Based Merge (Bottom-Up)

```
Level 0: [trajectory_1, trajectory_2, ..., trajectory_N]   ← N chunks
Level 1: [merged_1_2, merged_3_4, ..., merged_N-1_N]      ← N/2 summaries (parallel)
Level 2: [merged_1_4, ..., merged_N-3_N]                  ← N/4 summaries (parallel)
...
Level log(N): [final_merged_skills]                        ← 1 result
```

**Tree-based > Sequential** because:
- Information loss distributed across log(N) levels, not N sequential steps
- Level-by-level parallelism (all level-1 merges run concurrently)
- Deduplication forced at each level (similar skills merged early)

### Chunking Strategy for Coding Transcripts

Split on **tool-call boundaries**, not fixed token counts:

```
Chunk boundaries:
  ├── Trajectory start/end markers
  ├── Tool invocation boundaries (system/tool_call)
  └── Task completion markers

Chunk size: 15K-20K tokens (Claude extraction window)
Overlap: 2K-3K tokens (include last 1-2 tool calls from previous chunk)
```

Implementation approach: Extend `crates/graph-builder/src/extraction/mod.rs` to accept chunked transcript input alongside the current direct-markdown path.

### Map Phase Prompt (Per-Chunk Extraction)

```
You are a skill extraction system. Extract ALL procedural skills from this coding transcript segment.

For each skill, extract:
- name: imperative verb phrase (e.g., "reproduce-bug-from-logs")
- procedure: numbered steps
- tools_used: tool names observed
- failure_modes: what can go wrong and how to fix
- confidence: 0.0-1.0

Output as JSON array under key "candidates".

Transcript segment:
{chunk_text}
```

### Reduce Phase Prompt (Merge + Deduplicate)

```
You are merging skill extractions from multiple transcript segments.
Remove duplicates, resolve conflicts, enrich descriptions.

MERGE RULES:
1. Skills with >70% procedure overlap → merge (keep longer procedure)
2. Conflicting guidance → keep both with resolution note
3. Similar skills → combine into more general skill
4. Preserve unique tool sequences from each source
5. Never silently drop a safety warning (high-risk action)

Input: Array of skill arrays from {N} chunks
Output: Single deduplicated skill array

Input:
{merged_candidates_json}
```

### Information Preservation Techniques

- **Never summarize below 30% of original token count** in a single reduce step
- **Preserve structured fields verbatim** (tool names, file paths, error messages) — these are anchors
- **Confidence scoring per skill**: propagate min/max/avg confidence through merge levels
- **Conflict log**: emit `conflicts_resolved` array alongside merged skills for auditability

### Multi-Model Pipeline Viability: YES

| Stage | Model | Budget |
|---|---|---|
| Map (per-chunk extraction) | Claude API (strict tool-calling) | Per-chunk latency, parallelizable |
| Reduce (merge + deduplicate) | Ollama llama3.2:3b | <3s per merge level |
| Quality scoring | Ollama llama3.2:3b (evaluator prompt) | <500ms per skill |
| Final SKILL.md synthesis | Ollama llama3.2:3b (synthesis prompt) | <3s |

Budget enforcement: Claude extraction uses `max_tokens`; Ollama reduce uses `num_predict: 512`.

---

## 4. Tool-Calling for Structured Extraction

### Recommended Approach: Claude `strict: true` Tool-Calling + Ollama `format: "json"` Fallback

Claude's `strict: true` guarantees schema conformance via grammar-constrained sampling. For Ollama, use `format: "json"` with schema in the prompt (no `tool_choice` support in current Ollama).

### Extraction Tool Schema

```json
{
  "name": "extract_skills",
  "description": "Extract reusable skill candidates from a coding session transcript",
  "strict": true,
  "input_schema": {
    "type": "object",
    "properties": {
      "candidates": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "name": {"type": "string", "maxLength": 64},
            "description": {"type": "string", "maxLength": 256},
            "procedures": {"type": "array", "items": {"type": "string"}},
            "conventions": {"type": "array", "items": {"type": "string"}},
            "assets": {"type": "array", "items": {"type": "string"}},
            "failure_modes": {"type": "array", "items": {"type": "string"}},
            "high_risk_actions": {"type": "array", "items": {"type": "string"}},
            "confidence": {"type": "number", "minimum": 0, "maximum": 1}
          },
          "required": ["name", "description"],
          "additionalProperties": false
        },
        "maxItems": 10
      }
    },
    "additionalProperties": false
  }
}
```

### Schema Design Rules

1. `additionalProperties: false` — prevents hallucinated fields
2. `maxLength` on strings — prevents budget overflow
3. `maxItems: 10` — prevents extraction bombs
4. Required fields minimal (`name`, `description` only)
5. Flat structure (max 2 nesting levels) — higher LLM conformance

### Budget Enforcement

LLMs cannot reliably count output tokens. **Always post-process**:

```rust
fn enforce_budget(candidates: &mut Vec<SkillCandidate>, config: &ExtractionConfig) {
    candidates.truncate(config.max_skills);
    for candidate in candidates.iter_mut() {
        candidate.description.truncate(config.max_skill_chars);
        for procedure in candidate.procedures.iter_mut() {
            procedure.truncate(config.max_skill_chars);
        }
    }
}
```

### Integration Into Existing Code

Current code in `crates/infrastructure/src/extraction/claude.rs` uses raw HTTP to Anthropic API. Add tool-calling by:
1. Sending `tools` array in the request body alongside `system` and `messages`
2. Setting `tool_choice: {"type": "tool", "name": "extract_skills", "disable_parallel_tool_use": true}`
3. Parsing `content[].type == "tool_use"` blocks from the response
4. Fall back to current raw prompt extraction if tool-calling fails

---

## 5. Embedding Provider Abstraction

### Current State

- `EmbeddingService` trait in `crates/domain/src/traits.rs` — async, online
- `EmbeddingGenerator` trait in `crates/graph-builder/src/graph/embeddings.rs` — sync, offline, uses 8-dim deterministic hash (placeholder)
- `OllamaEmbeddingService` implements `EmbeddingService` via `/api/embeddings`

### Recommended Enhancements

**Extend `EmbeddingService` trait** with provider metadata:

```rust
#[derive(Debug, Clone)]
pub struct EmbeddingProviderInfo {
    pub name: &'static str,
    pub model: String,
    pub dimensions: usize,
}

#[async_trait]
pub trait EmbeddingService: Send + Sync {
    fn provider_info(&self) -> EmbeddingProviderInfo;
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
    async fn health_check(&self) -> Result<(), EmbeddingError> { Ok(()) }
}
```

**Dimension validation** (reject mismatches, not silently accept):

```rust
fn validate_dimensions(embedding: &[f32], expected: usize) -> Result<(), EmbeddingError> {
    if embedding.len() != expected {
        return Err(EmbeddingError::DimensionMismatch {
            expected,
            actual: embedding.len(),
        });
    }
    Ok(())
}
```

**Fallback chain** with circuit breakers (add to `crates/infrastructure/src/embeddings/`):

```
Priority chain:
  1. Ollama nomic-embed-text (primary)  → dimensions: 768
  2. Ollama mxbai-embed-large (fallback) → dimensions: 1024
  3. Zero-vector placeholder (last resort) → marks "unembedded"

Circuit breaker per provider: N failures → open circuit for reset_period
```

**Replacing `DeterministicEmbeddingGenerator`**: The current 8-dim hash placeholder in `crates/graph-builder/src/graph/embeddings.rs` must be replaced with real Ollama embeddings. This requires making the graph builder async-compatible or running embedding in a blocking runtime context (tokio `spawn_blocking`).

### OpenTelemetry Integration

Add `opentelemetry` and `opentelemetry_sdk` crates. Key spans:

| Span Name | Attributes |
|---|---|
| `embedding.generate` | provider, model, dimensions |
| `embedding.batch` | provider, batch_size |
| `embedding.fallback` | failed_provider, fallback_provider |

Histogram metrics: `embedding.latency_ms` with boundaries [1, 5, 10, 25, 50, 100, 250, 500, 1000].

### Docker Compose Updates

```yaml
ollama:
  image: ollama/ollama:0.24.0
  healthcheck:
    test: ["CMD", "curl", "-f", "http://localhost:11434/api/tags"]
    interval: 30s
    timeout: 10s
    retries: 3
    start_period: 60s
  environment:
    OLLAMA_NUM_PARALLEL: 2
    OLLAMA_KEEP_ALIVE: 5m
  deploy:
    resources:
      reservations:
        devices:
          - driver: nvidia
            count: all
            capabilities: [gpu]
```

---

## 6. Local LLM Compilation (Ollama Synthesis)

### Recommended Model: `llama3.2:3b` Q4_K_M

- 3B parameters, ~2-3s latency on modern GPU
- Strong instruction-following for merging/synthesis tasks
- Good at respecting structural constraints (SKILL.md format)

Alternative: `qwen2.5:3b` Q4_K_M — slightly better at code-domain synthesis, comparable latency.

### Ollama Configuration for <3s Budget

```json
{
  "model": "llama3.2:3b",
  "options": {
    "num_ctx": 2048,
    "num_predict": 512,
    "temperature": 0.3,
    "top_p": 0.9,
    "repeat_penalty": 1.1
  },
  "keep_alive": 300
}
```

Key: `num_ctx: 2048` and `num_predict: 512` are the latency knobs. Each doubling of context roughly doubles inference time.

### Synthesis Prompt (NOT Concatenation)

```
You are a skill synthesis expert. Merge multiple skill fragments into ONE coherent SKILL.md.

FRAGMENTS:
{fragments}

CRITICAL: SYNTHESIZE, do not concatenate.

CONCATENATION (BAD):
  - Fragment 1 says: "use git checkout -b"
  - Fragment 2 says: "use git switch -c"
  (just listing both without resolution)

SYNTHESIS (GOOD):
  - Branch creation: `git switch -c` (preferred, Git 2.23+), fallback: `git checkout -b`
  - Rationale: Fragments A and B agree on branch naming, disagree on command

RULES:
1. Extract common patterns across ALL fragments → these are canonical
2. Unique contributions → keep with scope qualifier
3. Conflicts → resolve explicitly (prefer more specific/recent guidance)
4. Output valid SKILL.md with frontmatter (name, description, tags)
5. Preserve ALL Procedure, Convention, and Asset subunits
6. NEVER silently drop a safety warning

OUTPUT:
---
name: {synthesized-name}
description: {one-line description}
tags: [{tag-list}]
---

# Procedures
{merged procedures}

# Conventions
{merged conventions}

# Assets
{merged assets}
```

### Routing Logic: Template vs Guidance Path

```rust
fn should_use_llm_synthesis(fragments: &[SkillFragment]) -> bool {
    // 1-2 fragments, no conflicts → template path (<500ms)
    if fragments.len() <= 2 && !has_conflicts(fragments) {
        return false;
    }
    // 3+ fragments OR conflicts → LLM path (<3s)
    if fragments.len() >= 3 || has_conflicts(fragments) {
        return true;
    }
    // Edge case: 2 fragments, >500 total tokens → LLM path
    fragments.iter().map(|f| f.content.len()).sum::<usize>() > 500
}
```

### Latency Budget Enforcement

```rust
const SYNTHESIS_TIMEOUT_MS: u64 = 2500; // 2.5s, 500ms buffer for template fallback

async fn synthesize_skills(fragments: Vec<SkillFragment>) -> CompiledSkill {
    match tokio::time::timeout(
        Duration::from_millis(SYNTHESIS_TIMEOUT_MS),
        ollama_synthesis(&fragments),
    ).await {
        Ok(Ok(skill)) => skill,
        _ => {
            tracing::warn!("LLM synthesis timeout, falling back to template compilation");
            template_compile(&fragments)
        }
    }
}
```

---

## Cross-Cutting Recommendations

### Architecture Integration Map

```
┌─────────────────────────────────────────────────────────────────────┐
│                      EXTRACTION PIPELINE (V2)                       │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  Transcript ──► Chunk by tool-call boundaries ──► Map Phase         │
│                                                     │               │
│                           ┌─────────────────────────┘               │
│                           ▼                                         │
│               Claude strict tool-calling (primary)                  │
│               Ollama format:json (fallback)                         │
│                           │                                         │
│                           ▼                                         │
│               ┌────── Reduce Phase (Tree Merge) ──────┐            │
│               │  Level 1: Ollama merge pairs          │            │
│               │  Level 2: Ollama merge again          │            │
│               │  ... log(N) levels                    │            │
│               └───────────────────────────────────────┘            │
│                           │                                         │
│                           ▼                                         │
│               Quality Scoring (Ollama evaluator)                    │
│               Gate: FME >= 0.6, Actionable >= 0.6                   │
│               Accept: overall >= 0.65                               │
│                           │                                         │
│                    ┌──────┴──────┐                                  │
│                    ▼              ▼                                  │
│              Template Path   Guidance Path                          │
│              (<500ms)        (<3s, Ollama synthesis)                │
│                    │              │                                  │
│                    └──────┬───────┘                                  │
│                           ▼                                         │
│               SKILL.md (human gate: .pending)                       │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Files to Create/Modify

| File | Change |
|---|---|
| `crates/domain/src/types.rs` | Add `QualityScores`, `QualityDecision`, `QualityConfig` |
| `crates/domain/src/errors.rs` | Add `DimensionMismatch` variant to `EmbeddingError` |
| `crates/domain/src/traits.rs` | Add `provider_info()`, `health_check()` to `EmbeddingService` |
| `crates/graph-builder/src/extraction/quality.rs` | **NEW** — quality scoring with Ollama evaluator |
| `crates/graph-builder/src/extraction/map_reduce.rs` | **NEW** — tree-based merge, chunking |
| `crates/graph-builder/src/extraction/mod.rs` | Wire map-reduce into `extract_skill` |
| `crates/infrastructure/src/extraction/claude.rs` | Add `strict: true` tool-calling |
| `crates/infrastructure/src/extraction/ollama.rs` | Add tool-calling fallback |
| `crates/infrastructure/src/embeddings/fallback_chain.rs` | **NEW** — circuit-breaker fallback |
| `crates/infrastructure/src/embeddings/mod.rs` | Export fallback chain |
| `crates/infrastructure/src/telemetry.rs` | **NEW** — OpenTelemetry setup |
| `crates/compiler/src/synthesis.rs` | **NEW** — Ollama SKILL.md synthesis |
| `crates/compiler/src/template.rs` | Add routing logic (template vs guidance path) |
| `docker-compose.yml` | Ollama healthcheck, GPU config |

### Constitution Compliance

All recommendations operate within constitution constraints:
- ✅ Local-first: all LLM calls to local Ollama or Claude API (no cloud-only dependencies)
- ✅ Human gate: quality-scored skills still produce `.pending` files
- ✅ Filesystem-observable: quality scores stored alongside skill files or in graph metadata
- ✅ <500ms template path: routing logic ensures template path stays fast
- ✅ <3s guidance path: Ollama synthesis with timeout + fallback

### Key References

1. **SkillLens** (arXiv:2605.23899) — 3 quality dimensions validated for coding skill extraction
2. **G-Eval** (Liu et al., 2024) — Chain-of-thought rubric scoring
3. **LLM-as-a-Judge** (Zheng et al., 2024) — Foundation for LLM evaluators, bias catalog
4. **SLMEval** (Daynauth et al., 2025) — Small-model evaluators, 5-30x cost reduction
5. **DeepEval** — DAGMetric for workflow gates, GEval with rubric constraints
6. **Ragas** — Faithfulness, correctness, relevancy metrics with NLI backing
7. **Anthropic Strict Tool Use** — Grammar-constrained sampling for 100% schema conformance
8. **LangChain MapReduceChain** / **LlamaIndex TreeSummarize** — Reference implementations for map-reduce patterns
