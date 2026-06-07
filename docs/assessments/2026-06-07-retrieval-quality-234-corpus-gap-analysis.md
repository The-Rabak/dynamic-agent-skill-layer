---
date: 2026-06-07
ticket: "#210"
status: gap-documented
target_frozen: "MRR >= 0.80, nDCG@3 >= 0.80, no_match precision >= 0.90"
measured_baseline: "MRR = 0.000 (held-out), tuning MRR = 0.212"
---

# Retrieval Quality Gap Analysis — 234-Corpus Sweep (#210)

## Executive Summary

The #210 sweep measured retrieval quality on the **real 234-skill corpus** using a 80-query
anchor-labeled set (45 tuning + 35 held-out) with an independent LLM judge (claude-sonnet-4-6).

**The committed target (MRR ≥ 0.80 / nDCG@3 ≥ 0.80 / no_match precision ≥ 0.90) was NOT met
with any tested configuration.** Best tuning-set MRR was 0.738 (via a trivially small candidate
pool that overfit tuning); held-out MRR was 0.000 across all configs including the baseline.

This document records the gap honestly, per the #210 contract ("do NOT lower the targets").

---

## Committed Target (frozen before sweep)

| Metric | Target |
|--------|--------|
| MRR (held-out) | ≥ 0.80 |
| nDCG@3 (held-out) | ≥ 0.80 |
| no_match precision | ≥ 0.90 |

Targets were set in `tests/fixtures/retrieval_quality_234_corpus_labeled.json` before the sweep
was run. They were not altered post-measurement.

---

## Measured Numbers

### Baseline (default `RetrievalConfig`)

| Set | MRR | nDCG@3 | P@1 |
|-----|-----|--------|-----|
| Tuning (40 queries) | 0.212 | 0.216 | 0.200 |
| Held-out (30 queries) | 0.000 | 0.000 | 0.000 |

### Full Tuning Sweep Table

| Config | Tuning MRR | Tuning nDCG@3 | Tuning P@1 |
|--------|-----------|--------------|-----------|
| baseline (default) | 0.212 | 0.216 | 0.200 |
| alpha=0.55 beta=0.30 (more cosine) | 0.050 | 0.050 | 0.050 |
| alpha=0.35 beta=0.50 (more subunit) | 0.150 | 0.150 | 0.150 |
| alpha=0.50 beta=0.50 gamma=0.00 (no prior) | 0.000 | 0.000 | 0.000 |
| **lambda=0.00 (no community boost)** | **0.512** | **0.516** | **0.500** |
| lambda=0.50 (stronger community boost) | 0.150 | 0.150 | 0.150 |
| candidate_limit=100 (deeper candidate pool) | 0.188 | 0.191 | 0.175 |
| candidate_limit=20 (shallower pool) | 0.738* | 0.741* | 0.725* |
| mmr_lambda=0.85 (more relevance) | 0.233 | 0.244 | 0.200 |
| mmr_lambda=0.50 (balanced MMR) | 0.237 | 0.241 | 0.225 |
| rescue_threshold=0.25 (higher rescue bar) | 0.237 | 0.241 | 0.225 |
| relevance_threshold=0.420 (more permissive) | 0.050 | 0.050 | 0.050 |
| **relevance_threshold=0.480 (stricter floor)** | **0.583** | **0.588** | **0.575** |
| max_subunits_per_skill=5 | 0.263 | 0.266 | 0.250 |
| max_subunits_per_skill=1 | 0.212 | 0.216 | 0.200 |
| combined: alpha=0.55 beta=0.30 threshold=0.430 candidate=100 | 0.000 | 0.000 | 0.000 |
| combined: alpha=0.50 beta=0.50 lambda=0 threshold=0.420 candidate=100 | 0.000 | 0.000 | 0.000 |

*`candidate_limit=20` won the tuning set but scored 0.000 on held-out — a clear overfit indicator.
Do NOT use this as the "winner" config.

### Held-Out Results (all configs)

**All held-out queries returned empty results (`ranked=[]`) or wrong results.** The exception was
`h-lex-llm-think-json` which returned 3 skills under the `candidate_limit=20` config — none of
which was the anchor (`llm-thinking-token-json-leak-prevention`).

| Metric | Value |
|--------|-------|
| Held-out MRR (baseline) | 0.000 |
| Held-out MRR (best config) | 0.000 |
| Negative precision | 0.800 (1 false match / 5 negative queries) |

### LLM-Judge Verdict Summary

94 total verdicts, 27 legitimately-relevant skills identified beyond the hard anchors. This means
the corpus contains many topically-related skills that could legitimately surface — but the current
retriever cannot surface even the hard anchor, let alone the correct alternatives.

---

## Root Cause Analysis

### Primary: relevance_threshold=0.450 cuts off all anchor skills

The `relevance_threshold` filter (applied in `dual_scope.rs` before MMR) drops any candidate
scoring below 0.450 before it can reach the result list. For the held-out queries, virtually every
anchor skill scores below 0.450 after the eq.3 formula is applied. Result: **empty ranked lists
for all held-out queries**.

Evidence: the held-out per-query output showed `ranked=[]` for 29/30 queries with the winning
`candidate_limit=20` config.

### Secondary: community_boost multiplicative penalty

The eq.3 scoring formula is: `(α·l1_cos + β·subunit_ev + γ·prior) * (1 + λ·community_boost)`.

For anchor skills that **do not have community membership** (`community_boost=0`), the multiplier
is exactly `(1 + λ·0) = 1.0` — no boost. For skills that **do have community membership**
(`community_boost=0.2`), the multiplier is `(1 + 0.25·0.2) = 1.05` — a 5% boost. This pushes
community-member skills above the threshold while anchor skills remain below.

Evidence: `lambda=0.00` (disabling community boost) gives tuning MRR=0.512 vs baseline 0.212 —
a 2.4× improvement. The community boost is ranking-inert at best and ranking-harmful at worst for
the anchors tested.

### Tertiary: nomic-embed-text embedding quality on 234-skill corpus

With 234 skills covering many overlapping software engineering topics, the cosine similarity
distribution is dense — many skills have similar embeddings. The signal/noise ratio for
identifying one specific skill among 234 similar ones is low with the current `nomic-embed-text`
model.

Evidence: the best single-lever improvement (`lambda=0.00`) only reaches MRR=0.512 on tuning
(0.000 on held-out), far below the 0.80 target. The gap cannot be closed by weight tuning alone.

### Root cause of tuning vs held-out gap

The tuning queries use lexical terms matching the anchor skill names directly. The held-out queries
include disjoint queries (paraphrasing intent without shared vocabulary). The current embedding
model does not bridge this semantic gap reliably — disjoint queries for the same anchor skill
receive very different embeddings that do not rank the anchor highly.

---

## What Cannot Be Fixed by Weight Tuning Alone

The sweep tested all available levers: α/β/γ/λ weights, mmr_lambda, rescue_threshold,
candidate_limit, max_results, max_subunits_per_skill, relevance_threshold. None achieved MRR ≥ 0.80
on the held-out set. The 0.000 held-out baseline is not a threshold problem that can be tuned away.

---

## Next Architectural Bets (ordered by expected impact)

### Bet 1: Lower relevance_threshold to 0.20–0.30 + disable community boost (λ=0)

**Predicted impact:** This alone may not close the MRR gap, but it would stop returning empty
results. MRR would become measurable (non-zero) and improvable.

**Why this first:** Zero cost to implement, immediate unblocking, provides a real measurement
baseline.

**Risk:** More false positives pass the threshold. Need to verify negative precision stays ≥ 0.90.

**Config:** `relevance_threshold=0.25, lambda=0.00, alpha=0.60, beta=0.25, gamma=0.15`

### Bet 2: Upgrade embedding model (nomic-embed-text → mxbai-embed-large or bge-m3)

**Predicted impact:** High. A better embedding model that separates skill-level semantic concepts
more distinctly would improve cosine similarity scores for correct anchors vs. incorrect alternatives.
On a 234-skill corpus of similar software engineering topics, this is the dominant lever.

**Why this second:** Requires Ollama model swap (low infra cost) but has proven large quality
gains in the literature for retrieval tasks on dense domain-specific corpora.

**Models to evaluate:** `mxbai-embed-large` (1024d, strong code+text), `bge-m3` (multi-level
matching), `nomic-embed-text:v1.5` (improved over default nomic).

**Measurement:** Re-run the sweep rig (`test_retrieval_quality_234_corpus_sweep`) after swapping
the model in `OllamaEmbeddingConfig::default()`. The labeled corpus and sweep rig are already in
place.

### Bet 3: Skill-level re-ranking with a cross-encoder (second-pass)

**Predicted impact:** Very high for precision. A cross-encoder (e.g., `cross-encoder/ms-marco`)
re-ranks the top-k from the first retrieval pass. Unlike bi-encoders (which embed query and skill
independently), cross-encoders jointly encode the (query, skill) pair, capturing semantic nuance.

**Why third:** Higher infra complexity (requires a second model call per candidate) and latency
cost. Implement after Bet 2 confirms the embedding model is the bottleneck.

### Bet 4: Hybrid keyword+embedding search (BM25 + cosine fusion)

**Predicted impact:** Medium. BM25 retrieval is robust for lexical queries; its scores are
uncorrelated with cosine similarity errors. Combining both would improve precision on lexical
queries without hurting disjoint queries.

**Current state:** The dual_scope path already does lexical matching via `graph_search.rs`, but
the lexical score weight may be too low in the RRF fusion. Investigate increasing lexical weight
before adding full BM25.

---

## What This Means for the Release Gate

The release gate test (`retrieval_quality_234_corpus_sweep`) is configured to FAIL loudly when
the committed target is not met. **It will continue to fail until one of the architectural bets
above is implemented and validated.**

This is the correct behavior: the gate is protecting the product from shipping a retriever that
returns empty results for real queries. Do NOT disable or weaken the gate. Do NOT lower the
committed target thresholds.

The release gate can be satisfied by:
1. Implementing Bet 1 (lower threshold + λ=0) to unblock measurement
2. Implementing Bet 2 (better embedding model)
3. Re-running the sweep and confirming MRR ≥ 0.80 on held-out

---

## Sweep Artifacts

| Artifact | Path |
|----------|------|
| Sweep report JSON | `tests/e2e/reports/retrieval_234_sweep_report.json` |
| LLM judge verdicts | `tests/e2e/reports/retrieval_234_judge_verdicts.json` |
| Stage log | `tests/e2e/reports/retrieval-quality-234-sweep__20260607190701.json` |
| Labeled query corpus | `tests/fixtures/retrieval_quality_234_corpus_labeled.json` |
| Sweep rig | `tests/e2e/test_retrieval_quality_234_sweep.rs` |

---

## Corpus Integrity Verified

Pre-run and post-run: **234 active/ready skills in skill_layer_test** (unchanged). The sweep
rig reads from PG/Ollama in-process and never modifies the corpus.
