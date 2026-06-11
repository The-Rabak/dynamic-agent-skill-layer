---
ticket_id: T11
title: Multi-view re-sweep on the rich corpus — validate the hybrid bet
kind: measurement
status: completed  # 2026-06-11 session work-2026-06-11-192727-T11 — see tests/e2e/reports/t11/T11-VALIDATION-REPORT.md
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Success Criteria (quality targets) + measurement mandate"
source_packet_ref: "promoted from todo #259 (P2)"
feature_home: "tests/e2e quality harness and scripts/retrieval_quality_* (+ crates/retrieval ONLY for the env-gated lexical-ranking measurement arm, conditional — see Scope)"
depends_on:
  - T10
  - T09
  - T06
dependency_type: hard
serves:
  - The earned verdict on whether hybrid beats dense once the corpus actually has multi-view content
  - A measurement instrument that can actually SEE arm differences (negative control, paired diagnostics, candidate-recall)
files:
  - scripts/retrieval_quality_live.py
  - scripts/retrieval_quality_sweep.py
  - scripts/compare_arms_top10.py
  - tests/fixtures/retrieval_quality_234_corpus_labeled.json  # INVALID for the 262-skill corpus — see Local Context
  - tests/e2e/reports/
  - crates/retrieval/src/  # conditional lexical-ranking arm only (env-gated, default OFF)
test_command: "real-server dense vs snapshot_hybrid vs qdrant_hybrid sweep on the T10 corpus + top-10 diff"
tdd_mode: ralph
---

# Multi-view re-sweep on the rich corpus — validate the hybrid bet

## Serves

On the current corpus, dense ≡ hybrid exactly (top-10 byte-identical, 0/30 diff) because 0/234 skills have any multi-view content. The "BM25 zero uplift → snapshot_dense default" verdict is therefore an artifact of empty fields, not proof. This ticket re-runs the dense-vs-hybrid comparison once T10 has populated multi-view content and T09 has built the dense multi-view views, to earn the real verdict and validate (or falsify) the [[hybrid-is-the-retrieval-bet]] decision.

## Scope

**Amended 2026-06-11 (owner-directed, from the midpoint assessment — `docs/assessments/2026-06-11-v1-7-midpoint-deep-grok-assessment.md`): T11 is an INSTRUMENT ticket first, a sweep second.** Root finding behind the flat 0.767-across-all-arms results: (a) candidate-generation backends share one ranking brain by design — BM25/Qdrant only expand the candidate pool, final ranking is always eq.3 over dense cosine (`crates/retrieval/src/dual_scope.rs:353-417`, `:580` — "Qdrant decides the candidate pool, the snapshot pipeline decides quality"), so at 262 skills with `candidate_limit=50` identical top-3 is expected BY CONSTRUCTION; (b) MRR@3 is quantized to {1, 0.5, 0.33, 0} and the prior fixture was top-rank-saturated (qwen3 moved nDCG 0.749→0.709 while MRR stayed pinned at 0.767 — the rank of the first relevant hit never moved on any query). A naive re-sweep would reproduce the tie and kill the [[hybrid-is-the-retrieval-bet]] decision for the wrong reason.

Ordered scope:

1. **Instrument validation gate (FIRST, before any arm comparison):** run a negative-control sweep on the real server with semantic scoring disabled (`alpha=0` via the existing env-tunable scoring weights — a real config of the real server, not a fake). The fixture is valid only if the control arm's MRR craters (≥50% relative drop). If it doesn't, the fixture cannot discriminate and must be rebuilt before any verdict is issued.
2. **Build the 262-corpus-aligned held-out fixture** under the anti-circularity rule (see Local Context — `use_when`-derived queries are demoted to a secondary stratum). Strata: lexical / paraphrase / intent-disjoint / negative, plus lexical-recall queries whose gold term lives ONLY in a multi-view field (`tools`/`artifacts`/`invariants`) — now satisfiable on the rich corpus. Target ≥100 positive queries.
3. **Re-sweep** `snapshot_dense` vs `snapshot_hybrid` (and `qdrant_hybrid`) on the T10 corpus, driving the real mcp-server over HTTP (no in-process rig). Measure BOTH halves: sparse/BM25 multi-view recall AND dense multi-view (T09) ON/OFF.
4. **Paired per-query diagnostics:** every sweep records per-query rank vectors per arm; verdicts come from paired differences (count of queries whose first-relevant rank changed + sign test), never from 3-decimal mean equality.
5. **Candidate-recall as a first-class metric:** report gold-in-pool rate (candidate recall@`candidate_limit`) per arm, separately from MRR — at this corpus size it is the ONLY signal candidate generation can move, and it is the number that predicts whether hybrid matters at 5k-skill scale.
6. **Resolution arm:** sweep MRR@10 (env `max_results=10` on the real server, measurement arm only) alongside the product metric MRR@3.
7. **Conditional lexical-ranking arm:** if paired diffs show dense ≡ hybrid identical rankings again, implement an env-gated lexical ranking term (δ·normalized-BM25 in eq.3, or per-scope RRF over dense+BM25 ranks) in `crates/retrieval`, default OFF, and re-sweep — the hybrid bet must be tested as a RANKING signal at least once before any final verdict on it.
8. **Floor + score-distribution recalibration:** re-sweep the 0.48 relevance floor on qwen3 score distributions; publish histograms in the report.
9. Record whether hybrid wins/ties/loses, and whether it reaches the plan's frozen aspiration (judge-aug MRR ≥ 0.80, nDCG@3 ≥ 0.80, no-match precision ≥ 0.90) or a documented positive delta over dense.

## Scope Fence

- No fabricated multi-view queries against empty fields (that was the #250 trap).
- Do not tune on held-out data; preserve the tuning/held-out split.
- Do not flip the production default without recording the measured delta and updating the retrieval contract doc (T08).
- The lexical-ranking arm (scope item 7) ships env-gated default-OFF only; promoting it to default is a separate owner decision with contract-doc update.
- Until T17 (boot-readiness honesty) lands, sweeps must gate on REAL readiness (a probe query returning non-degraded results), not on `/health` alone — qwen3 cold-boot re-embeds the corpus for ~7 min while `/health` already reports healthy; measuring inside that window corrupts both latency and quality numbers.

## Acceptance Criteria

- [x] **Instrument gate passed:** α=0 craters MRR 100% (p=0.0000) on the new fixture — recorded in §1 of the report BEFORE any arm verdict.
- [x] A meaningful fraction of the corpus carries populated multi-view fields (PG: tools 150, invariants 188, use_when 188 of 262).
- [x] Held-out set obeys the anti-circularity rule (headline strata = the 24 sessions' problem statements / fresh-vocab paraphrases; `use_when` demoted to a labeled secondary stratum) and contains `multiview` lexical-recall queries whose gold term lives only in a multi-view field. All 137 anchors live-resolve.
- [x] Fresh real-server sweep records hybrid win/tie/loss vs dense on BOTH sparse (snapshot_hybrid: LOSS) and dense multi-view (dense_views_on: WIN) signals, with per-query paired rank dumps + sign-test verdicts (snapshot_hybrid p=0.0000; qdrant_hybrid p=1.0; dense_views p=0.0074) — not mean equality.
- [x] Candidate-recall@candidate_limit reported per arm separately from MRR (dense 0.723, snapshot_hybrid 0.555, qdrant_hybrid 0.723, dense_views 0.796); the verdict weighs it as THE lever at this scale.
- [x] MRR@10 resolution arm reported alongside MRR@3 — finding: MRR@3==MRR@10 for all arms (no rank-4..10 near-miss population).
- [x] Tie gate hit (dense ≡ qdrant_hybrid, 137 ties). Per owner decision 2026-06-11, STOPPED at the gate — the env-gated lexical-ranking Rust arm was NOT built; deferred to a separate owner decision (BM25-as-candidate already hurt → strong prior it would not win as a ranking term). Recorded in §4/§5.
- [x] Relevance floor re-calibrated on qwen3 distributions; top-1 score histograms in §3/§4 (dense min 0.581/median 0.747/max 0.93). 0.48 floor confirmed well-calibrated.
- [x] Uplift is readable: #260 eq.3 `score` used throughout (the "compressed 0.016" was the RRF `fusion_rank_score` artifact, now separated).
- [x] Default-arm + lexical-retrieval-ROI decision annotated (§5): keep snapshot_dense backend, do not promote either hybrid; RECOMMEND RETRIEVAL_DENSE_VIEWS→default-ON. Retrieval contract doc + assessment updated with the measured delta (actual flag-default flip is a pending owner-approved change).

## Local Context

- WHY source: plan `## Success Criteria` + the measurement standing rule (drive the real server).
- Depends on T10 (rich corpus), T09 (dense multi-view views), and the T06-folded #260 score-exposure fix (so uplift is readable).
- This is the gate the plan sets before any final "lexical retrieval isn't worth pursuing" verdict feeds T14 (#205) / T15 (#218).

### T10→T11 Handoff: eval-set is INVALID for the new corpus (owner action required before authoring)

`tests/fixtures/retrieval_quality_234_corpus_labeled.json` **cannot be used as-is for T11.**
Every anchor skill ID in that fixture pointed at the old 234-skill corpus, which was wiped in full
when T10 rebuilt the corpus from genuine dev sessions (262 skills, new UUIDs throughout). Running
the sweep against the cold 262 corpus with the old fixture will silently yield 0 recall on every
query — the anchors resolve to nothing — producing a fabricated zero, not a real measurement.

**T11's first deliverable is therefore to regenerate a corpus-matched held-out fixture against
the live 262-skill corpus before running any sweep.**

The method for generating that eval set was an **open owner decision** (flagged in
`tests/e2e/reports/replica-run/VALIDATION-REPORT.md`, T10 follow-up #3). The previously-leading
candidate — derive held-out queries from each skill's `use_when` field — is **demoted by owner
decision 2026-06-11**: querying the corpus with text the corpus generated measures *self-recall*,
not retrieval (circularity). Skills' multi-view text and the queries derived from it share
phrasing by construction, so it inflates every arm and cannot rank them.

**Decided method (2026-06-11):** headline query strata are drawn from material the skill text was
NOT generated from — task descriptions / prompts in the 24 source session transcripts (the
sessions exist; the skills were extracted from their *resolutions*, queries come from their
*problem statements*), plus hand-labeled intent-disjoint paraphrases. `use_when`-derived queries
are permitted only as a clearly-labeled secondary stratum (useful for the dense-views ON/OFF
mechanical check), never the headline. Negatives stay adversarial (plausible but off-corpus).
The α=0 instrument gate (Scope item 1) is the arbiter: any fixture that fails it is rebuilt,
whatever its provenance.

## Source

Promoted 2026-06-09 from todo #259 (P2), itself filed from the #250 Option-1 live re-sweep. Full diagnostic history in git of `todos/259-*` (incl. the 2026-06-09 PATCH).

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
