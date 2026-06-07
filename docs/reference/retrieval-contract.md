# Retrieval Contract

This document describes how skill retrieval **actually works today**. Every claim is grounded in the
real code and cites the exact file and line at time of writing (branch `feat/v-1-5-1`). No aspirational
claims are stated as current behavior. Known gaps are enumerated at the end.

---

## 1. How every lifecycle event triggers retrieval

Every hook-driven lifecycle event (SessionStart, UserPromptSubmit, PreCompact) calls the same
`compile_context` MCP tool with a prompt string and runs the same query-driven cosine ranking path.
There is **no special SessionStart behavior** — all hooks are structurally identical.

### Hook wiring

`config/claude-code/hooks.example.json` (lines 9–73) registers three hooks:

| Event | Tool | Prompt sent |
|---|---|---|
| `SessionStart` | `compile_context` | `{{initial_prompt}}` |
| `UserPromptSubmit` | `compile_context` | `{{prompt}}` |
| `PreCompact` | `compile_context` | `{{summary}}` with `trigger: "compact"` |

All three call `compile_context` with a prompt, `session_id`, and `repo_path`. The only behavioral
difference is `trigger: "compact"` on PreCompact, which bypasses session-level duplicate suppression
for that single call. The retrieval path itself is identical across all three.

### compile_context tool

Defined in `crates/mcp-server/src/protocol.rs` (lines 43–60). The tool:

1. Accepts `prompt`, `session_id`, `repo_path`, and an optional `trigger`.
2. Checks session suppression (deduplication). A `compact` trigger bypasses this check.
3. Delegates to the `SkillRetriever::retrieve(prompt, repo_path)` async trait method.
4. Compiles the ranked results into injected context via `TemplateOnlyCompiler`.

See `crates/mcp-server/src/tools/compile_context.rs` for the full tool implementation.

**At SessionStart the prompt is typically thin or absent** (`{{initial_prompt}}` may be empty or a
short user sentence), so the cosine similarity has little to anchor on. This is a known limitation
— see section 6.

---

## 2. Scoring formula

Every candidate skill in the corpus is scored by the SkillRAE eq.3 formula:

```
score = (α·l1_cos + β·subunit_evidence + γ·prior) · (1 + λ·community_boost)
```

### Default weights

Defined in `crates/retrieval/src/scoring.rs` (lines 21–30), `ScoringWeights::default()`:

| Symbol | Weight | Term |
|---|---|---|
| α | 0.45 | Skill-level L1 cosine similarity (query embedding vs skill embedding) |
| β | 0.35 | Semantic subunit evidence — mean cosine of the skill's subunits to the query |
| γ | 0.20 | Usage prior (see section 3) |
| λ | 0.25 | Community boost multiplier |

`score_eq3` at `crates/retrieval/src/scoring.rs` (lines 32–38):
```rust
let base = weights.alpha * components.l1_semantic
    + weights.beta * components.subunit_evidence
    + weights.gamma * components.prior;
base * (1.0 + weights.lambda * components.community_boost)
```

### Subunit evidence (β term)

The β term is **semantic** subunit evidence (mean cosine of the top-k subunit embeddings against the
query), not keyword overlap. Lexical overlap is tracked separately and used only for rationale
display. See `crates/retrieval/src/graph_search.rs` lines 28–31 and the `perform_scope_search`
function in `crates/retrieval/src/dual_scope.rs` (lines 280–292).

---

## 3. Usage prior and the cold-start guarantee

### Formula

`crates/retrieval/src/scoring.rs` (lines 56–75), function `usage_prior`:

```
prior = min(ln(1 + usage_count) · e^(−age_days / 30), 0.15)
```

- **30-day time constant** ≈ 21-day half-life: recent usage weights heavily.
- **Cap at 0.15**: `prior ≤ 0.15` → after the γ=0.20 weight, the prior contributes at most
  `0.20 × 0.15 = 0.03` of the total score. This makes recency a mild tiebreaker,
  not a dominant driver.
- **`usage_count == 0` → `prior = 0.0`**: a freshly-approved skill gets no usage boost and must
  compete on cosine and subunit evidence alone.

### Cold-start guarantee

A newly-approved skill (usage_count=0, prior=0) that is **relevant to the query** (high cosine)
ranks above an irrelevant skill with high usage (large prior) because the cosine term (α=0.45) and
subunit evidence (β=0.35) dominate the score and cannot be overridden by the ≤0.03 prior ceiling.

This is the design intent of the cap. The regression test
`relevant_zero_usage_skill_outranks_irrelevant_high_usage_skill` in
`crates/retrieval/src/scoring.rs` locks this property.

---

## 4. No-match relevance floor

Defined in `crates/retrieval/src/orchestrator.rs` (lines 196–197):

```rust
relevance_threshold: 0.450,
```

Candidates whose eq.3 score falls below 0.450 are excluded before fusion and never returned.

### Calibration evidence

The 0.450 floor was calibrated from live per-query-tagged measurements on the isolated 8-skill
quality corpus (2026-06-07, 772 events). The gap between the worst negative score (0.4386 for
"kubernetes TLS termination") and the lowest true-positive disjoint hit (0.4565 for
"git-rebase-conflict-resolution") is 0.0179 wide. The floor sits 0.0064 above the max negative
and 0.0115 below the min positive. See the full evidence table in the `RetrievalConfig::default()`
comment in `crates/retrieval/src/orchestrator.rs` (lines 158–196).

The floor can be overridden at runtime without redeployment via `RETRIEVAL_RELEVANCE_THRESHOLD`.
See `RetrievalConfig::relevance_threshold_from_env` at `crates/retrieval/src/orchestrator.rs`
(lines 207–227).

---

## 5. Dual-scope search and weighted RRF fusion

### Concurrent scope search

`crates/retrieval/src/dual_scope.rs` (lines 43–105), function `search_scopes_concurrently`:

- Project scope and global scope are searched **concurrently** (via `tokio::join!` for two scopes,
  `tokio::spawn` for three or more).
- Each scope is filtered by `scope_id` and `source_paths` before cosine ranking: only skills whose
  `scope_id` matches and whose `source_paths` are under the scope root are eligible.
- Each scope runs independently behind a per-scope timeout (default 400 ms,
  `RetrievalConfig::scope_timeout_ms`).

### Scope weights

`crates/retrieval/src/orchestrator.rs` (lines 472–478), `scope_weight`:

| Scope type | Weight |
|---|---|
| Project | 1.0 (`project_scope_weight`) |
| Global | 0.7 (`global_scope_weight`) |
| Team | 0.7 (same as global) |

### Weighted Reciprocal Rank Fusion (RRF)

`crates/retrieval/src/fusion.rs` (lines 83–105), function `weighted_reciprocal_rank_fusion`:

Candidates from all scopes are merged via weighted RRF. For a candidate at rank `r` in a scope
with weight `w`, the RRF contribution is:

```
contribution = w / (k + r)   where k = 60.0 (rrf_k default)
```

The same skill appearing in multiple scopes accumulates contributions from each. After fusion,
results are sorted by descending fused score with project-scope candidates winning ties
(see `fused_candidate_order` and `scope_priority` in `crates/retrieval/src/fusion.rs`
lines 152–173).

---

## 6. Mid-session `find_skill` — the agent's task-time retrieval path

`find_skill` is a real, exposed MCP tool available to agents at any point during a session.

### Tool descriptor

`crates/mcp-server/src/protocol.rs` (lines 61–76):

```json
{
  "name": "find_skill",
  "description": "Find top matching skills from the retrieval graph",
  "required_arguments": ["prompt"],
  "properties": {
    "prompt": "Natural-language query to match against the skill graph",
    "limit": "Maximum number of skills to return (default 5)"
  }
}
```

### Semantics

An agent calls `find_skill(prompt, limit)` when it recognizes mid-task that it needs additional
skill context — for example, after receiving a user prompt that touches a domain not surfaced at
session start. The tool runs the same `SkillRetriever::retrieve` path as `compile_context` but
without session suppression or duplicate filtering, and returns raw ranked skill matches. This is
the **high-signal, on-demand retrieval path** and the sharpest tool available to an agent during
a task.

---

## 7. Known limitations / future work

### #220 — priming ranker and recurrence-based global (corpus-dependent, not yet built)

- **No priming mode today.** SessionStart runs the identical query-driven cosine path as
  UserPromptSubmit. A thin or absent `{{initial_prompt}}` means cosine has little signal, so
  session start frequently returns no match or returns whatever has the highest prior from past
  usage. A deliberate priming ranker (centrality + recency + freshness slot) does not exist yet.
  This is tracked in **#220** and requires quality measurement on the **#216 corpus** before shipping.

- **Global scope is cosine × 0.7, not recurrence-based.** The flat 0.7 down-weight is a
  principled but simplified heuristic. A signal based on cross-project recurrence (how often a
  skill recurs across multiple distinct projects — the data #180 already computes) would better
  capture "globally appropriate" intent. Not yet implemented. Tracked in **#220**.

### #209 — floor calibrated on a small corpus

The no-match relevance floor of 0.450 was calibrated on an 8-skill isolated corpus. The
calibration may shift on a larger or more diverse corpus. Tracked in **#209**.

### Capped prior means cold-start is mild, not fatal

A relevant new skill (usage_count=0) competes purely on cosine and subunit evidence. Because
α=0.45 and β=0.35 together account for up to 80% of the base score (before the ≤0.03 prior
ceiling), a relevant new skill surfaces fine on a relevant query. It may rank ≤0.03 score points
below an equally-relevant established peer — a mild tiebreaker, not exclusion. The regression
test in `scoring.rs` locks this guarantee.
