---
title: "feat: V1.7 Local Hybrid SkillDAG Retrieval"
type: feat
status: active
date: 2026-06-08
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
constitution_version: "2.1.0"
constitution_waivers: []
brainstorm_ref: null
architecture_ref: null
source_docs:
  tickets:
    - todos/205-pending-p0-prove-efficacy-task-outcome-ab-harness.md
    - todos/216-pending-p0-seed-corpus-via-self-ingestion-foundation.md
    - todos/218-pending-p0-swebench-compounding-efficacy-experiment.md
    - todos/220-pending-p1-priming-mode-recurrence-global-trigger-split.md
  local_docs:
    - docs/assessments/2026-06-07-brutal-grok-efficacy-and-fakes-assessment.md
    - docs/assessments/2026-06-07-retrieval-quality-234-corpus-measured.md
    - docs/assessments/2026-06-08-community-graph-why-harmful-and-grounded-path-208.md
    - docs/assessments/2026-06-08-ollama-num-ctx-truncation-176.md
    - docs/assessments/2026-06-08-local-vs-cloud-extraction-gap-214.md
    - docs/reference/retrieval-contract.md
    - docs/reference/online-retrieval-cqrs.md
    - docs/reference/skill-md-format.md
  external_research:
    - https://arxiv.org/abs/2606.03056
    - https://github.com/Ericbai06/SkillDAG
    - https://qwenlm.github.io/blog/qwen3-embedding/
    - https://ollama.com/library/qwen3-embedding
    - https://qdrant.tech/documentation/search/text-search/
    - https://qdrant.tech/documentation/manage-data/vectors/
handoff:
  problem_narrative: true
  user_story: true
  architectural_context: true
  success_criteria: true
  execution_slices: true
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
  rationale: "Each slice must ship a measured retrieval capability behind flags or a complete agent-facing retrieval surface, not a horizontal refactor."
---

# V1.7 Local Hybrid SkillDAG Retrieval Plan

This is a planning artifact only. Do not implement from this document in the planning session.

## Problem Narrative

The governing verdict has not changed: the system is now mostly correct, but usefulness is still unproven. Extraction is credible enough for the next chapter when driven by `claude-code`: the latest real-worker comparison shows `claude-code` producing 71 drafts across 10 transcripts with a non-empty-procedure rate around 0.68, while fixed local Gemma now produces real procedural output but at lower density around 0.256 and much worse throughput. For the efficacy proof, assume the high-quality corpus is extracted with `claude-code`.

Retrieval is the live doubt. The current default is no longer bad: after cutting community boost and recalibrating the no-match floor, the 234-skill real-server retrieval rig improved judge-augmented held-out MRR from 0.594 to 0.767, nDCG@3 to 0.749, hit@3 to 0.867, and no-match precision to 1.000. That is close to the frozen 0.80 aspiration but still short, and the existing architecture still has real limitations:

- Skill-level embedding text is only `name + description + tags`.
- Request-time candidate generation is in-memory dense cosine over `RetrievalSnapshot`, not Qdrant.
- Qdrant is currently write-side durability only, per the CQRS contract.
- Community/HDBSCAN output is organizational and diagnostic, not a trustworthy rank signal.
- The graph does not yet expose typed relationships such as dependencies, alternatives, or conflicts.
- Agent-facing retrieval is still basically "ranked matches", not SkillDAG-style "matches plus neighbors plus conflicts plus show bodies on demand".

The user constraint is decisive: no external paid embedding or cross-encoder APIs. Users may have Claude Code, Codex, or similar subscribed harnesses, but should not need separate API keys or per-use paid embedding/reranking calls. V1.7 therefore needs local open-weight retrieval intelligence: start with `qwen3-embedding:4b` in local Ollama, add local hybrid lexical/BM25 retrieval, optionally add a local reranker behind a latency gate, and expose graph structure to the agent in a way that helps selection without blindly injecting more context.

## User Story

As a developer using a local-first skill layer with Claude Code, Codex, or another agentic harness, I want retrieval to quickly find the right skills, explain adjacent dependencies and conflicts, and avoid injecting irrelevant context, so that the system can improve task outcomes without requiring external API keys or slowing every prompt.

## Architectural Context

Current storage and extraction flow:

- Extraction writes proposed skills as `.skills/<slug>/SKILL.md.pending` through `crates/session-extractor/src/writer.rs`.
- Human approval remains filesystem-observable: rename `SKILL.md.pending` to `SKILL.md`.
- Only active `SKILL.md` files are ingested by graph-builder. Lifecycle constants live in `crates/domain/src/lifecycle_files.rs`; active file checks live around `crates/graph-builder/src/watcher.rs`.
- The canonical skill format is YAML frontmatter for `name`, `description`, and `tags`, plus markdown sections such as `## Procedures`, `## Conventions`, `## Assets`, `## Evidence`, and `## Summary`. See `docs/reference/skill-md-format.md`.
- Graph-builder parses the real `SKILL.md` with `crates/graph-builder/src/extraction/rules.rs`.
- Graph-builder currently embeds skill-level text as `name + description + tags` in `crates/graph-builder/src/graph/build.rs`.
- Subunits are extracted from markdown body bullets and become retrievable evidence for the beta term in eq.3.

Current graph and persistence flow:

- Graph-builder computes communities in `crates/graph-builder/src/graph/communities.rs`, currently via HDBSCAN plus tag/community fallbacks.
- Graph rebuild persists the canonical graph to Postgres and writes vectors to Qdrant through the outbox path in `crates/graph-builder/src/graph/rebuild.rs` and `crates/infrastructure/src/vector/qdrant.rs`.
- Qdrant currently stores dense vectors only as write-side durable state. It is not queried by `compile_context` or `find_skill`.
- `docs/reference/online-retrieval-cqrs.md` explicitly states the read path is the in-memory `RetrievalSnapshot` rebuilt from Postgres on graph events.

Current retrieval flow:

- `crates/mcp-server/src/lib.rs::build_graph_from_pg` loads skills from Postgres, recomputes skill and subunit embeddings, computes usage priors, attaches community centroids, and builds a `RetrievalSnapshot`.
- `crates/retrieval/src/orchestrator.rs` owns config, snapshot swapping, health markers, and the `retrieve` flow.
- `crates/retrieval/src/dual_scope.rs` resolves project/global scopes, filters skills by scope/source paths, calls `rank_by_cosine`, computes semantic subunit evidence through `search_graph`, applies eq.3, filters by relevance floor, applies MMR, then weighted RRF fuses scopes.
- `crates/retrieval/src/scoring.rs` keeps eq.3: `alpha * l1_semantic + beta * subunit_evidence + gamma * prior`, with community boost now defaulting off.
- `compile_context` injects compiled context with suppression/cache behavior; `find_skill` returns raw ranked matches and is the better surface for task-time, agent-driven retrieval.

Current measured truths to preserve:

- Default no-match relevance floor is 0.48, calibrated on the real 234-skill corpus.
- Community boost must remain off by default. Do not reintroduce graph/community as a scalar multiplier.
- Regression gate remains judge-augmented held-out MRR >= 0.60 and no-match precision >= 0.90; 0.80 MRR/nDCG remains the aspiration, not a faked green.
- All retrieval quality measurement must drive the real running `mcp-server` over HTTP. No in-process reconstruction of retrieval, scope fusion, or scoring.

SkillDAG research conclusions to port carefully:

- SkillDAG's most relevant design is not "graph boosts dense ranking"; it is an agent-callable typed graph interface.
- `search(q, K, D)` returns separate `matches`, `neighbors`, and `conflicts`.
- `matches` are dense/vector top-K. The graph is not mixed into this list.
- `neighbors` are depth-bounded BFS over positive typed edges such as `depends_on`, `specializes`, `composes_with`, and `similar_to`.
- `conflicts_with` is a one-hop prune/do-not-co-select signal and is not traversed.
- `show` loads full skill bodies on demand.
- Cold-start edge construction uses multiple views: `e_self` for what a skill does and `e_needs` for what it requires, then candidate-pair classification.
- Online graph edits use propose-first dry runs and commit-only-with-evidence edits.

External retrieval model context:

- Qwen's official Qwen3 Embedding series includes 0.6B, 4B, and 8B embedding models plus matching rerankers. The official model overview lists `Qwen3-Embedding-4B` as a 4B, 32K sequence-length, 2560-dimensional embedding model, instruction-aware and MRL-capable.
- Ollama publishes `qwen3-embedding:4b` as a local model with a 40K context window and 2.5GB model size on the Ollama library page.
- Qdrant supports sparse vectors and BM25-style text search via sparse vector configuration with IDF weighting. Qdrant's docs describe BM25 as using term frequency, inverse document frequency, and document length, and expose sparse vectors alongside dense vectors.

## Non-Goals

- Do not prove final task efficacy in this plan. V1.7 prepares retrieval for #205/#218, but the efficacy proof still needs the benchmark harness.
- Do not use external paid embedding, reranking, or LLM APIs as required runtime dependencies.
- Do not make an LLM query-decomposition call on the hot path.
- Do not re-enable community boost as a ranking multiplier.
- Do not quietly change the CQRS read-path contract. If Qdrant becomes a request-time dependency, document that as an intentional architecture migration.
- Do not load full skill bodies into every prompt. Agent-facing `show` should keep bodies on demand.

## Success Criteria

V1.7 is successful only if it improves retrieval while preserving speed, local-first operation, and no-match honesty.

Quality targets:

- On the existing 234-skill real-server held-out rig, the best local-first V1.7 arm should reach judge-augmented MRR >= 0.80 and nDCG@3 >= 0.80 while keeping no-match precision >= 0.90.
- If 0.80 is not reached, the plan is still shippable only if the measured delta over current default 0.767 is positive, documented, and useful for #218 instrumentation.
- Any new default must preserve or raise the regression gate. Never lower the gate to fit a weak result.

Latency targets:

- `compile_context` p95 stays under the constitutional 500ms target on the measured corpus.
- `find_skill` and the new graph search tool should target p95 under 500ms for normal top-K searches on the 200-500 skill V1 corpus.
- Optional reranking may have a looser opt-in budget, but cannot be default until measured p95 stays inside the user-facing budget.

Local-first targets:

- Default data plane uses local Ollama, local Qdrant, and local Postgres.
- `qwen3-embedding:4b` is the first measured embedder arm.
- Reranker, if implemented, is local-only and disabled by default until it earns its latency/quality cost.

Agent usefulness targets:

- Agent-facing search returns separate matches, neighbors, conflicts, and rationale.
- Full skill bodies are loaded on demand, not blindly injected.
- The agent can see why a neighbor appears and whether it is dependency, specialization, composition, similarity, or conflict evidence.

## Execution Shape

- **Mode:** vertical-slices
- **Why:** Each slice can be validated as a real retrieval behavior against the running stack, while keeping risky migrations behind flags until measured.

## TDD & Evidence Contract

- **Precedence:** Plan-level TDD overrides local defaults.
- **Effective mode:** Ralph-driven TDD.
- **Effective loop:** Failing tests first -> minimal implementation -> refactor -> post-refactor rerun.
- **Required evidence:** Unit tests for schema/scoring/tool-format behavior; real-server e2e retrieval quality and latency evidence for behavior that affects ranking.
- **Exceptions:** None.

## Constitution Alignment

- **Local-first:** Preserved. The retrieval data plane remains local. Qwen/Qdrant/reranker work must not require external paid APIs.
- **Zero-touch session start:** Preserved only if default `compile_context` stays under 500ms. Heavy reranking or LLM decomposition cannot run by default at SessionStart.
- **Human gate for mutations:** Preserved. New skills still land as `.pending`; graph edge edits must use propose/commit semantics and should require explicit agent or human evidence.
- **Portable scope:** Preserved. New structured fields must live in portable `SKILL.md` frontmatter/body sections, not harness-specific sidecars unless sidecars are purely cache/index artifacts.
- **Filesystem-observable state:** Preserved for skills. Edge mutations need an observable audit trail, ideally Postgres plus optional filesystem export or generated graph report.
- **Approval-sensitive changes:** This plan includes model changes, schema migrations, and likely Qdrant collection/schema changes. Execution must stop for owner approval where the constitution requires it.

## Design Decisions

1. **Primary next embedder:** Start with local Ollama `qwen3-embedding:4b`.

   Rationale: it is local, strong on retrieval/code tasks relative to tiny local defaults, already being pulled by the user, and avoids external API keys. The model dimension and instruction behavior must be discovered from the actual Ollama response and recorded in graph metadata before any rebuild.

2. **Hybrid retrieval:** Add BM25/sparse lexical retrieval to dense retrieval.

   Rationale: the skill corpus contains exact tool names, file formats, crate names, APIs, and invariants that dense embeddings often blur. BM25 is cheap and local. Qdrant can store sparse vectors and combine dense/sparse candidate pools, but the current repo does not query Qdrant at request time, so this is an architecture migration, not a small ranking tweak.

3. **Qdrant candidate generation should start in shadow mode.**

   Rationale: using Qdrant on the hot path breaks the current DS-003-style resilience claim that stopping Qdrant does not degrade `compile_context`. The correct path is to add `snapshot_dense` vs `qdrant_hybrid` candidate backends behind config, measure both, update docs/ADR if Qdrant becomes default, and keep fallback behavior explicit.

4. **Graph structure should produce separate graph evidence, not a scalar rank boost.**

   Rationale: #208 proved the old community multiplier is wrong. SkillDAG also keeps vector matches separate from neighbors/conflicts. Use graph edges for expansion, explanation, pruning, and agent choice.

5. **Structured extraction format must grow before graph quality can grow.**

   Rationale: typed edges need better source material than a name, summary, and tags. Skills should expose what they do, when to use them, prerequisites, artifacts, APIs/tools, invariants, failure modes, anti-patterns, and relationships. This can be done while preserving existing readers by adding optional fields/sections.

6. **Query decomposition is not a default runtime LLM feature.**

   Rationale: the user cares about quick retrieval. V1.7 may support cheap decomposition only: caller-provided subqueries, deterministic parsing of explicit files/tools/invariants, or multiple embeddings of already-available fields if latency stays inside budget. No extra LLM call on the hot path.

7. **Reranking is optional and local-only.**

   Rationale: a local Qwen3 reranker or other local cross-encoder could attack P@1, but it must earn its cost in a top-N rerank arm. Default retrieval cannot depend on a 4B cross-encoder if p95 exceeds the budget.

## Proposed V1.7 Architecture

### Data Model Additions

Extend the canonical skill representation with optional fields and sections. Existing `SKILL.md` files remain valid.

Recommended optional frontmatter fields:

- `use_when`: short list of task triggers.
- `avoid_when`: short list of negative triggers.
- `artifacts`: file types, protocols, config names, or repo objects the skill applies to.
- `tools`: commands, libraries, frameworks, services, models, or APIs.
- `invariants`: verifier-critical constraints.
- `requires`: prerequisites the skill assumes.
- `produces`: outcome or artifact produced by following the skill.
- `edge_hints`: optional human/agent-proposed relation hints, not authoritative by themselves.

Recommended optional body sections:

- `## Use When`
- `## Avoid When`
- `## Prerequisites`
- `## Invariants`
- `## Failure Modes`
- `## Related Skills`
- `## Evidence`

Embedding views:

- `e_summary`: name + description + tags, current alpha-equivalent view.
- `e_task`: use_when + procedures + artifacts + tools, optimized for task query matching.
- `e_needs`: prerequisites + requires + failure modes, optimized for dependency edge candidates.
- `e_negative`: avoid_when + anti-patterns, used for conflict/negative evidence and maybe no-match safeguards.
- `e_subunit`: existing per-subunit embeddings.

Do not concatenate every full body into one giant embedding input by default. The previous embedding-window work exists for a reason. Multi-view bounded text is preferable to one unbounded text blob.

### Indexing Additions

Dense:

- Index `e_summary` and `e_task` with `qwen3-embedding:4b`.
- Preserve model name, model digest if available, dimension, and instruction prefix policy in graph metadata.
- Force/recommend rebuild on model or dimension change. A mixed `nomic`/`qwen` index is invalid for cosine comparisons.

Sparse/BM25:

- Build a lexical document per skill from bounded, high-signal fields: name, tags, description, tools, artifacts, invariants, procedures headings/bullets.
- Store sparse vectors in Qdrant if the Qdrant backend is selected.
- If Qdrant sparse/BM25 support is too heavy or version-dependent in the Rust adapter, fallback candidate is an in-memory BM25 index inside `RetrievalSnapshot` for V1.7, then migrate to Qdrant later. The plan preference is Qdrant hybrid, but speed and implementation risk decide.

Graph:

- Add typed edges with at least:
  - `depends_on`
  - `specializes`
  - `composes_with`
  - `similar_to`
  - `conflicts_with`
- Edge fields should include `source_skill_id`, `target_skill_id`, `edge_type`, `origin`, `confidence`, `reason`, `evidence`, `created_at`, and `updated_at`.
- Positive walkable edge types: `depends_on`, `specializes`, `composes_with`, `similar_to`.
- `conflicts_with` is not walkable and is returned separately as a prune signal.
- Directed acyclicity should apply to the backbone edge types where meaningful: `depends_on` and probably `specializes`.

### Retrieval Flow

Default target flow after V1.7, assuming Qdrant earns default status:

1. Receive prompt and scope context.
2. Embed query locally with `qwen3-embedding:4b`.
3. Build cheap lexical query from the same prompt with deterministic tokenization and exact tool/file/API terms.
4. Per scope, gather candidates from dense and sparse channels.
5. Fuse dense and sparse candidate pools with RRF or a measured weighted blend.
6. Compute existing semantic subunit evidence for candidate details.
7. Apply no-match/relevance gate calibrated for the new embedder/backend.
8. Apply MMR/diversity.
9. Return matches.
10. For graph search tools, separately expand neighbors over typed edges and list direct conflicts.
11. Optional local reranker over top-N only if enabled and within latency budget.

Important: graph neighbors should not silently enter `compile_context` as if they were primary matches. They should be exposed to the agent in a separate structure unless measured evidence says automatic inclusion helps.

### Agent-Facing Surfaces

Keep `compile_context` conservative. Add or evolve task-time retrieval surfaces around `find_skill`.

Recommended new MCP tool:

`search_skill_graph(prompt, limit = 5, depth = 1, include_bodies = false)`

Return shape:

```json
{
  "matches": [
    {
      "skill_id": "...",
      "name": "...",
      "score": 0.0,
      "dense_score": 0.0,
      "lexical_score": 0.0,
      "subunit_evidence": 0.0,
      "matched_scope": "project",
      "why": ["matched tool: qdrant", "procedure evidence: sparse vector config"]
    }
  ],
  "neighbors": [
    {
      "skill_id": "...",
      "name": "...",
      "reached_from": "...",
      "depth": 1,
      "via": "depends_on",
      "reason": "..."
    }
  ],
  "conflicts": [
    {
      "skill_id": "...",
      "conflicts_with": "...",
      "reason": "..."
    }
  ],
  "query_views": {
    "dense": "...",
    "lexical_terms": ["..."],
    "decomposition": []
  },
  "latency_ms": 0,
  "graph_version": 0
}
```

Recommended companion tools:

- `show_skill(skill_id)`: return full `SKILL.md` body on demand.
- `explain_skill_match(prompt, skill_id)`: optional diagnostic using stored scores and subunit evidence, no LLM required.
- `propose_skill_edge(source, target, type, reason)`: dry-run only, returns related existing edges/history.
- `edit_skill_edge(...)`: commit path, evidence required, approval-sensitive.

`compile_context` can later consume `search_skill_graph` internally, but V1.7 should first expose the richer surface and measure agent behavior.

## Execution Slices

### Slice 1 - V1.7 Measurement Harness Arms

- **Slice type:** vertical tracer
- **Serves:** Measurement before ranking changes.
- **Demo scenario:** Run the existing 234-corpus real-server quality rig and compare current default vs qwen embedder vs hybrid backend arms without changing production default.
- **Feature home:** `tests/e2e` quality harness and `scripts/retrieval_quality_*`.
- **Scope:** Add config/reporting support for V1.7 retrieval arms.
- **Scope fence:** Do not alter retrieval behavior except through config flags used by the real server.
- **Files:** `scripts/retrieval_quality_live.py`, `scripts/retrieval_quality_sweep.py`, `tests/e2e/reports/`, likely config docs.
- **Depends on:** Current #210 harness.
- **Dependency type:** measurement foundation.
- **Success criteria:** Reports include backend, embedder model, dense/sparse/rerank flags, latency, MRR, nDCG, hit@3, recall@3, no-match precision.
- **Test command:** Real-server retrieval quality sweep command used by #210, extended with V1.7 arms.

### Slice 2 - Local Qwen3 Embedder Backend and Rebuild Safety

- **Slice type:** vertical infrastructure behavior
- **Serves:** Better dense retrieval without external APIs.
- **Demo scenario:** Configure local Ollama to use `qwen3-embedding:4b`, rebuild graph, and run `find_skill` against the real server with model metadata visible in reports.
- **Feature home:** `crates/infrastructure/src/embeddings`, `crates/graph-builder`, `crates/mcp-server`.
- **Scope:** Make embedding model/dimension/version observable and safe to change.
- **Scope fence:** Do not tune ranking yet. Do not mix old and new vectors.
- **Files:** `crates/infrastructure/src/embeddings/ollama.rs`, `crates/graph-builder/src/graph/build.rs`, `crates/mcp-server/src/lib.rs`, persistence metadata/migrations if needed.
- **Depends on:** Slice 1.
- **Dependency type:** model backend.
- **Success criteria:** Qwen arm produces correctly dimensioned vectors, graph rebuild fails loudly on dimension mismatch, reports record model metadata, existing `nomic` path still works.
- **Test command:** Unit tests for metadata/dimension guards plus real-server retrieval sweep with `OLLAMA_EMBED_MODEL=qwen3-embedding:4b` or the repo's chosen env name.

### Slice 3 - Expanded Skill Format and Multi-View Extraction Fields

- **Slice type:** vertical data contract
- **Serves:** Better dense/sparse matching and typed edge construction.
- **Demo scenario:** A newly extracted `SKILL.md.pending` includes optional use/avoid/prereq/artifact/tool/invariant fields; graph-builder parses it; old skills still parse unchanged.
- **Feature home:** `crates/session-extractor` plus `crates/graph-builder/src/extraction`.
- **Scope:** Extend writer, parser, and docs for optional structured fields.
- **Scope fence:** Do not require all old skills to be migrated before retrieval works.
- **Files:** `docs/reference/skill-md-format.md`, `crates/session-extractor/src/writer.rs`, extraction prompt contract files, `crates/graph-builder/src/extraction/rules.rs`, roundtrip tests.
- **Depends on:** Slice 2 can run independently, but Slice 4 depends on this.
- **Dependency type:** schema/source data.
- **Success criteria:** Roundtrip test proves real writer output survives real graph-builder parsing; missing optional fields behave as empty; no `.pending` bypasses human gate.
- **Test command:** Existing skill-md roundtrip integration test plus session-extractor unit tests.

### Slice 4 - Hybrid Dense/BM25 Candidate Generation

- **Slice type:** vertical retrieval behavior
- **Serves:** Exact-term recall for tools, APIs, files, invariants, and repo-specific language.
- **Demo scenario:** A query containing an exact crate/tool/API term retrieves the right skill even when dense-only ranks it below top-3.
- **Feature home:** `crates/retrieval`.
- **Scope:** Add `snapshot_dense` vs `qdrant_hybrid` or `snapshot_hybrid` candidate backend behind config.
- **Scope fence:** Do not make Qdrant a default hot-path dependency until latency and resilience docs are updated.
- **Files:** `crates/retrieval/src/*`, `crates/infrastructure/src/vector/qdrant.rs`, `crates/mcp-server/src/lib.rs`, Qdrant collection setup/migrations or in-memory lexical index files.
- **Depends on:** Slice 1 and preferably Slice 2.
- **Dependency type:** ranking candidate generation.
- **Success criteria:** Hybrid arm improves or at least does not regress held-out MRR/nDCG/no-match precision; p95 stays under 500ms; Qdrant dependency semantics are explicit in health markers when hybrid mode is active.
- **Test command:** Unit tests for fusion/lexical scoring plus real-server retrieval sweep comparing dense-only vs hybrid.

### Slice 5 - Typed Skill Graph Storage and Cold-Start Edge Proposals

- **Slice type:** vertical graph capability
- **Serves:** SkillDAG-style structural retrieval without graph-as-blind-boost.
- **Demo scenario:** Given two active skills where one clearly depends on the other, the graph stores a typed `depends_on` edge with reason/evidence and graph search returns it as a neighbor.
- **Feature home:** `crates/graph-builder` and persistence graph schema.
- **Scope:** Add typed edge schema, edge history, validation, and initial edge proposal generation.
- **Scope fence:** Cold-start edge classification must not require external API keys. If agent/Claude Code classification is used, keep it an explicit maintenance/offline command, not automatic hot-path behavior.
- **Files:** Postgres migrations, `crates/graph-builder/src/graph/*`, persistence adapters, docs.
- **Depends on:** Slice 3.
- **Dependency type:** graph data.
- **Success criteria:** Edges persist with type/reason/origin; invalid cycles or contradictory backbone edges fail; `conflicts_with` exists but is not traversed as a positive neighbor.
- **Test command:** Unit tests for edge validation plus integration test around rebuild/persistence.

### Slice 6 - SkillDAG-Style Agent Retrieval Tools

- **Slice type:** vertical agent-facing behavior
- **Serves:** Let agents choose and inspect skills instead of receiving opaque injected context.
- **Demo scenario:** An agent calls `search_skill_graph`, sees top matches, one-hop dependencies/alternatives, direct conflicts, and then calls `show_skill` only for the promising skill bodies.
- **Feature home:** `crates/mcp-server/src/tools` plus `crates/retrieval`.
- **Scope:** Add MCP tool(s) and protocol schema for structured graph search and show-by-id.
- **Scope fence:** Keep `compile_context` conservative unless measured evidence says richer automatic injection helps.
- **Files:** `crates/mcp-server/src/protocol.rs`, `crates/mcp-server/src/tools/*`, `crates/retrieval/src/*`, compiler only if `compile_context` is intentionally changed.
- **Depends on:** Slice 4 for hybrid scores, Slice 5 for neighbors/conflicts.
- **Dependency type:** user-facing retrieval surface.
- **Success criteria:** Tool output separates `matches`, `neighbors`, and `conflicts`; output includes why fields and latency; full bodies are on demand.
- **Test command:** MCP protocol/unit tests plus real-server e2e call through HTTP/MCP harness.

### Slice 7 - Optional Local Reranker and Cheap Query Decomposition

- **Slice type:** optional measured enhancement
- **Serves:** Close the last P@1/MRR gap if dense+BM25 still falls short.
- **Demo scenario:** Enable local reranker over top-20 candidates and show improved P@1/MRR without exceeding the configured latency budget.
- **Feature home:** `crates/retrieval`.
- **Scope:** Add opt-in local reranker and bounded query decomposition.
- **Scope fence:** No external API. No default LLM decomposition call. Disable by default until measured.
- **Files:** retrieval rerank module, local model adapter if needed, config/docs/tests.
- **Depends on:** Slice 4.
- **Dependency type:** ranking refinement.
- **Success criteria:** Reranker arm reports quality delta and p95 latency; query decomposition is limited to deterministic extraction or caller-provided subqueries; default remains fast.
- **Test command:** Unit tests for rerank ordering plus real-server sweep with reranker off/on.

### Slice 8 - Contract Docs, ADR, and Efficacy Handoff

- **Slice type:** vertical documentation and gate
- **Serves:** Keep docs honest and hand #205/#218 a measured retrieval substrate.
- **Demo scenario:** A new execution session can read one contract doc and know exactly whether default retrieval uses snapshot dense, Qdrant hybrid, local rerank, typed graph expansion, and what quality/latency it measured.
- **Feature home:** `docs/reference` and `docs/assessments`.
- **Scope:** Update retrieval contract, CQRS/read-path doc, and write a V1.7 measured assessment.
- **Scope fence:** Do not claim graph improves retrieval unless the report shows it.
- **Files:** `docs/reference/retrieval-contract.md`, `docs/reference/online-retrieval-cqrs.md`, new assessment doc, maybe ADR if Qdrant is promoted to default read path.
- **Depends on:** Slices 1-7 as applicable.
- **Dependency type:** governance and handoff.
- **Success criteria:** Docs match code; default backend and flags are clear; #205/#218 can attribute outcomes to retrieval hits, misses, latency, and graph evidence.
- **Test command:** Documentation review plus final quality/latency report.

## Query Decomposition Policy

Default V1.7 should not perform LLM query decomposition at runtime.

Allowed cheap forms:

- Agent-provided subqueries, where the calling harness already knows the task decomposes into multiple retrieval needs.
- Deterministic field extraction from the prompt: files, extensions, crate names, commands, services, frameworks, error codes, protocols, and quoted invariants.
- Static multi-view querying where the same prompt is embedded once and lexical terms are extracted without model calls.
- A small bounded number of additional dense queries only if measured p95 stays under 500ms.

Deferred forms:

- LLM-generated query plans.
- HyDE-style synthetic documents.
- Multi-turn retrieval planning.
- Per-prompt local 4B model calls just to rewrite the query.

The only exception is an explicit agent-facing tool call where the agent chooses to spend time. That is not part of zero-touch `compile_context`.

## Risk Register

- **Qdrant read-path dependency risk:** Hybrid Qdrant retrieval changes the CQRS resilience contract. Mitigation: flag/shadow first, update health/docs/ADR before default.
- **Embedding model migration risk:** Qwen vector dimension differs from nomic. Mitigation: store model metadata, fail loudly on mixed dimensions, rebuild fully.
- **Latency risk:** Qwen embedding and reranking may be slower than nomic. Mitigation: measure p95 on real server, use reranker only top-N and opt-in.
- **Sparse search overconfidence:** BM25 can over-rank exact but irrelevant term matches. Mitigation: fuse with dense and keep calibrated no-match gate.
- **Schema bloat risk:** Too many structured fields can make extraction brittle. Mitigation: optional fields, writer-reader roundtrip tests, bounded embedding views.
- **Graph hallucination risk:** Automatically inferred edges may be wrong. Mitigation: store edge confidence/origin/reason, use proposal workflow, avoid automatic conflict generation without evidence.
- **Prompt bloat risk:** SkillDAG-style neighbors may increase injected context. Mitigation: expose neighbors separately and require `show_skill` for full bodies.
- **Measurement leakage risk:** Tuning on held-out data would fake progress. Mitigation: preserve tuning/held-out split and drive real server only.

## Complexity Justification

This is more than "swap the embedder" because the measured gap is not a single knob:

- Dense embeddings are probably the main ceiling, but exact artifacts/tools matter in coding tasks.
- Qdrant can support hybrid retrieval, but using it at request time is an architectural migration.
- SkillDAG's gain comes from typed graph surfaces and agent agency, not a hidden graph multiplier.
- Efficacy proof needs instrumentation that can say whether the agent used the right match, a dependency, an alternative, or no skill.

The plan keeps risk bounded by shipping measured slices behind flags and only promoting defaults after live quality and latency evidence.

## Open Questions for Architecture Phase

1. Should V1.7 prefer Qdrant hybrid as the eventual default, or keep in-memory snapshot retrieval default until corpus size exceeds the ADR-0001 threshold?
2. What exact Qdrant version is assumed in Docker Compose, and does it support the needed sparse/BM25 mode locally without cloud inference?
3. Should typed edges live only in Postgres, or should a filesystem-visible graph export also exist to satisfy the spirit of filesystem-observable state?
4. Is `show_skill(skill_id)` enough, or should `find_skill` be evolved in place to avoid adding another tool?
5. Should edge proposal classification use Claude Code as an explicit maintenance action, or start deterministic/manual only?
6. What is the hard p95 budget for `find_skill` separate from `compile_context`? The constitution says session-start compilation under 500ms, but task-time agent search may tolerate a different budget if explicit.

## Recommended Default Execution Order

1. Add measurement arms first.
2. Add Qwen embedder metadata and full rebuild safety.
3. Measure Qwen-only against current default.
4. Extend skill schema and parser.
5. Add hybrid dense/BM25 candidate generation in shadow/flagged mode.
6. Measure hybrid against Qwen-only and current default.
7. Add typed graph storage/proposals.
8. Add SkillDAG-style agent-facing search/show tools.
9. Try local reranker only if the quality gap remains and latency budget has room.
10. Update docs/ADR and hand measured defaults to #205/#218.

## Final Gate

Before V1.7 is considered ready for efficacy work:

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets --features test-utils -- -D warnings`
- `cargo test --workspace --all-targets --features test-utils`
- Real-server retrieval sweep for current default vs V1.7 default.
- Real-server retrieval latency report.
- Retrieval contract docs updated.
- If Qdrant is default hot path, CQRS doc and ADR updated to remove the old "Qdrant down does not degrade compile_context" claim or scope it to the snapshot backend only.
- The final assessment states honestly whether V1.7 hit 0.80 MRR/nDCG or remains short.

## References

- Battle plan: `docs/assessments/2026-06-07-brutal-grok-efficacy-and-fakes-assessment.md`
- Retrieval measurement: `docs/assessments/2026-06-07-retrieval-quality-234-corpus-measured.md`
- Graph cut rationale: `docs/assessments/2026-06-08-community-graph-why-harmful-and-grounded-path-208.md`
- Ollama context truncation: `docs/assessments/2026-06-08-ollama-num-ctx-truncation-176.md`
- Local vs cloud extraction: `docs/assessments/2026-06-08-local-vs-cloud-extraction-gap-214.md`
- Retrieval contract: `docs/reference/retrieval-contract.md`
- CQRS read model: `docs/reference/online-retrieval-cqrs.md`
- Skill format: `docs/reference/skill-md-format.md`
- SkillDAG paper: https://arxiv.org/abs/2606.03056
- SkillDAG repo: https://github.com/Ericbai06/SkillDAG
- Qwen3 Embedding official blog: https://qwenlm.github.io/blog/qwen3-embedding/
- Ollama qwen3-embedding model page: https://ollama.com/library/qwen3-embedding
- Qdrant BM25/text search docs: https://qdrant.tech/documentation/search/text-search/
- Qdrant sparse vector docs: https://qdrant.tech/documentation/manage-data/vectors/
