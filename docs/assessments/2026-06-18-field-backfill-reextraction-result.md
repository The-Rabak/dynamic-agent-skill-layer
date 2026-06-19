# Field-backfill re-extraction — result (2026-06-18)

Branch `feat/v-1-7`. Follows the work prompt
`docs/plans/2026-06-18-field-backfill-recall-eval-work-prompt.md` and the decomposition
`docs/assessments/2026-06-18-multiview-recall-vs-rerank-decomposition.md`.

## TL;DR

The framed hypothesis — *backfilling the 86 missing-`use_when` skills raises recall, especially on
transcript/use_when* — is **disproven**, in two stages:

1. **find_skill is structurally inert** (proven analytically, no extraction needed): the 86
   empty-`use_when` skills are **disjoint from the find_skill gold set**, and **every** positive-stratum
   gold already has `use_when`. Backfilling non-gold skills cannot raise `candidate_recall@50` by
   construction. The only field gap on actual find_skill golds is `requires`/e_needs on **6** skills, of
   which only **3** are currently missed (`inspect-current-state-before-mutating`,
   `build-tool-silent-coverage-gap`, `two-phase-artifact-wiring-…`).

2. **The priming path (`compile_context`) is where the real gap lives** (21 session_start golds miss
   `use_when`, 33 miss `requires`; 19/22 session_start queries have a backfill-target in their relevant
   set) — so the effort was pivoted there. But the honest backfill **cannot be performed**:
   re-extracting the genuine source transcripts with the current frontier prompt yields **0 grounded
   fields** for these specific skills. The corpus was **not mutated**; no re-measurement was warranted.

## Method (real pipeline, no fakes)

- Pre-backfill priming baseline captured on the live `compile_context` path over the 22-query
  `session_start` stratum (dense ON = production default): **primed coverage@3 = 0.0805** (thin 0.124 /
  verbose 0.037), baseline (no-trigger) 0.0685 — reproduces the T18 before-number; negative control
  craters (instrument valid). `tests/e2e/reports/retrieval/t12_priming_pre_backfill.json`.
- Re-extracted the **14 genuine source transcripts** (`~/.claude/projects/.../<uuid>.jsonl`) that the
  28 recoverable session_start targets trace to, with the **current frontier prompt** (claude-code /
  sonnet-4-6, `prompt_contract.rs` @ f1647d5, newer than the 2026-06-10 corpus build), via the rebuilt
  `maintenance-worker` over the real `/ingest/transcript` + transcript-drain path, into a **scratch dir**
  (live corpus untouched). 174 drafts; **122 carry rich multiview fields**.
  Drivers: `scripts/backfill_reextract.py`.
- **Confidence-gated semantic match** (the chosen method) fresh-draft → existing target:
  description-embedding cosine (real ollama qwen3-4b), accept only on cosine ≥ 0.90 + mutual-best +
  margin ≥ 0.03 + same source session. `scripts/backfill_match.py`,
  `tests/e2e/reports/retrieval/backfill_match_review.json`.

## Why 0 honest backfills

The re-architected frontier prompt **re-segments** sessions — it does not field-update the existing
skill identities. Two disjoint draft classes emerge, with an **airtight correlation**:

| draft class | count | has `skill_type` | multiview fields |
|---|---|---|---|
| typed (failure_fix/rule/best_practice/diagnostic/anti_pattern/principle/procedure) | 122 | 122/122 | **rich** |
| type-less / description-only | 20 | 0/20 | **empty** |

`rich & no-type = 0`, `empty & has-type = 0`. The extractor populates multiview fields **iff** it assigns
a `skill_type`.

**All 28 backfill targets map to the type-less / description-only class.** They are episodic or trivial
items — tool-error-recovery procedures (`read-before-edit-*`, `reread-file-before-edit-*`,
`retry-edit-after-file-modified`), session task-narratives (`audit-rust-system-honesty`,
`compare-retrieval-arms-top10`, `triage-todo-backlog-with-decisions`, `deepen-plan-*`), and narrow
one-off fixes (`fix-bm25-silent-none-fallback`, `fix-askuserquestion-options-overflow`). Where
re-extraction preserves a target's identity (5 matches at cosine 0.90–0.95, e.g.
`audit-rust-codebase-stub-violations` ≈ `audit-rust-system-honesty` @ 0.954) the fresh draft is **itself
field-empty**; where re-extraction produces rich fields, the draft is a **different skill** (e.g. source
`8a0965e3` yields rich `fail-loud-must-cover-all-service-binaries`,
`live-e2e-tests-must-not-inject-in-process-fakes` — none of which is the target).

So the field gap on these skills is **not** an extraction loss or a stale-prompt artifact: the improved
prompt, demonstrably capable of rich multiview output (122/142 drafts), **deterministically leaves these
specific items type-less and field-empty**. Backfilling their `use_when`/`requires` would require either
(a) fabricating fields onto untyped episodic records (violates the no-fakes mandate), or (b) accepting a
re-segmented corpus (breaks the fixture, whose golds are pinned to the old identities).

## ROOT CAUSE + FIX (2026-06-19): skeleton-mining is provider-blind

Tracing *why* these items are type-less: the orchestrator strategy is **"prose always, skeleton
additive"** (`crates/session-extractor/src/orchestrator.rs:558`). For every window it runs the frontier
prose extractor (→ rich typed multi-view skills) and, when a failing→passing **tool arc** exists, it
*additionally* runs the **skeleton miner** (`skeleton::map_episode`) and unions the result in. The
skeleton miner builds its candidate with `..Default::default()` — **type-less and view-less by
construction** — because it was designed (#188) as a bounded label transform (name/description/
generality/keep only) for **small local models that hallucinate** when asked to invent structured skills.

This pass is **provider-blind**: it runs even on the frontier tier, where the prose extractor already
produces a rich typed skill from the same window. The result is the corpus's 22 view-less `skeleton-mined`
skills — episodic "what-I-did" tool-arc traces that are often near-duplicates of a richer prose skill and
never participate in dense-view retrieval. They are 22/277 (8%) of the corpus and the bulk of the
"31% field gap."

**Fix (implemented, tested):** gate the additive skeleton pass on the routing tier. New
`OrchestrationConfig.skeleton_mining_enabled` (default `true` to preserve local-tier behavior + all
existing tests); `lib.rs` sets it `false` when `routing_decision.tier == Frontier`; `map_one_window`
skips `map_episode` when disabled. Unit test
`map_one_window_skeleton_gate_off_drops_additive_skeleton_on_tool_arc` (185 lib tests pass, clippy/fmt
clean). Gated smoke on `3367184b`: **10 drafts, 0 skeleton-mined, 0 view-less, 7 rich** (vs ungated 14
with view-less dups) — skeleton dups removed, rich prose coverage retained.

### Measured impact (reversible gate-simulation on the live stack)

The gated corpus is faithfully simulated by removing the 22 view-less skeleton skills from the served
corpus (the rich prose skills are already present from the ungated build; MMR `lambda=0.65` makes an
offline post-filter inexact, so the skills were removed from the real served set and re-reconciled). The
22 are **0 find_skill positive-stratum golds** (clean) and **16 session_start golds** (priming confound).

| metric | baseline (277) | gated-sim (255) | delta |
|---|---|---|---|
| **find_skill MRR@3** (clean) | 0.7409 | 0.7445 | **+0.0036** |
| find_skill nDCG@3 | 0.7512 | 0.7539 | +0.0027 |
| find_skill transcript MRR@3 | 0.6481 | 0.6620 | **+0.0139** |
| find_skill no_match precision | 1.000 | 1.000 | 0 |
| priming coverage@3 *(confounded)* | 0.0805 | 0.0729 | −0.0076 |

- **find_skill: a small clean win, zero downside.** Removing the view-less skeleton distractors slightly
  improves ranking (most on the transcript stratum, where these traces most resemble failure-narrative
  queries); no gold is removed, no_match stays perfect, recall unchanged.
- **priming: the −0.0076 is a fixture artifact, not a regression.** It drops *only* because the priming
  fixture pins 16 of the removed low-value skeleton skills as `session_start` golds. This is direct
  evidence the priming fixture rewards view-less episodic skills and should be regenerated against a
  gated corpus — it is not the gate being harmful.

Live corpus restored to 277 (byte-identical to the pre-run snapshot) after the measurement.

### Status / deployment

Code change landed in the working tree (`session-extractor`), worker host-binary rebuilt + validated.
NOT yet committed and the `maintenance-worker` **Docker image is not rebuilt**, so production extraction
still runs ungated until that ships. Recommended follow-on: regenerate the corpus from the gated prompt
(frontier) and regenerate the priming fixture golds against it, then re-measure coverage@3 on a
quality-aligned fixture.

## Corpus integrity

Live corpus byte-identical before/after the run: **277 skills, 0 added / 0 removed / 0 field-changed**
(`corpus_fieldstate_pre_backfill.json` vs post). The scratch isolation + graph-builder's
file-authoritative reconcile kept the worker's maintenance passes from mutating the served corpus. The
0.0805 baseline stands; with 0 fields applied, post-backfill priming is identical by construction —
no re-measurement (running it would be measuring an unchanged system).

## Implications / recommended next steps

1. **The type-less tier is the real lever, not field-backfill.** The actionable finding is an extraction
   question: should episodic tool-recovery records and session task-narratives be in the served corpus at
   all, and if a subset (e.g. `fix-askuserquestion-options-overflow`, which arguably *has* a trigger)
   should be typed, the fix belongs in the **typing/generality gate of `prompt_contract.rs`**, not in a
   field-backfill pass. Investigate why these items are denied a `skill_type`.
2. **The fixture is pinned to low-value identities.** 28 of the 153 session_start golds are type-less
   episodic items. Consider whether the priming eval should reward retrieving them at all; a corpus +
   fixture regeneration from the current prompt (122 rich drafts available) is the larger, cleaner play.
3. **e_needs on the 3 missed find_skill golds** (`inspect-current-state-before-mutating`,
   `build-tool-silent-coverage-gap`, `two-phase-artifact-wiring-…`) is the only remaining honest
   find_skill backfill — but it shares the same blocker (they are type-less; re-extraction won't field
   them) and the ceiling is ~5 query-instances.

## Artifacts

- Drivers: `scripts/backfill_reextract.py`, `scripts/backfill_match.py`, `scripts/backfill_apply.py`.
- Baseline: `tests/e2e/reports/retrieval/t12_priming_pre_backfill.json`.
- Match review: `tests/e2e/reports/retrieval/backfill_match_review.json`.
- Integrity snapshot: `tests/e2e/reports/retrieval/corpus_fieldstate_pre_backfill.json`.
- Re-extraction scratch (kept): `/tmp/backfill-reextract/extract_result_{smoke,full}.json` + worker logs.
