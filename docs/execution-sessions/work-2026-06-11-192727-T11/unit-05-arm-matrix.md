---
unit: "Full arm matrix sweep (dense vs hybrid, dense-views ON/OFF, MRR@3/@10)"
unit_number: 5
unit_kind: expansion
serves: "the earned dense-vs-hybrid + dense-multiview verdict"
status: completed
attempt_count: 3
domains: [measurement, retrieval]
session_id: work-2026-06-11-192727-T11
---

## Result (real mcp-server, anchor-only, all 137 positives + 25 negatives) — tests/e2e/reports/t11/sweep_matrix.json
| arm | MRR@3 | MRR@10 | nDCG@3 | hit@3 | cand_recall@50 | no_match | gold-in-pool |
|---|---|---|---|---|---|---|---|
| snapshot_dense (baseline) | 0.686 | 0.686 | 0.696 | 0.723 | 0.723 | 0.92 | 99/137 |
| snapshot_hybrid (BM25)    | 0.522 | 0.522 | 0.530 | 0.555 | 0.555 | 0.92 | 76/137 |
| qdrant_hybrid             | 0.686 | 0.686 | 0.696 | 0.723 | 0.723 | 0.92 | 99/137 |
| dense_views_on (T09)      | 0.743 | 0.743 | 0.755 | 0.788 | 0.796 | 0.92 | 108/137 |

## Paired direction (independently recomputed from raw first_relevant_rank vectors)
- snapshot_hybrid vs dense: 0 better / 23 worse / 114 tie; lost 23 golds from pool (99→76). **BM25 sparse candidate fusion strictly HURTS.** sign p=0.0000.
- qdrant_hybrid vs dense: 0 / 0 / 137 tie; identical gold-found (99); **byte-identical ranking** (Qdrant read path == in-memory dense). p=1.0000.
- dense_views_on vs dense: 13 better / 2 worse / 122 tie; recovered 9 golds (99→108). **Real significant uplift.** sign p=0.0074. Gains concentrate in headline strata: transcript 8-0, disjoint 3-2, lexical 1-0, use_when 1-0, multiview 0-0.

## VERDICT (anchor-only; judge-augmented pass pending for the 0.80 aspiration)
- The SPARSE/BM25 "hybrid bet" [[hybrid-is-the-retrieval-bet]] is FALSIFIED on the rich corpus — BM25 candidate fusion loses candidate recall.
- qdrant_hybrid is EXACTLY equivalent to snapshot_dense (no gain) → its CQRS break is not worth promoting.
- The DENSE multi-view bet (T09 RETRIEVAL_DENSE_VIEWS) is VALIDATED: +0.057 MRR@3, +0.073 cand_recall@50, +0.059 nDCG@3, sign-significant. The multi-view content pays off — through DENSE views (e_task/e_needs max-over-views), not sparse BM25.
- MRR@3==MRR@10 for ALL arms → gold is top-3 or missed-entirely; misses are POOL-RECALL failures, not ranking-order. Candidate-recall is the lever, not fine ranking. The MRR@10 resolution arm shows no rank-4..10 near-miss population.
- TIE GATE: dense ≡ qdrant_hybrid tie EXACTLY (137 ties). Per owner decision 2026-06-11, STOP — do NOT build the Rust lexical-ranking arm (BM25-as-candidate-gen already HURT, strong prior it would not help as a ranking term either; deferred to separate owner decision).

## Floor recalibration (scope item 8)
- Top-1 eq.3 score dist (#260): dense min 0.581 / median 0.747 / max 0.93; dense_views min 0.63 / median 0.776. Current 0.48 floor is BELOW the weakest real top-1 (0.581) → never rejects a real top match; no_match 0.92 ≥ 0.90 target. The "compressed ~0.016" alarm was the RRF fusion_rank_score artifact, not eq.3 — resolved by #260. Keep 0.48 (optional tighten to ~0.55).

## Harness bugs fixed live (orchestrator)
1. reboot_arm internal wait_ready used a hardcoded 234-corpus warmup prompt that matches nothing in 262 → monkeypatched _sweep.wait_ready to the T17 /health-200 gate.
2. reboot_arm derived hybrid collection name from OLLAMA_EMBED_MODEL defaulting to nomic → pinned OLLAMA_EMBED_MODEL=qwen3-embedding:4b in every arm (graph-builder built skills__qwen3-embedding-4b__hybrid=262pts; poll had watched nonexistent nomic name).
3. t11_metrics.paired_rank_diffs n_a_better/n_b_better labels are swapped vs mean_delta/aggregate (cosmetic; p-values correct). TODO note for the report.
