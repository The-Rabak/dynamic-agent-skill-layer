# Retired Retrieval Quality Fixtures

The following fixture files have been **retired** (2026-06-12, T20):

| Retired file | Reason |
|---|---|
| `retrieval_quality_labeled.json` | Pre-262 corpus; loaded the stale Rust synthetic harness (`tests/e2e/quality/labeled_corpus.rs`) that seeded a hand-crafted 10-skill corpus, NOT the real qwen3 dogfood corpus. Superseded. |
| `retrieval_quality_234_corpus_labeled.json` | Intermediate 234-skill corpus fixture; 0/30 queries aligned with the live 262-skill corpus (see T11 verdict). Superseded. |

## What to use instead

**Active fixture:** `tests/fixtures/retrieval_quality_262_corpus_labeled.json`
- 137 positives + 25 negatives, anchored to the real 262-skill qwen3 dogfood corpus.
- Built from 24 genuine project dev sessions; anti-circular (queries grounded in
  session problem statements, not `use_when` fields).
- Validated by T11 (2026-06-11): α=0 crater 100%, dense_views MRR@3 0.743, cand-recall@50 0.796.

**Active gate:** `python3 scripts/retrieval_sweep.py --gate --run-id <timestamp>`
- Drives the REAL running mcp-server over HTTP on the 262 fixture.
- Asserts `GATE_THRESHOLDS` floors (MRR@3 ≥ 0.64, cand-recall@50 ≥ 0.68, nDCG@3 ≥ 0.64,
  no_match ≥ 0.88) and the α=0 crater canary (≥50% relative MRR drop).
- Emits `tests/e2e/reports/retrieval/gate_<run-id>.json` + `latency_<run-id>.json`.

**Rust shim:** `cargo test -p mcp-server --test test_retrieval_quality_gate -- --ignored`
- Shells out to the Python gate above; asserts exit 0.

The superseded Rust synthetic harness (`tests/e2e/test_retrieval_quality.rs`,
`tests/e2e/quality/labeled_corpus.rs`, `tests/e2e/quality/metrics.rs`) has been deleted.
It seeded a synthetic corpus and measured `compile_context` — incompatible with the
262-fixture schema (anchor/strata/split) and unable to consume the real qwen3 embeddings.
