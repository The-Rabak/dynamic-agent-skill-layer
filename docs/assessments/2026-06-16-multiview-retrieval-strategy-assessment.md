# Multi-View Retrieval Strategy Assessment

Date: 2026-06-16
Branch context: feat/v-1-7 working tree

## Executive Verdict

The evidence does not support "multi-view is inert" as a global claim.

The corrected claim is narrower:

1. Query-side priming segmentation is inert on the 4b production arm for the current 22-query SessionStart stratum. Coverage is flat at 0.080499 for segment caps 1, 2, 3, and 8 while p95 latency rises from 564 ms to 1287 ms.
2. Query-side priming segmentation is not inert on the 0.6b/TEI arm. Coverage improves from 0.079741 at cap 1 to 0.093377 at cap 4/8, although the gain is small and below the adoption bar.
3. Skill-side dense views are real and have shown a measured 4b win in T11 returned rankings: MRR@3 0.686 -> 0.743, nDCG@3 0.696 -> 0.755, sign-test p = 0.0074. That finding should be preserved.
4. The T11 "candidate_recall@50" wording is suspect. `find_skill` invokes the retriever before applying the request `limit`, and retriever Task mode caps returned skills at `RETRIEVAL_MAX_RESULTS`/`config.max_results`, default 3. Unless the live server had `RETRIEVAL_MAX_RESULTS=50`, the persisted `candidate_recall@50` metric is measuring recall over the returned top set, not a true deep candidate pool.

My conclusion: extraction quality is not the leading suspect. The stronger explanations are candidate-generation architecture, corpus/query saturation, and model-specific embedding behavior. Dense views can help ranking when the candidate is already in play, but the default snapshot path does not prove that view fields can independently recall skills whose summaries miss.

## Evidence Anchors

### Query-side priming segmentation

Code path:

- `crates/retrieval/src/query_segments.rs:29` defines `DEFAULT_MAX_SEGMENTS = 8`.
- `crates/retrieval/src/query_segments.rs:41` implements deterministic prompt segmentation.
- `crates/retrieval/src/orchestrator.rs:1250` enters the Priming branch.
- `crates/retrieval/src/orchestrator.rs:1270` segments the prompt.
- `crates/retrieval/src/orchestrator.rs:1281` embeds all segments.
- `crates/retrieval/src/orchestrator.rs:1318` merges segment passes by max score.

Measured result:

- 4b artifacts: `tests/e2e/reports/retrieval/t12_priming_segcap1.json`, `segcap2`, `segcap3`, `t12_priming_default.json`.
- Coverage is identical to six decimals: 0.080499 at caps 1/2/3/8.
- p95 latency rises: 564 ms, 766 ms, 1067 ms, 1287 ms.
- 0.6b/TEI artifacts: `t12_priming_a3_0p6b_tei_seg1.json`, `seg4`, `seg8`.
- Coverage improves: 0.079741 -> 0.093377 and plateaus at cap 4.

Interpretation:

The production 4b model already captures the useful match in the first/full prompt segment for this benchmark. Extra segment embeddings do not change selected coverage. The weaker 0.6b arm sometimes benefits from later segments, which argues against a generic logic bug in segmentation.

### Skill-side dense views

Code path:

- `crates/retrieval/src/dense_views.rs:114` builds `e_task` from `use_when`, procedure text, artifacts, and tools.
- `crates/retrieval/src/dense_views.rs:190` fuses summary/task/needs views by max positive cosine.
- `crates/retrieval/src/dual_scope.rs:422` reads the dense-view gate.
- `crates/retrieval/src/dual_scope.rs:436` and `crates/retrieval/src/dual_scope.rs:635` apply dense-view fusion in scoring.
- `crates/retrieval/src/orchestrator.rs:427`, `:547`, and `:648` define, default, and env-wire `RETRIEVAL_DENSE_VIEWS`.
- `crates/mcp-server/src/lib.rs:1664` loads per-view embeddings from Postgres.
- `crates/mcp-server/src/lib.rs:1821` builds dense view fields.
- `crates/mcp-server/src/lib.rs:1871` attaches dense metadata to skills.

Measured result:

- `tests/e2e/reports/t11/T11-VALIDATION-REPORT.md` reports the 4b dense-view win.
- `tests/e2e/reports/t11/sweep_matrix.json` persists MRR@3 0.686131 -> 0.743309 and no_match precision held at 0.92.
- The local metric self-test passed: `python3 scripts/retrieval_metrics.py --self-test`.

Important architecture limit:

In snapshot dense mode, initial candidate generation ranks by `e_summary` embedding first. Dense views are then used in `score_and_select_candidates`. That means dense views can improve scoring and thresholding for candidates already surfaced by summary dense search, but they do not necessarily create an independent view-level candidate pool.

That distinction matters. It explains why dense views can improve T11 top-result metrics while still failing to behave like a true recall expansion mechanism.

### Measurement caveat: candidate_recall@50

`crates/mcp-server/src/tools/find_skill.rs:104` calls:

```rust
retriever
    .retrieve(&request.query, RetrievalIntent::Task)
    .await
```

Only after that does it read:

```rust
let limit = request.limit.unwrap_or(5);
```

and truncate the already-returned `outcome.skills`.

Because Task retrieval itself uses `config.max_results` in `crates/retrieval/src/orchestrator.rs`, the script-level `limit=50` does not guarantee a 50-candidate pool. The raw artifacts also store `top3_names`, and positive rows have at most three returned names in the observed files.

This does not erase the dense-view win. It changes what was proven:

- Proven: dense views improved returned rankings/top-set hit behavior on 4b in T11.
- Not proven by current artifacts: dense views improved true deep candidate recall@50.

## Hypothesis Assessment

### Hypothesis 1: Extraction was bad

Assessment: unlikely as the primary cause.

Reasons:

- The DB evidence shows multi-view rows are materially populated: many view kinds per skill and hundreds of e_task/e_needs/e_negative rows.
- The 4b T11 dense-view win would be hard to get if the extracted fields were broadly useless.
- The code builds and caches the intended view families, and tests cover empty-view fallback and max positive fusion.

Residual concern:

The implementation quality is not perfect. `build_graph_from_pg` feeds `subunit_procedure_text` from subunit titles only, while dense-view docs describe richer subunit content. Blank `e_needs` and `e_negative` views are skipped. `e_negative` is embedded but excluded from positive fusion and currently does not act as a penalty. These are field-use limitations, not evidence that frontier extraction failed.

### Hypothesis 2: Multi-view fields are poorly populated or too redundant

Assessment: plausible in part, but not proven.

The fields are populated, but population is not the same as discriminative signal. The current evidence does not show:

- per-view text length distributions,
- summary-vs-task lexical overlap,
- gold-query term coverage by view,
- winning-view attribution,
- view-exclusive recall cases.

If e_task/e_needs mostly restate name/description/tags, dense fusion will often tie e_summary. That would look inert without implying broken extraction.

### Hypothesis 3: Corpus too small and too similar

Assessment: strong explanation.

The corpus is a compact dogfood skill corpus with many related engineering workflow skills. The SessionStart priming stratum has only 22 queries. On the 4b arm, the first prompt segment or summary embedding appears sufficient to find the same gold set, so extra query-side segments have no room to contribute. The 0.6b arm gaining from later segments supports the idea that this is model/corpus saturation, not a dead code path.

### Hypothesis 4: Architecture prevents dense views from contributing to recall

Assessment: strong and actionable.

In the current snapshot path, dense views are late fusion over an already-built candidate set. If a skill's summary misses the initial candidate pool, its e_task/e_needs vectors cannot rescue it unless another path, such as BM25, introduces it. This is the most important strategy issue: the system has dense view rescoring, not a fully view-indexed retrieval stage.

## Recommended Validation Plan

1. Fix the measurement harness before making more strategy calls.
   - Ensure `limit=50` actually retrieves 50 candidates, either by setting `RETRIEVAL_MAX_RESULTS=50` in evaluation runs or adding an explicit eval-only retriever limit override.
   - Split metrics into `pool_recall@50`, `returned_hit@3`, `MRR@3`, and `no_match_precision`.
   - Add a regression test that a `find_skill` evaluation request can return more than three candidates when asked.

2. Add attribution logs.
   - For skill-side dense views, log per-candidate `winning_view`, `e_summary_cos`, `e_task_cos`, `e_needs_cos`, and candidate source.
   - For query-side priming, log `winning_segment_idx` per selected skill.
   - This directly answers whether later segments or extended fields ever win.

3. Run a true view-level candidate-generation experiment.
   - Rank every `(skill_id, view_kind)` vector, max-pool by skill, then apply existing Eq3 scoring.
   - Compare summary-only pool, dense-view pool, BM25-expanded pool, and hybrid pool.
   - This is the test that actually validates or falsifies extended fields as recall contributors.

4. Run destructive ablations.
   - Blank e_task/e_needs and rerun.
   - Shuffle e_task/e_needs across skills and rerun.
   - Mask summary terms for a labeled subset where view fields should carry the match.
   - If metrics do not move under these ablations, the views are not materially used or are too redundant.

5. Audit field quality quantitatively.
   - Count nonblank fields by source and skill family.
   - Measure token overlap between e_summary and e_task/e_needs.
   - For gold pairs, measure whether query tokens appear only in extended fields.
   - Sample false positives and false negatives with view texts side-by-side.

6. Treat e_negative as a real signal.
   - The system stores `e_negative` but excludes it from positive fusion.
   - For no_match failures, test a penalty or veto feature based on negative-view similarity.
   - This is especially relevant because the 0.6b/TEI arm failed on no_match precision, not positive-task MRR.

7. Make production priming cheaper.
   - For the current 4b production arm, set `RETRIEVAL_PRIMING_MAX_SEGMENTS=1` unless there is a future-corpus risk reason to pay the latency.
   - If 0.6b/TEI remains under consideration, cap priming at 4. Cap 8 added no coverage over cap 4 in the observed curve.

## Bottom Line

Dense multi-view should not be removed. It has a validated 4b top-ranking benefit and the fields are materially populated.

But the current system and reports overstate what dense views have proven. The evidence proves late-fusion ranking help, not true extended-field candidate recall. To decide embedding and retrieval strategy, the next useful work is not another frontier extraction pass. It is a cleaner retrieval experiment with real pool sizes, view attribution, and a view-level candidate-generation arm.
