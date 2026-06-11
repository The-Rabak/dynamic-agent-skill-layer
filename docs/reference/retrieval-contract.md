# Retrieval Contract

This document describes how skill retrieval **actually works today**. Every claim is grounded in the
real code. No aspirational claims are stated as current behavior. Known gaps are enumerated at the end.

The grounded core below (sections 1–7) was written on branch `feat/v-1-5-1` and remains accurate for
the **default** path. The **V1.7 delta** (section 0) records what `feat/v-1-7` changed; where the two
differ, V1.7 wins. For the measured state, flags, and unmeasured gaps, see
`docs/assessments/2026-06-11-v1-7-retrieval-contract-measured.md`.

---

## 0. V1.7 delta (branch `feat/v-1-7`, 2026-06-11)

V1.7 is **additive** over the v1.5.1 core below. The default query path is unchanged in shape
(query → dual-scope cosine → weighted RRF → eq.3 floor), with these changes:

- **Embedder is now `qwen3-embedding:4b` (dim 2560), model-keyed collection `skills__qwen3-embedding-4b`**
  (T02; `infrastructure::ollama::DEFAULT_EMBEDDING_MODEL`). Dimension is discovered live.
- **Candidate generation is backend-selectable** via `RETRIEVAL_BACKEND` (`RetrievalBackend`,
  `crates/retrieval/src/orchestrator.rs:166`, fail-loud parse). **Default `snapshot_dense`** (the path
  documented in §5). `snapshot_hybrid` (in-memory dense + Okapi BM25 over the 9 multi-view fields) and
  `qdrant_hybrid` (live Qdrant dense+sparse read) are **experimental, opt-in**. T04 measured no uplift
  on the empty-multiview corpus; **T11 re-measured on the rich 262-corpus and the verdict held and
  hardened**: `snapshot_hybrid` (BM25 sparse candidate fusion) is **net-negative** (MRR 0.686→0.522,
  loses 23 golds from the candidate pool), and `qdrant_hybrid` is **byte-identical to `snapshot_dense`**
  (137/137 ties, p=1.0) — so dense stays the default and neither hybrid backend is promoted.
  `qdrant_hybrid` reads Qdrant at query time and thus breaks the default CQRS split — see
  `online-retrieval-cqrs.md`.
- **Dense multi-view views** (`e_task`/`e_needs`/`e_negative`, max-over-views α fusion) behind
  `RETRIEVAL_DENSE_VIEWS` (T09). Currently **default-OFF** (OFF == byte-for-byte pre-T09 ranking), but
  **T11 measured a validated uplift and RECOMMENDS promoting it to default-ON**: anchor-only MRR@3
  0.686→0.743, candidate-recall@50 0.723→0.796, nDCG@3 0.696→0.755 (sign p=0.0074); judge-aug held-out
  **0.912 / 0.839 / 0.92** (vs dense 0.884 / 0.804 / 0.92), p95 369ms < 500ms. The actual flag-default
  flip is a pending owner-approved change (behavior-changing default); see
  `tests/e2e/reports/t11/T11-VALIDATION-REPORT.md`.
- **No local reranker / query decomposition** — T07 was **skipped** (optional; T04 showed candidate
  generation is not the ceiling).
- **Typed skill graph edges** (`skill_edges`: depends_on / composes_with / similar_to / conflicts_with)
  are persisted (T05), exposed to agents but **not** used as a ranking multiplier.
- **Agent-facing score is now relevance, not the RRF artifact (#260).** `find_skill`/`search_skill_graph`
  expose `score` = eq.3 relevance (threaded `ScoredSkill.semantic_score`), `fusion_rank_score` = the RRF
  ordering value, plus per-match `rationale` and a `retrieval_context {embedding_model, collection,
  graph_version}` provenance block (#243). `search_skill_graph` additionally returns separate
  `neighbors` (positive edges incident on matches) and `conflicts` (`conflicts_with`, never folded into
  match scores) and `latency_ms`. `inspect_skill` exposes the 7 multi-view fields. `/health` carries
  `embedding_arm` and `retrieval_backend` components.
- **Community/graph multiplier (`λ·community_boost` in §2) is measured ranking-inert** — present in
  code but it does not change ranking; do not claim the graph improves retrieval ranking.
- **The 0.80 quality target is now VALIDATED on the qwen3 262-corpus (T11, 2026-06-11).** Using the
  corpus-aligned anti-circularity fixture (`retrieval_quality_262_corpus_labeled.json`, 137 positives,
  α=0 instrument gate passed at 100% crater), judge-augmented **held-out** retrieval meets the frozen
  aspiration: `snapshot_dense` 0.884 MRR / 0.804 nDCG@3 / 0.92 no-match, and `dense_views_on` 0.912 /
  0.839 / 0.92. The 0.48 no-match floor is confirmed well-calibrated (top-1 eq.3 scores span 0.58–0.93,
  all above the floor; the old "compressed ~0.016" alarm was the RRF artifact, not eq.3). See
  `tests/e2e/reports/t11/T11-VALIDATION-REPORT.md` and the assessment doc §8.

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

```rust
relevance_threshold: 0.48,
```

Candidates whose eq.3 score falls below 0.48 are excluded before fusion and never returned.

### Calibration evidence (recalibrated on the real 234-corpus, #209)

The floor was **recalibrated from 0.450 to 0.48** on the real 234-skill corpus (#209, 2026-06-08),
superseding the original 8-skill toy calibration. It was re-measured by sweeping the floor on the
**live mcp-server** (`find_skill` over HTTP + a real claude judge; 40 tuning positives + 20 off-topic
negatives), calibrating on the tuning split and validating on the disjoint held-out split:

| floor | no_match precision | pos hit@3 | pos MRR (tuning, judge-aug) |
|---|---|---|---|
| 0.45 (old) | 0.600 | 0.725 | 0.533 |
| 0.46 | 0.800 | 0.675 | 0.596 |
| **0.48 (chosen)** | **1.000** | **0.800** | 0.662 |
| 0.50 | 1.000 | 0.750 | 0.683 |

The old 0.450 was miscalibrated **too low**: on a heterogeneous corpus it admitted mediocre-eq3
skills that displaced better ones in the top-k *and* let off-topic queries fabricate matches
(no_match precision only 0.600). Raising the floor to 0.48 improves **both** negative rejection
(→1.000) and positive ranking — removing low-score noise cleans the returned top-k rather than
trading recall for precision. Held-out validation at 0.48: no_match precision 1.000, MRR 0.767,
hit@3 0.867, recall@3 0.808. 0.48 is the lowest floor reaching perfect no_match precision (most
recall headroom for unseen queries). Full evidence: the `RetrievalConfig::default()` comment and
`docs/assessments/2026-06-07-retrieval-quality-234-corpus-measured.md`.

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

### #209 — floor recalibrated on the real corpus (RESOLVED)

The no-match relevance floor was recalibrated from 0.450 to **0.48** on the real 234-skill corpus
(2026-06-08), measured by sweeping the floor on the live server (see §4). The old 0.450 (8-skill
calibration) was too low — it leaked off-topic fabrications (no_match precision 0.600) and admitted
ranking noise. 0.48 achieves perfect no_match precision and higher positive quality. Resolved.

### Multi-view structured fields (migration 009) — reader status update (T04, 2026-06)

Migration `009_skill_multiview_fields.sql` added seven nullable `TEXT[]` columns to `skills`
(`use_when`, `avoid_when`, `artifacts`, `tools`, `invariants`, `requires`, `produces`).
Its header originally stated "No production code SELECTs these columns yet." That is no longer
accurate as of T04 (2026-06). Current readers:

- `rebuild.rs::list_skills` (`infrastructure/src/persistence/rebuild.rs`) — reads all seven fields
  into `SkillRecord` to populate the in-memory `RetrievalSnapshot`.
- `build_graph_from_pg` (`crates/mcp-server/src/lib.rs`) — reads them for snapshot construction
  at boot and on each graph-version change.
- `inspect_skill` (`crates/mcp-server/src/lib.rs`) — surfaces them in the MCP tool response.

The migration DDL itself is unchanged (migrations are append-only); this note documents that the
write-ahead schema is now fully live on both the write side (session-extractor/graph-builder) and
the read side (retrieval snapshot + MCP tool responses).

### Capped prior means cold-start is mild, not fatal

A relevant new skill (usage_count=0) competes purely on cosine and subunit evidence. Because
α=0.45 and β=0.35 together account for up to 80% of the base score (before the ≤0.03 prior
ceiling), a relevant new skill surfaces fine on a relevant query. It may rank ≤0.03 score points
below an equally-relevant established peer — a mild tiebreaker, not exclusion. The regression
test in `scoring.rs` locks this guarantee.
