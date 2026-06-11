# V1.7 Retrieval — Measured Assessment & Efficacy Handoff

> **Status: honest snapshot at the close of V1.7 Phase A (2026-06-11).**
> This assessment records what V1.7 retrieval *actually ships*, what was *measured*,
> what was *deferred*, and what the efficacy work (#205/#218 → T14/T15) can rely on.
> It does not backfill unbuilt claims. Where a number cannot be honestly measured on
> the current corpus, that is stated as a gap, not papered over.

Branch: `feat/v-1-7`. Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`.

---

## 1. What V1.7 ships (the retrieval contract)

| Dimension | Shipped default | Status |
|---|---|---|
| **Embedding arm** | `qwen3-embedding:4b` (dim 2560), model-keyed collection `skills__qwen3-embedding-4b` | **Default** (T02). `infrastructure::ollama::DEFAULT_EMBEDDING_MODEL`; dim discovered live. |
| **Candidate-generation backend** | `snapshot_dense` — in-memory cosine over `RetrievalSnapshot` | **Default** (T04). Selector `RetrievalBackend` (`orchestrator.rs:166`), `RETRIEVAL_BACKEND` env, fail-loud parse. |
| `snapshot_hybrid` (in-memory dense + Okapi BM25 sparse over the 9 multi-view fields) | env opt-in | **Experimental** (T04). Measured **no uplift** on the current corpus. |
| `qdrant_hybrid` (named dense + sparse-idf Qdrant collection, Query-API read arm) | env opt-in | **Experimental** (T04). Reads Qdrant at query time → breaks the default CQRS read/write split; see `online-retrieval-cqrs.md`. |
| **Dense multi-view views** (`e_task`/`e_needs`/`e_negative`, max-over-views fusion) | `RETRIEVAL_DENSE_VIEWS=off` | **Default-OFF** (T09). Code complete; flag-OFF == byte-for-byte pre-T09 ranking. Measured ON/OFF delta deferred to T11 (see §4). |
| **Local reranker / query decomposition** | not shipped | **SKIPPED** (T07, owner decision 2026-06-11). Optional-by-design; T04 showed candidate-gen is not the ceiling. Revisit only if a corpus-aligned sweep finds a closeable gap within p95. |
| **Typed skill graph edges** | `skill_edges` (depends_on / composes_with / similar_to / conflicts_with) | **Shipped** (T05). Postgres-only; deterministic cold-start proposer; high-confidence auto-commit. |
| **Agent graph surface** | `search_skill_graph` (matches / neighbors / conflicts) + `find_skill` + `inspect_skill` | **Shipped** (T06). |
| **Scoring** | SkillRAE eq.3 (α=0.45 cosine, β=0.35 subunit, γ=0.20 prior, λ=0.25 community) | unchanged weights; **community term measured ranking-inert** (see §3). |
| **No-match floor** | `relevance_threshold = 0.48` (recalibrated on the 234-corpus, #209) | unchanged; `RETRIEVAL_RELEVANCE_THRESHOLD` override. NOT re-validated on the qwen3 262-corpus (see §4). |

### Agent-facing retrieval contract (T06, this batch)

`find_skill` and `search_skill_graph` now expose an honest, agent-consumable surface:

- **`score`** = eq.3 **relevance** (threaded `ScoredSkill.semantic_score`), NOT the RRF rank artifact.
- **`fusion_rank_score`** = the weighted-RRF value (cross-scope ordering provenance only).
- **`rationale`** = `["rrf=…","semantic=…","subunit_evidence=…","lexical=…"]` per match.
- **`retrieval_context`** = `{embedding_model, collection, graph_version}` provenance (#243), sourced from the persisted `embedding_model_metadata` active row + snapshot graph version.
- **`search_skill_graph`** returns separate `matches`, `neighbors` (positive edges incident on matched skills), `conflicts` (`conflicts_with`, never folded into neighbors), `latency_ms`.
- **`inspect_skill`** exposes the 7 multi-view fields (`use_when`/`avoid_when`/`artifacts`/`tools`/`invariants`/`requires`/`produces`).
- **`/health`** carries `embedding_arm` (`model=/dim=/collection=`) and `retrieval_backend` (`backend=…`) static components.

---

## 2. The #260 fix — relevance is now measurable (live-proven)

Before V1.7/T06, `find_skill.score` exposed the weighted-RRF rank artifact (`scope_weight × 1/(rrf_k+rank)`), capped ~0.0164 and quantized to ~2 values — **independent of match quality**. A 0.80-cosine match and a 0.50-cosine match at the same rank returned the identical `0.016`. This made the agent-facing score useless and produced the misleading "qwen3 scores are compressed ~0.016" reading.

**Fixed and verified live** against the real 262-skill qwen3 mcp-server (2026-06-11):

| Query | Exposed `score` (eq.3 relevance) | `fusion_rank_score` (RRF) |
|---|---|---|
| "qdrant hybrid retrieval backend" | 0.836 / 0.740 / 0.748 | 0.016393 / 0.016129 / 0.015873 |
| "clippy warnings gate" | 0.794 / 0.711 / 0.655 | 0.016393 / 0.016129 / 0.015873 |
| "conventional commit co-authored-by" | *(no_match — correct; topic absent)* | — |

The relevance scores discriminate by match quality (0.65–0.84) and the RRF artifact is preserved separately. A genuinely off-topic query correctly returns `no_match`. **qwen3 relevance is healthy; there is no "compressed score" threshold problem** — that earlier reading was the RRF artifact, now corrected.

Live contract proof: `tests/e2e/test_skill_graph_tools.rs` (3/3 `--ignored` PASS against the real server) asserts the structural contract and the score-distinctness property, and fails loud (does not skip) if a populated corpus returns no match.

---

## 3. The graph/community multiplier is ranking-inert (do not claim it helps)

eq.3 still computes `base · (1 + λ·community_boost)` with λ=0.25 (`scoring.rs`). Measurement (the 2026-06-07 brutal efficacy assessment) found `community_boost` **ranking-inert** — it is effectively binary/uniform across the candidate set, so the multiplier cancels out and does not change the ranking. The V1.7 decision (T05 scope fence, plan) is explicit: **do not reintroduce graph-as-multiplier**. Typed edges (T05) and the `search_skill_graph` neighbors/conflicts surface (T06) expose the graph to the *agent* as separate signals — they do **not** boost match scores. Any future claim that the graph improves *retrieval ranking* must be earned by measurement, not asserted.

---

## 4. What is NOT measured yet (the honest gaps)

### 4.1 Held-out MRR/nDCG on the multi-view corpus — BLOCKED on an aligned eval fixture

The only committed labeled eval fixture is `tests/fixtures/retrieval_quality_234_corpus_labeled.json`, whose 30 relevant-skill anchors are **0/30 present** in the live T10 262-skill qwen3 corpus (verified via PG join: `30|0`). Consequences:

- A held-out MRR/nDCG sweep against this fixture would score ≈0 for *any* arm — an uninformative, dishonest number. The T08 `test_command`'s `--gate --regression-floor 0.60` against this fixture **cannot pass honestly** on the dogfood corpus and was not forced to.
- The **frozen 0.80 MRR/nDCG@3 target is therefore NOT validated** on the V1.7 multi-view corpus — neither met nor refuted. The last honest held-out number (MRR 0.767, no_match precision 1.0) was measured on the *234* corpus with nomic (`2026-06-07-retrieval-quality-234-corpus-measured.md`); it does not transfer to the qwen3 262-corpus.

**Owned by T11** (`corpus-multiview-resweep-hybrid-validation`, now ready): build a 262-corpus-aligned labeled fixture, then run the real-server held-out sweep. T11 also absorbs **T09's deferred dense-views ON/OFF sweep** (same blocker).

### 4.2 No-match floor (0.48) not re-validated for qwen3

The 0.48 floor was calibrated on the 234-corpus with the prior embedder. qwen3's cosine distribution differs; the floor should be re-swept on the aligned fixture (T11) before being trusted as optimal. It is not currently a correctness problem (live queries return well-separated relevance and correct no_match), but it is unproven-optimal.

### 4.3 Hybrid bet still open

T04 measured **zero uplift** from `snapshot_hybrid` / `qdrant_hybrid` on the (pre-multi-view) corpus: all arms tied at MRR 0.767, p95 114/128/119 ms. The hypothesis ([[hybrid-is-the-retrieval-bet]]) is that a denser, multi-view-rich corpus + dense multi-view views (T09) will let hybrid/dense-views beat plain dense. That hypothesis is **untested** until T11 runs the aligned sweep. `snapshot_dense` stays default until measurement says otherwise.

---

## 5. Latency

T04 real-server sweep (234-corpus, qwen3 warm): p95 ≈ **114 ms** (snapshot_dense) / 128 ms (snapshot_hybrid) / 119 ms (qdrant_hybrid) — all within the <500 ms constitutional budget. Caveat: **cold-boot** on qwen3 re-embeds the whole corpus (~7 min for 262 skills) and `/health` flips healthy before the snapshot finishes; `find_skill` can hang until embed activity drains. Follow-up: load precomputed vectors at boot ([[qwen3-default-operational-findings]]).

---

## 6. Efficacy instrumentation handoff (#205/#218 → T14/T15)

The efficacy harness can attribute every retrieval call to a concrete arm and corpus:

- **Active arm:** `embedding_model_metadata` (`key='active'`, written per rebuild, #228) + the `/health` `embedding_arm` component (`model=/dim=/collection=`, #239) + the `retrieval_backend` component (T06). A run is attributable to `(embedding_model, collection, graph_version, backend)`.
- **Per-call signal:** `find_skill`/`search_skill_graph` return `score` (relevance), `fusion_rank_score`, `rationale` (rrf/semantic/subunit/lexical components), `retrieval_context`, and (graph surface) `neighbors`/`conflicts`/`latency_ms`. This is enough to log hits, misses, graph evidence, and latency without server-side instrumentation changes.
- **Harness honesty gates** (#229/#230): `--require-dimension`/`--gate` fail loud on a null dimension; `OLLAMA_HOST` is SSRF-validated; `QDRANT_COLLECTION` is charset-guarded. Measurement never silently ships an unattributable arm.
- **DimensionMismatch** is fatal **only when Qdrant is reachable at boot** (#235); the offline window is observable but not fully closed (relay re-`ensure_collection` on reconnect is documented remaining work) — do not overstate it as always-fatal.
- **Model-keyed collections** are `skills__<slug>`, not `"skills"` (#234/#236), via a `Result`-returning, charset-guarded derivation.

**Hard prerequisite for honest efficacy:** T14/T15 must measure against a **corpus-aligned eval set** (the T11 deliverable). Until then, A/B efficacy can compare layer-ON vs layer-OFF task outcomes (#205) but cannot cite a retrieval MRR/nDCG number for the dogfood corpus.

---

## 7. Phase A exit summary

| Ticket | Outcome |
|---|---|
| T01 measurement harness | ✅ arms + gates |
| T02 qwen3 embedder + rebuild safety | ✅ default qwen3, model-keyed collections |
| T03 multi-view fields | ✅ 7 fields write-ahead + read |
| T04 hybrid candidate gen | ✅ shipped behind selector; **measured no uplift**; snapshot_dense stays default |
| T05 typed skill edges | ✅ shipped |
| T06 agent graph tools (#255/#260/#243) | ✅ shipped + **live-proven** |
| T07 reranker/decomposition | ⏭️ **skipped** (earns no cost given T04) |
| T08 contract docs + handoff | ✅ this document + reconciled `retrieval-contract.md` / `online-retrieval-cqrs.md` |
| T09 dense multi-view views | ✅ code; measured ON/OFF sweep → **T11** |

**Bottom line:** V1.7 ships a real, agent-callable, honestly-scored retrieval substrate on a local qwen3 arm with a typed skill graph. Its *efficacy* — whether the multi-view/hybrid bet beats plain dense, and whether 0.80 is reachable — is **not yet measured** and is gated entirely on T11 building a corpus-aligned eval. No part of V1.7 should be cited as efficacy-proven until that sweep runs.

---

## 8. T11 measured verdict (2026-06-11) — efficacy gate DISCHARGED

The §6 "hard prerequisite" (a corpus-aligned eval set) and the §7 T09 "deferred sweep" are now
**done**. Built `tests/fixtures/retrieval_quality_262_corpus_labeled.json` (137 positives + 25
negatives, anti-circularity: headline queries from the 24 sessions' problem statements / fresh-vocab
symptom paraphrases, gold mapped via `source_session_id`; `use_when` demoted to a labeled secondary
stratum). The α=0 instrument gate **passed at a 100% MRR crater** (p=0.0000) — the fixture genuinely
discriminates retrieval quality, not self-recall. All numbers drive the real mcp-server over HTTP,
judged by the real `claude` CLI; readiness gated on the honest T17 `/health`-200 signal.

**Frozen 0.80 aspiration: MET on the corpus-aligned fixture.** Judge-aug held-out (n=53):
`snapshot_dense` 0.884 MRR / 0.804 nDCG@3 / 0.92 no-match; `dense_views_on` 0.912 / 0.839 / 0.92.
Previously un-validatable on the dogfood corpus (234 fixture 0/30 aligned) — now validated, not faked.

**Hybrid bet — split, earned verdict:**
- SPARSE / BM25 (`snapshot_hybrid`): **FALSIFIED** — net-negative (MRR 0.686→0.522, gold-in-pool
  99→76, 0 queries improved, p=0.0000). Sparse candidate fusion displaces good dense candidates.
- `qdrant_hybrid`: **EXACTLY equivalent** to dense (137/137 ties, p=1.0) — CQRS break buys nothing; do
  not promote.
- DENSE multi-view (T09 `RETRIEVAL_DENSE_VIEWS`): **VALIDATED** — the real lever. Anchor-only MRR@3
  0.686→0.743, candidate-recall@50 0.723→0.796, nDCG@3 0.696→0.755 (sign p=0.0074); judge-aug held-out
  0.912/0.839/0.92; p95 369ms < 500ms. **Recommend promoting to default-ON** (pending owner-approved
  flag-default flip — behavior-changing default).

**Lever finding:** MRR@3 == MRR@10 for every arm → gold is top-3 or absent from top-10; arm
differences live entirely in **candidate-recall@limit**, not fine ranking. That is the metric to
track as the corpus scales (T14/T15). The 0.48 no-match floor is confirmed well-calibrated for qwen3.

**Tie gate:** dense ≡ qdrant_hybrid tie exactly → execution stopped at the gate; the env-gated
lexical-ranking Rust arm was **not built** (owner decision 2026-06-11; strong prior it would not win
since BM25-as-candidate already hurt). Full evidence: `tests/e2e/reports/t11/T11-VALIDATION-REPORT.md`.

The §7 T09 row should now read: ✅ code **+ measured (T11): validated uplift, recommend default-ON**.
