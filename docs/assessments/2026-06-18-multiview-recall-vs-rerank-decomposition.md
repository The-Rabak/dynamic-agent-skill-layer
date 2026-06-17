# Multi-View Retrieval: Recall-vs-Re-rank Decomposition

Date: 2026-06-18
Branch: feat/v-1-7
Supersedes the open questions in `2026-06-16-multiview-retrieval-strategy-assessment.md`
(prior-session take; its point #4 — the `candidate_recall@50` depth suspicion — is **confirmed**
and resolved below).

## Executive verdict

The claim "multi-view extended fields aren't contributing" is **false** for the skill-side dense
views on the 4b production arm. Measured on the live `find_skill` path, end to end:

- **Extended views are a RECALL lever, not merely a re-rank lever.** With a truthful deep pool
  (`RETRIEVAL_MAX_RESULTS=50`), enabling dense views raises `candidate_recall@50` **0.810 → 0.883
  (+0.073)** — they pull gold skills into the candidate pool that the `e_summary` embedding alone
  never surfaces. They *also* re-rank within the pool (MRR@3 **0.684 → 0.741, +0.057**).
- **The signal concentrates on `transcript` queries** (real failure-narrative prompts — the same
  shape as the production session-start / priming input): recall **0.583 → 0.778 (+0.195)** and
  MRR@3 **0.472 → 0.648 (+0.176)**.
- This **contradicts the "corpus too self-similar" hypothesis**: if density drowned the views,
  recall would not move. Instead views add the *most* recall precisely where it is hardest
  (transcript, 0.58 baseline) and correctly add nothing where the summary already recalls
  everything (multiview 1.000, lexical 0.967 — no headroom).

Two distinct mechanisms were being conflated under "multi-view":

| Mechanism | Env | 4b verdict |
|---|---|---|
| **A. Query-side priming segmentation** | `RETRIEVAL_PRIMING_MAX_SEGMENTS` | Inert on 4b (coverage flat 0.0805 across caps 1→8, latency rises 564→1287ms). Helps slightly on 0.6b. |
| **B. Skill-side dense views** (`e_summary`/`e_task`/`e_needs`) | `RETRIEVAL_DENSE_VIEWS` | **Win on 4b** — the +0.073 recall / +0.057 MRR above. |

The "inert" reading came from (1) conflating A with B, (2) reading the flat numbers off the
`session_start` stratum where absolute MRR ≈ 0.10 (everything floors), and (3) generalizing 0.6b
inertness to 4b.

## Decomposition table (live `find_skill`, 4b, `RETRIEVAL_MAX_RESULTS=50`)

| stratum (n) | recall@50 OFF → ON | MRR@3 OFF → ON |
|---|---|---|
| transcript (36) | 0.583 → 0.778 (**+0.195**) | 0.472 → 0.648 (**+0.176**) |
| use_when (15) | 0.800 → 0.867 (+0.067) | 0.700 → 0.733 (+0.033) |
| disjoint (36) | 0.806 → 0.861 (+0.055) | 0.685 → 0.704 (+0.018) |
| lexical (30) | 0.967 → 0.967 (+0.000) | 0.850 → 0.861 (+0.011) |
| multiview (20) | 1.000 → 1.000 (+0.000) | 0.800 → 0.800 (+0.000) |
| **aggregate** | **0.810 → 0.883 (+0.073)** | **0.684 → 0.741 (+0.057)** |

Reproduces the original T11 candidate-recall finding (0.723 → 0.796) as a *true* deep-pool number.
Evidence: `tests/e2e/reports/retrieval/recall_dv{ON,OFF}_mr50.json`, and the e_summary-only A/B in
`mvprobe_dv_{on,off}_4b.json` / `mvprobe2_dv{ON,OFF}_neg0.json`.

### Methodology gotcha (resolved)

`find_skill` (`crates/mcp-server/src/tools/find_skill.rs:108`) calls `retrieve(..)` with **no
limit**; Task selection truncates to `config.max_results` (**default 3**) *before* the handler's
`.take(request.limit)` (`:124`). So `candidate_recall@50` is really **recall@3** unless
`RETRIEVAL_MAX_RESULTS` is raised. All recall claims here were measured with `MAX_RESULTS=50`.
Per-stratum reporting (added to `t12_task_quality_probe.py`, diffed by `retrieval_stratum_diff.py`)
is the durable fix for the aggregate-hides-the-stratum trap.

## Corpus / field reality (current 277-skill corpus, `skill_layer_test`, `qwen3-embedding:4b`)

- **Fields are populated and distinct** (not the stale 234-corpus, which was empty): use_when 69%,
  avoid_when 69%, invariants/produces 69%, requires/artifacts 62%, tools 55%. Intra-skill
  `cos(e_summary, e_task)` median 0.757 (only 3% > 0.9) → distinct, not redundant.
- **View embeddings:** `e_summary` 277, `e_task` 277, `e_needs` 191, `e_negative` 190.
- **31% field gap:** 86 skills lack `use_when`/`e_needs` → the primary backfill target.
- **Corpus density:** `e_summary` nearest-neighbor cosine mean 0.811; 42% have a neighbor > 0.85,
  6% > 0.95. Dense, but does not neuter the views (see verdict).
- **Hygiene:** `skill_embeddings` holds 540 distinct skill_ids for 4b vs 277 live skills — **263
  orphaned stale embeddings**. Harmless to retrieval (the snapshot rebuilds from live skills and
  only looks up their keys) but worth purging. 0.6b arms are clean at 277.

## e_negative — activated, measured, kept OFF

`e_negative` (`avoid_when`, 190 skills) was built and stored but never read. It is now wired as an
env-tunable subtractive penalty (`apply_negative_penalty`, `RETRIEVAL_NEGATIVE_VIEW_WEIGHT`,
**default 0.0 = byte-for-byte identity**). Measured net-harmful as a flat α-subtraction:

| weight | MRR@3 | no_match precision |
|---|---|---|
| 0.0 (default) | 0.741 | 1.000 |
| 0.25 | 0.527 | 1.000 |
| 0.50 | 0.128 | 1.000 |

A relevant skill's `avoid_when` correlates with its own topic, so the penalty suppresses *true*
positives; `no_match` was already perfect, so there was zero upside on this fixture. **Stays off.**
Re-evaluate only with a margin-gated design (penalize only when `e_negative` beats the positive α)
or on a corpus with imperfect `no_match` (e.g. the `session_start` priming path). Evidence:
`mvprobe2_dvON_neg0p{25,50}.json`.

## Hypotheses verdict

| Hypothesis | Verdict |
|---|---|
| H1 — extraction was bad | **False.** Fields rich + distinct; frontier extraction produced literal discriminating tokens. |
| H2 — multi-view fields poorly populated | **Mostly false.** 55–69% populated and distinct. Real edge: 31% gap + `e_negative` was inert (now wired, kept off). |
| H3 — corpus too small / too similar | **Refuted as the cause.** Corpus is dense, but views add recall *most* where it is hardest; density caps the absolute ceiling, it does not make views inert. |

## Next steps (ranked)

1. **Backfill the 31% field gap** (86 skills missing `use_when`/`e_needs`), re-run this
   decomposition — prediction: transcript recall climbs further. Highest leverage.
2. **Measure dense ON/OFF on the production priming path** (`compile_context`), not just
   `find_skill` — confirm the transcript recall lift carries into session-start.
3. e_negative margin-gated variant on the priming path (imperfect-no_match regime).
4. Corpus dedup (6% near-dup, > 0.95) + purge 263 orphan embeddings → H3 ceiling test.
