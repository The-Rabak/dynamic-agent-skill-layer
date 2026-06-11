---
unit: "Build 262-aligned anti-circularity held-out fixture"
unit_number: 2
unit_kind: tracer-bullet
serves: "an instrument that can discriminate arms (vs the 0/30-aligned 234 fixture)"
status: completed
attempt_count: 2
domains: [measurement, fixture, llm-synthesis]
session_id: work-2026-06-11-192727-T11
---

## What Was Implemented (execution-agent ac64594, sonnet)
- `scripts/build_t11_fixture.py` (drives real claude CLI --model claude-sonnet-4-6 for query synthesis, batched).
- `tests/fixtures/retrieval_quality_262_corpus_labeled.json` — 162 queries: 36 transcript + 36 disjoint (headline) + 30 lexical + 20 multiview + 15 use_when (secondary) + 25 negatives. 137 positives, 45 distinct gold skills across all 24 sessions. Split 98 tuning / 64 held_out (disjoint by anchor).
- `tests/e2e/reports/t11/fixture_build_summary.json`.
- Filtered 262→188 usable skills (excluded type=None preference stubs). Anti-circularity: headline queries grounded in transcript problem statements / symptom paraphrases in fresh vocab; use_when demoted to labeled secondary stratum.

## Orchestrator validation (independent)
- Structural: kinds/splits/counts confirmed; 137 pos / 25 neg.
- LIVE corpus alignment: ALL 137 anchors + all relevant names resolve in the live PG 262-skill corpus (0 misses) — vs the old 234 fixture's 0/30. The instrument is corpus-aligned.
- Eyeballed strata: transcript = genuine developer phrasings; disjoint = fresh vocab; lexical = distinctive-term reuse; multiview = symptom-phrased; negatives = adversarial off-corpus (React/Flask/etc.). Anti-circularity honored.
- REAL arbiter is the α=0 instrument gate (unit 4).

## Test Results
- Command: structural+live-corpus validation; Result: PASS (OK pos 137 neg 25, 0 live misses). Attempts: 2 (first attempt batch-size-8 claude CLI timeout → batch 5 + 300s).
