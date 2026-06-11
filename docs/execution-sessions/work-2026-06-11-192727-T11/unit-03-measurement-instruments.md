---
unit: "Extend harness: alpha=0 control, candidate-recall, MRR@10, paired diffs"
unit_number: 3
unit_kind: infra-packet
serves: "give the sweep eyes — candidate-recall, paired sign test, alpha=0 crater check"
status: completed
attempt_count: 1
domains: [measurement, python]
session_id: work-2026-06-11-192727-T11
---

## What Was Implemented (execution-agent ad4f4cb, sonnet)
- `scripts/t11_metrics.py`: pure functions (mrr_at_k, ndcg_at_k, hit/recall/precision, candidate_recall_at_limit, paired_rank_diffs, sign_test [exact binomial via math.comb], histogram, crater_check) + `--self-test` (34 synthetic cases).
- `scripts/t11_sweep.py`: live orchestrator. Reuses retrieval_quality_sweep reboot lifecycle; readiness = poll /health for HTTP 200 (T17 honesty, NOT warmup-query); captures find_skill name + #260 score (fail-loud on None); per-arm MRR@3/@10, nDCG@3, hit@3, recall@3, candidate-recall@50, per-query rank vectors, top-1 score histogram, no-match precision; paired_rank_diffs+sign_test vs snapshot_dense; crater_check for alpha0_control. Emits tests/e2e/reports/t11/sweep_<run-id>.json.

## Orchestrator validation (independent)
- `python3 scripts/t11_metrics.py --self-test` → PASS (34/34). `t11_sweep.py --help` → OK.
- Verified `_sweep.set_env(overrides)` is called before each reboot; set_env covers RETRIEVAL_ALPHA + RETRIEVAL_DENSE_VIEWS + RETRIEVAL_BACKEND and clears prior-arm keys → env is correctly exported to the docker-compose subprocess (no silent fake-tie risk).
- CONFIGS: snapshot_dense{}, snapshot_hybrid, qdrant_hybrid, dense_views_on, alpha0_control. Note: dense_views_on/alpha0_control use reboot_arm (forces graph-builder rebuild) — wasteful but harmless; T17 cache keeps the re-embed fast (cache-hit reload).

## Test Results
- Command: `python3 scripts/t11_metrics.py --self-test && python3 scripts/t11_sweep.py --help`
- Result: PASS. Attempts: 1. Live sweep path unexecuted-by-design (orchestrator owns it).
