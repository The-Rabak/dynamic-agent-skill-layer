---
ticket_id: T11
title: Multi-view re-sweep on the rich corpus — validate the hybrid bet
kind: measurement
status: blocked
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Success Criteria (quality targets) + measurement mandate"
source_packet_ref: "promoted from todo #259 (P2)"
feature_home: "tests/e2e quality harness and scripts/retrieval_quality_*"
depends_on:
  - T10
  - T09
  - T06
dependency_type: hard
serves:
  - The earned verdict on whether hybrid beats dense once the corpus actually has multi-view content
files:
  - scripts/retrieval_quality_live.py
  - scripts/retrieval_quality_sweep.py
  - scripts/compare_arms_top10.py
  - tests/fixtures/retrieval_quality_234_corpus_labeled.json
  - tests/e2e/reports/
test_command: "real-server dense vs snapshot_hybrid vs qdrant_hybrid sweep on the T10 corpus + top-10 diff"
tdd_mode: ralph
---

# Multi-view re-sweep on the rich corpus — validate the hybrid bet

## Serves

On the current corpus, dense ≡ hybrid exactly (top-10 byte-identical, 0/30 diff) because 0/234 skills have any multi-view content. The "BM25 zero uplift → snapshot_dense default" verdict is therefore an artifact of empty fields, not proof. This ticket re-runs the dense-vs-hybrid comparison once T10 has populated multi-view content and T09 has built the dense multi-view views, to earn the real verdict and validate (or falsify) the [[hybrid-is-the-retrieval-bet]] decision.

## Scope

- Author held-out lexical-recall queries whose gold term lives ONLY in a multi-view field (`tools`/`artifacts`/`invariants`) — now satisfiable on the rich corpus.
- Re-sweep `snapshot_dense` vs `snapshot_hybrid` (and `qdrant_hybrid`) on the T10 corpus, driving the real mcp-server over HTTP (no in-process rig).
- Measure BOTH halves: sparse/BM25 multi-view recall AND dense multi-view (T09) recall.
- Record whether hybrid wins/ties/loses, and whether it reaches the plan's frozen aspiration (judge-aug MRR ≥ 0.80, nDCG@3 ≥ 0.80, no-match precision ≥ 0.90) or a documented positive delta over dense.

## Scope Fence

- No fabricated multi-view queries against empty fields (that was the #250 trap).
- Do not tune on held-out data; preserve the tuning/held-out split.
- Do not flip the production default without recording the measured delta and updating the retrieval contract doc (T08).

## Acceptance Criteria

- [ ] A meaningful fraction of the corpus carries populated multi-view fields (PG count > 0).
- [ ] Held-out set contains lexical-recall queries whose gold term lives ONLY in a multi-view field.
- [ ] A fresh real-server sweep records hybrid win/tie/loss vs dense, on both sparse and dense multi-view signals.
- [ ] Uplift is readable: scores reflect relevance (depends on the #260 fix folded into T06), not the RRF rank artifact.
- [ ] The default-arm + lexical-retrieval-ROI decision is annotated with which case the data supports, and the retrieval contract doc is updated if the default flips.

## Local Context

- WHY source: plan `## Success Criteria` + the measurement standing rule (drive the real server).
- Depends on T10 (rich corpus), T09 (dense multi-view views), and the T06-folded #260 score-exposure fix (so uplift is readable).
- This is the gate the plan sets before any final "lexical retrieval isn't worth pursuing" verdict feeds T14 (#205) / T15 (#218).

## Source

Promoted 2026-06-09 from todo #259 (P2), itself filed from the #250 Option-1 live re-sweep. Full diagnostic history in git of `todos/259-*` (incl. the 2026-06-09 PATCH).

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
