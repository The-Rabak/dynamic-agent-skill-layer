---
date: 2026-06-07
topic: retrieval-quality-234-corpus-measured (#210)
assessor: Claude (Opus 4.8), orchestrated
status: measured — target not met, gap documented
method: REAL running mcp-server over HTTP (find_skill) + real claude-sonnet LLM judge; NO in-process reconstruction
related_tickets: ["210", "208", "209", "220"]
related_memory:
  - measurement-drives-real-app-no-in-process-reconstruction
  - brutal-eval-2026-06-07-efficacy-and-fakes
---

# Retrieval quality on the real 234-corpus — measured, tuned, gated (#210)

## How this was measured (and a discarded mistake)

Every number here is produced by driving the **real running `mcp-server`** over HTTP
(`find_skill`, the documented task-time retrieval path that runs the production
`SkillRetriever::retrieve` path) against the real 234-skill corpus in `skill_layer_test`, and
judging the returned candidates with the **real `claude` CLI** (`claude --print --output-format
json --model claude-sonnet-4-6`, the same provider invocation the production extraction seam
uses). The retrieval logic runs 100% inside the real server; the judge runs 100% inside the real
model. Tooling: `scripts/retrieval_quality_live.py` (measure) + `scripts/retrieval_quality_sweep.py`
(reboot the real server per config via `RetrievalConfig::from_env` and measure each).

**A discarded mistake, recorded for honesty:** the first attempt built an *in-process* rig that
hand-rolled the retrieval path (synthetic single scope + manual fusion, not the real
`RetrievalOrchestrator`). It reported held-out MRR = **0.017** — a ~35× lie versus the real
server's **0.594**. It was deleted. All numbers below come from the real app. See the memory
`measurement-drives-real-app-no-in-process-reconstruction`.

## Ground truth (anchor + LLM-judge pooling)

The corpus is self-extracted from this project's own sessions, so it contains **dense clusters of
near-synonym process skills** — a query derived from one anchor is often answered equally well by
3–5 sibling skills. Single-anchor scoring therefore *understates* quality badly (anchor-only
held-out MRR is 0.233 while the system genuinely surfaces a relevant skill into the top-3 for 2/3
of queries). Ground truth is:

```
relevant_set(q) = { anchor(q) } ∪ { skill s : the real LLM judge marked (q, s) relevant }
```

MRR/recall credit the anchor or any judge-relevant sibling; precision/nDCG credit the judged set.
Queries are anchor-derived from skill content **without retriever access** (no flatter-bias), split
into **disjoint tuning (45) ∥ held-out (35)** sets (`tests/fixtures/retrieval_quality_234_corpus_labeled.json`).

## Committed target (FROZEN before the sweep — not lowered)

**Judge-augmented held-out: MRR ≥ 0.80, nDCG@3 ≥ 0.80, no_match precision ≥ 0.90.**

## Baseline (default config: α=0.45 β=0.35 γ=0.20 λ=0.25)

| split | MRR | nDCG@3 | P@1 | hit@3 | no_match precision |
|---|---|---|---|---|---|
| held-out (anchor-only) | 0.233 | 0.233 | 0.233 | 0.233 | — |
| **held-out (judge-augmented)** | **0.594** | 0.484 | 0.533 | 0.667 | 0.800 |

## Tuning sweep (judge-augmented MRR on the tuning split)

Each config is a full reboot of the real server with `RETRIEVAL_*` overrides; the corpus re-embeds
at boot (deterministic `nomic-embed-text`, so configs are comparable).

| config | MRR | nDCG@3 | P@1 | hit@3 |
|---|---|---|---|---|
| default (λ=0.25) | 0.442 | 0.347 | 0.375 | 0.525 |
| **λ=0 (community boost OFF)** | **0.533** | **0.472** | 0.400 | **0.725** |
| α=0.40 β=0.45 γ=0.15 (beta-heavy) | 0.362 | 0.261 | 0.325 | 0.425 |
| α=0.60 β=0.30 γ=0.10 (alpha-heavy) | 0.354 | 0.256 | 0.300 | 0.425 |
| λ=0 + beta-heavy | 0.429 | 0.320 | 0.375 | 0.500 |
| subunit_deep (max_subunits=5, beta-heavy) | 0.362 | 0.266 | 0.300 | 0.450 |
| mmr_lambda=0.85 (relevance-favoring) | 0.450 | 0.366 | 0.375 | 0.550 |
| candidate_limit=100 | 0.392 | 0.316 | 0.325 | 0.475 |

**Winner: λ=0.** Validated on the disjoint held-out split (winner picked on tuning only):

| metric | default | **λ=0 (winner)** | target |
|---|---|---|---|
| MRR | 0.594 | **0.644** | 0.80 |
| nDCG@3 | 0.484 | **0.556** | 0.80 |
| P@1 | 0.533 | 0.567 | — |
| hit@3 | 0.667 | 0.733 | — |
| no_match precision | 0.800 | **1.000** | 0.90 |

## Findings

1. **The community boost (λ) is mildly HARMFUL, not merely decorative.** Turning it off improves
   MRR (+0.05 held-out), nDCG@3 (+0.072), *and* no_match precision (0.800 → 1.000). Because every
   skill belongs to ≥1 community, the uniform 1.05× boost was pushing community-member skills above
   the relevance floor for off-topic queries (fabricating no-match failures) and adding ranking
   noise. **This is decisive measured input for #208** (keep-or-cut): the binary boost loses to
   λ=0 on every metric. #208 should either replace it with a genuinely differentiating signal
   (query-to-community-centroid affinity) that *beats* λ=0, or formally cut HDBSCAN from the read
   path and bake λ=0. The α/β/γ levers stay at default (rebalancing measurably hurt).

2. **α/β/γ rebalancing does not help.** Both beta-heavy and alpha-heavy underperform default. The
   default 0.45/0.35/0.20 split is near-optimal; the quality ceiling is **not** in the weight blend.

3. **The ceiling is the embedding model + corpus density, not the scoring formula.** With
   `nomic-embed-text` (whose cosine similarities are known-inflated — see the floor calibration
   comment in `orchestrator.rs`) over a synonym-dense corpus, the best reachable judge-augmented
   held-out MRR is ~0.64. The system surfaces a relevant skill into the top-3 for **73%** of
   held-out queries (hit@3 0.733), but ranking the *best* one at position 1 (P@1 0.567) is where it
   falls short of the 0.80 bar.

## Gap vs target, and the next architectural bets (target NOT lowered)

**UPDATE after #208 (boost off) + #209 (floor 0.48):** the combined committed default now measures
**held-out judge-augmented MRR 0.767, nDCG@3 0.749, hit@3 0.867, no_match precision 1.000** — within
**0.033 of the 0.80 MRR aspiration and 0.051 of nDCG@3**, up from the 0.644 baseline. The floor (#209)
was the dominant lever; the #210 weight-sweep had not varied it. The frozen 0.80 target is **not yet
met but is now close**, and no_match precision is solved. Ranked next bets to close the last ~0.03–0.05
(each measured on this corpus via the same real-server rig before shipping):

1. **Stronger retrieval embedding model.** `nomic-embed-text`'s inflated, low-separation cosine is
   the most likely dominant bottleneck (it also forces the razor-thin #209 floor). Swapping to a
   higher-quality retrieval embedder is the highest-leverage bet. **Measure first.**
2. **LLM / cross-encoder re-ranking of the top-N.** The real judge cleanly separates relevant from
   irrelevant among the returned candidates (it lifts MRR from 0.233 anchor-only to 0.594+). An
   in-pipeline re-ranker over the top-N would operationalize that separation at query time — most
   directly attacks the P@1 0.567 weakness.
3. **Richer ℓ₁ embedding text.** The skill vector is `name + description + tags`; including the
   `## Procedures` body may sharpen discrimination among sibling process skills.
4. **#220 priming/recurrence** (deferred to Phase 4) for the session-start thin-prompt case.

## #209 — no_match floor recalibration (the biggest single lever)

The floor was the lever the #210 weight-sweep didn't touch — and it turned out to be the largest.
Recalibrated by sweeping `RETRIEVAL_RELEVANCE_THRESHOLD` on the real server (40 tuning positives +
20 off-topic negatives), calibrate-on-tuning → validate-on-held-out:

| floor | no_match precision | pos hit@3 | pos MRR (tuning, judge-aug) |
|---|---|---|---|
| 0.45 (old) | 0.600 | 0.725 | 0.533 |
| 0.46 | 0.800 | 0.675 | 0.596 |
| **0.48 (chosen)** | **1.000** | **0.800** | 0.662 |
| 0.50 | 1.000 | 0.750 | 0.683 |
| 0.52 | 1.000 | 0.725 | 0.725 |

**The old 0.450 was miscalibrated too low** (calibrated on 8 skills). On the real corpus it leaked
off-topic fabrications (no_match precision 0.600) and admitted mediocre-eq3 skills that displaced
better ones in the returned top-k. Raising the floor to **0.48** improves *both* negative rejection
(→1.000) and positive ranking — it is not the usual precision/recall tradeoff; removing low-score
noise cleans the top-k. **Held-out validation at 0.48 (disjoint): no_match precision 1.000, MRR
0.767, nDCG@3 0.749, hit@3 0.867, recall@3 0.808.** Floor baked to 0.48 in `RetrievalConfig::default()`.

## #208 — community graph keep-or-cut decision (measured, then CUT from ranking)

The three eq.3 community-boost arms, measured on the real server (held-out, judge-augmented), via
`scripts/measure_208_community_boost.py` (reboots the real server per arm with
`RETRIEVAL_COMMUNITY_BOOST_MODE`):

| arm | MRR | nDCG@3 | P@1 | hit@3 | no_match precision |
|---|---|---|---|---|---|
| (a) binary (0.2 for any community member) | 0.594 | 0.484 | 0.533 | 0.667 | 0.800 |
| (b) centroid_affinity (cosine(query, community centroid)) | 0.667 | 0.530 | **0.633** | 0.700 | 0.600 |
| (c) **off** (no community boost) | 0.644 | **0.556** | 0.567 | **0.733** | **1.000** |

**Decision: CUT — the community graph is demoted from ranking; default `community_boost_mode = Off`.**
Rationale: (a) binary is inert and mildly harmful (confirmed in the #210 sweep). (b) centroid_affinity
is a genuinely differentiating signal (it moved MRR off binary), but its only edge over (c) is +0.022
MRR / +0.066 P@1 — **within noise** on a 30-query held-out set (one query ≈ 0.033 RR) — and it is bought
with a **catastrophic no_match regression (1.000 → 0.600)**: the boost inflates off-topic skills over the
relevance floor, fabricating context for 40% of negative queries. (b) is also *lower* on nDCG@3. (c) Off
is robust-best on nDCG@3, hit@3, and no_match precision. The community boost does not earn its place in
ranking under the current embedding model. HDBSCAN communities remain a build-time
organizational/diagnostic artifact; `CentroidAffinity` is retained behind the env flag for re-evaluation
under a stronger embedder. The README "retrieval rides on the graph" claim is corrected accordingly.

Side effect: flipping the default from binary (MRR 0.594, fails the 0.60 regression floor) to off
(MRR 0.644, no_match 1.000) brings the default config above the #210 regression floor.

## Release gate (regression floor + documented aspiration)

`scripts/retrieval_quality_live.py --split held_out --gate --regression-floor 0.60` runs against the
real server. The **hard gate is a regression floor** (judge-augmented held-out MRR ≥ 0.60, no_match
precision ≥ 0.90) that fails loudly only on a real backslide below today's measured level — so the
e2e suite keeps its signal for everything else rather than being permanently red. The **0.80/0.80
target remains the documented aspiration**, printed by the gate as `UNMET (tracked in
docs/assessments/)` and tracked here — it is **not lowered and not faked green**. When a next-bet
(better embedder / re-ranker) lifts quality, raise the regression floor toward 0.80.
