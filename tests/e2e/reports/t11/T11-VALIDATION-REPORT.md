# T11 — Multi-view re-sweep: validate the hybrid bet (2026-06-11)

**Instrument-first re-sweep on the rich 262-skill qwen3 corpus.** Every number below was
produced by driving the **real running mcp-server over HTTP** (`find_skill`) on the live
T10 corpus, judged by the **real `claude` CLI** (`claude-sonnet-4-6`). No in-process
reconstruction, no fabricated values. (Standing rule: measurement drives the real app.)

- Corpus: 262 skills, qwen3-embedding:4b (2560-dim), `skill_layer_test` + `skills__qwen3-embedding-4b`.
  Multi-view populated: tools 150, invariants 188, use_when 188 (71% rich — the T10 corpus).
- Fixture: `tests/fixtures/retrieval_quality_262_corpus_labeled.json` — 137 positives + 25 negatives,
  45 distinct gold skills across all 24 source sessions. Strata: transcript 36, disjoint 36,
  lexical 30, multiview 20, use_when 15 (secondary), negatives 25. Anti-circularity: headline
  queries grounded in genuine session **problem statements** / fresh-vocabulary symptom
  paraphrases (NOT the skills' own `use_when`/description). Every anchor resolves in the live corpus.
- Readiness: every arm gated on the **honest T17 `/health`-200** signal (no warming-while-healthy
  measurement window).
- Reports: `sweep_gate.json` (α=0), `sweep_matrix.json` (4 arms anchor-only), `sweep_judged.json`
  (judge-augmented dense vs dense_views).

---

## 1. Instrument gate (AC#1) — PASSED

The fixture is valid only if zeroing the dense semantic signal craters MRR ≥50% relative.

| arm | MRR@3 | cand_recall@50 | no_match |
|---|---|---|---|
| snapshot_dense | 0.686 | 0.723 | 0.92 |
| **alpha0_control** (`RETRIEVAL_ALPHA=0`) | **0.000** | 0.000 | 1.0 |

**Relative MRR drop = 100% (≥50% required).** Paired sign test (α=0 vs dense): 99 queries
worse, 0 better, p=0.0000. The fixture measures real retrieval quality, not self-recall.

---

## 2. Arm matrix (anchor-only, all 137 positives) — `sweep_matrix.json`

| arm | MRR@3 | MRR@10 | nDCG@3 | hit@3 | **cand_recall@50** | gold-in-pool | no_match |
|---|---|---|---|---|---|---|---|
| **snapshot_dense** (baseline) | 0.686 | 0.686 | 0.696 | 0.723 | 0.723 | 99/137 | 0.92 |
| snapshot_hybrid (BM25) | 0.522 | 0.522 | 0.530 | 0.555 | **0.555** | 76/137 | 0.92 |
| qdrant_hybrid | 0.686 | 0.686 | 0.696 | 0.723 | 0.723 | 99/137 | 0.92 |
| **dense_views_on** (T09) | **0.743** | **0.743** | **0.755** | **0.788** | **0.796** | 108/137 | 0.92 |

**Paired per-query direction** (recomputed from raw first-relevant-rank vectors, authoritative):

| candidate vs snapshot_dense | better | worse | tie | gold-found Δ | sign-test p |
|---|---|---|---|---|---|
| snapshot_hybrid | 0 | 23 | 114 | 99→76 (−23) | 0.0000 |
| qdrant_hybrid | 0 | 0 | 137 | 99→99 (0) | 1.0000 |
| dense_views_on | 13 | 2 | 122 | 99→108 (+9) | 0.0074 |

---

## 3. Judge-augmented (the frozen-target metric) — `sweep_judged.json`

Frozen aspiration (plan): judge-aug **held-out** MRR ≥ 0.80, nDCG@3 ≥ 0.80, no-match ≥ 0.90.

| arm (judge-aug) | split | MRR | nDCG@3 | hit@3 | no_match | 0.80/0.80/0.90 |
|---|---|---|---|---|---|---|
| snapshot_dense | held_out (n=53) | 0.884 | 0.804 | 0.925 | 0.92 | **MET** |
| snapshot_dense | all (n=137) | 0.864 | 0.811 | 0.891 | 0.92 | MET |
| **dense_views_on** | held_out (n=53) | **0.912** | **0.839** | 0.962 | 0.92 | **MET (best)** |
| dense_views_on | all (n=137) | 0.911 | 0.863 | 0.942 | 0.92 | MET |

Judge-aug paired (dense_views vs dense): 10 better / 3 worse / 124 tie, mean Δ favors dense_views,
p=0.0923 (marginal — the judge rescues alternates that compress the gap). Aggregate uplift is
consistent: +0.047 MRR, +0.052 nDCG@3, +0.059 candidate-recall judged.

Latency (dense_views_on, 30 live queries): mean 292ms, p50 274ms, **p95 369ms < 500ms SLO ✓**.

---

## 4. Verdict

**The frozen 0.80 MRR / 0.80 nDCG@3 / 0.90 no-match aspiration is MET** on the corpus-aligned
262 fixture — by both the current default (held-out 0.884 / 0.804 / 0.92) and the dense-views arm
(0.912 / 0.839 / 0.92). This is the gate the plan set; it was previously un-validatable because the
234 fixture was 0/30 aligned with the dogfood corpus. **It is now validated, not faked.**

**On "is the hybrid bet real?" — split decision, earned:**

1. **The SPARSE / BM25 hybrid bet is FALSIFIED on the rich corpus.** `snapshot_hybrid` strictly
   *hurts*: it never improved a single query and pushed 23 gold skills out of the candidate pool
   (gold-in-pool 99→76, MRR 0.686→0.522, p=0.0000). BM25 candidate fusion injects lexical noise
   that displaces good dense candidates. This reproduces — for the right reason this time, on
   populated multi-view content — the T04 "no uplift from sparse" finding, and strengthens it to
   "sparse candidate fusion is net-negative here."

2. **`qdrant_hybrid` is EXACTLY equivalent to `snapshot_dense`** (137/137 ties, identical gold-found,
   p=1.0). The Qdrant read path reproduces the in-memory dense ranking byte-for-byte. Its CQRS
   break (read-path Qdrant dependency) buys **zero** retrieval gain → **do not promote**; keep it
   experimental, as T08 recorded.

3. **The DENSE multi-view bet (T09 `RETRIEVAL_DENSE_VIEWS`) is VALIDATED.** It is the real lever the
   multi-view corpus unlocks: +0.057 MRR@3, +0.073 candidate-recall@50, +0.059 nDCG@3 anchor-only
   (sign p=0.0074); +0.047 MRR judged; held-out judge-aug **0.912 / 0.839 / 0.92**, p95 369ms.
   Gains concentrate in the realistic headline strata (transcript 8-0, disjoint 3-2). It recovers
   golds into the candidate pool (99→108) that single-view dense missed.

**Candidate-recall, not fine ranking, is the lever at this scale.** MRR@3 == MRR@10 for *every*
arm — the first relevant hit is always in the top-3 or absent from the top-10 entirely; there is no
rank-4..10 near-miss population. So arm differences live entirely in *which golds reach the
candidate pool* (candidate-recall), exactly as the midpoint assessment predicted from the shared-
ranking-brain architecture. This is the number that will predict whether any candidate-gen change
matters at 5k-skill scale.

**Tie gate (scope item 7):** `dense` ≡ `qdrant_hybrid` tie exactly (137 ties). Per owner decision
(2026-06-11), execution **stops at the tie gate**: the env-gated lexical-ranking (δ·BM25 / RRF)
Rust arm was **not built**. The data gives a strong prior it would not help — BM25 as a *candidate*
signal already hurt, so a BM25 *ranking* term on this corpus is unlikely to win — but that remains a
separate, explicit owner decision. T11 stays measurement-only.

**Floor (scope item 8):** top-1 eq.3 scores (#260) are healthy and uncompressed — dense min 0.581 /
median 0.747 / max 0.93; dense_views min 0.63 / median 0.776. The 0.48 relevance floor sits *below*
the weakest real top-1 match (0.581), so it never rejects a real top result, and no-match precision
is 0.92 (≥0.90). The old "qwen3 scores compressed ~0.016" alarm was the **RRF `fusion_rank_score`
artifact**, not the eq.3 relevance score — resolved by #260. Keep 0.48 (optional tighten to ~0.55
to push no-match higher; not required).

---

## 5. Recommendations

- **PROMOTE `RETRIEVAL_DENSE_VIEWS` to default-ON** (currently default-OFF, T09). It is the one
  change the data clearly mandates: validated uplift, frozen aspiration met, p95 within SLO. This is
  a behavior-changing production default → owner-approved one-line flag-default flip + this report as
  evidence (left as an explicit decision, consistent with keeping T11 measurement-only). If flipped,
  update the T08 retrieval-contract doc (`docs/reference/retrieval-contract.md`) accordingly.
- **Keep `snapshot_dense` as the candidate-gen backend; do NOT promote either hybrid backend.**
  snapshot_hybrid hurts; qdrant_hybrid is equivalent and breaks CQRS for no gain.
- **The lexical-ranking arm is deferred** (tie-gate stop). Revisit only if a future, larger corpus
  shows a candidate-recall gap that dense multi-view cannot close.
- **For #218 / efficacy (T14/T15):** candidate-recall@limit is the metric to track as the corpus
  scales; MRR is saturated/quantized at this size.

## 6. Known instrument caveat

`scripts/t11_metrics.py:paired_rank_diffs` is correct; `t11_sweep.py` originally called it with
`(baseline, candidate)` while printing a `"candidate vs baseline"` label, so the **persisted
`n_a_better`/`n_b_better` in `sweep_matrix.json` / `sweep_judged.json` are reversed relative to that
label** (n_a_better there = baseline-better). The sign-test p-values are unaffected (two-sided). The
call was fixed for future runs; all paired directions in this report were independently recomputed
from the raw `first_relevant_rank` vectors and are authoritative.

**Instrument location (T20, 2026-06-12):** The metrics/sweep scripts have been promoted to a
ticket-agnostic home — `scripts/retrieval_metrics.py` / `scripts/retrieval_sweep.py` — as part of T20.
The historical names `t11_metrics.py` / `t11_sweep.py` no longer exist.
The gate mode (`--gate`) and the raw per-query latency artifact path are described in
`tests/fixtures/RETIRED_FIXTURES.md` (which also documents the fixture retirements).

## Erratum (2026-06-12, T20)

**§2 and §3 gold-in-pool count for `dense_views_on`:** the table in §2 and the paired direction
table both state 108/137 (+9 gold found vs snapshot_dense's 99/137).  The correct figure is
**109/137 (+10)**.  Derivation: dense_views_on anchor-only candidate_recall@50 = 0.7956 (§2);
0.7956 × 137 = 109.0 → **109 gold skills in the candidate pool**, not 108.  The rounding was an
error; the `candidate_recall_at_limit` metric value (0.7956 = 109/137) is correct and unchanged.
The sign-test (p=0.0074) and all verdict statements in §4 are unaffected.

**Latency artifact:** the raw per-query latency for the `dense_views_on` arm (the source of the
cited p95 369ms) will be persisted at `tests/e2e/reports/t11/latency_<run-id>.json` when the live
gate runs (`scripts/retrieval_sweep.py --gate`).  The orchestrator Unit B (T20 live run) populates
this artifact.  The format and path are defined in the `_run_gate` function of `retrieval_sweep.py`.
