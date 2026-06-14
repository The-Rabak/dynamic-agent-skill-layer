---
unit: "Unit A — Pre-registration (DRAFT, awaiting owner lock)"
unit_number: 1
unit_kind: infra-packet
serves: "The locked, priming-appropriate ruler T12's signals are graded against; the owner sign-off gate"
status: draft-staged
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/18-priming-instrument-session-start-stratum.md
session_id: work-2026-06-14-224314-T18
---

# T18 priming-instrument — PRE-REGISTRATION (DRAFT for owner lock)

**Status: DRAFT. Nothing measured. No stratum authored.** Per pre-registration discipline, the
content below must be locked (owner-approved, committed verbatim into the T18 ticket) BEFORE the
session-start stratum is authored and BEFORE any measured run. Numbers marked **[LOCK]** are the
owner's to confirm/adjust; the *decision rules and discipline* around them are the non-negotiable part.

Grounding (real artifacts, read this session): metrics extend the T20 shared lib
`scripts/retrieval_metrics.py` (which today has mrr/ndcg/recall/candidate_recall/sign_test/crater_check
but **no priming metrics**); the stratum extends `tests/fixtures/retrieval_quality_262_corpus_labeled.json`
(162 queries across transcript/disjoint/lexical/multiview/use_when/negative — **no session_start
kind**); the surface under test is `compile_context` (production SessionStart, <500ms zero-touch).

---

## 1. What this instrument measures and why

The SessionStart **prime** is a bounded, zero-touch set of skills `compile_context` injects when a
session opens — no single gold answer, unlike task retrieval. T11 proved MRR/nDCG are the wrong ruler
here (quantized, saturated, single-gold). This instrument measures the prime with
**priming-appropriate** metrics so T12's signals (recurrence-baseline, freshness slot, centrality,
recent-use) are kept or dropped on evidence. It also quantifies the production failure the CL runs
worked around: `compile_context` returns `no_match` on realistic **verbose** openings (T14 smoke
Finding 2).

## 2. Session-start stratum design (authored in Unit B AFTER lock — NOT yet written)

- **Two substrata**, both drawn from the 24 genuine transcripts' OPENING turns:
  - **thin/vague** openings (short, underspecified — "ok let's keep going on the retrieval thing").
  - **verbose** openings (multi-paragraph, code blocks, full real length) — the Finding-2 distribution.
- **Multi-gold labels:** each query's gold is a SET `G_q` of project-baseline skills a useful prime
  should surface (NOT a single anchor), mapped via `source_session_id`. A subset of golds is tagged
  `fresh` (high-value, brand-new / low-prior-use) to drive the freshness metric.
- **Anti-circularity (hard gate, Unit B):** prompts come from opening turns / fresh-vocabulary
  paraphrases, NEVER from the gold skills' `use_when`/`description`. Token-overlap probe headline must
  sit in the **~0.3 band** (T11 transcript/disjoint), reject the query if **≥0.6**.
- Size: **≥20 queries** per the AC (target ~12 thin + ~12 verbose if cheap), each with a labeled set.

## 3. Metric definitions (NEW additions to `retrieval_metrics.py`)

Let `P_q,N` = the top-N skills `compile_context` injects for query `q`; `G_q` = labeled gold set.

1. **set-coverage@N** = `|P_q,N ∩ G_q| / |G_q|`. **Headline = mean over queries at N = the production
   `compile_context` injection cap** [LOCK: confirm the cap]; report the coverage **curve** over
   `N ∈ {3,5,8,…,cap}`. This is the primary metric.
2. **freshness hit-rate@N** = over queries with ≥1 `fresh` gold, the fraction for which ≥1 `fresh`
   gold appears in `P_q,N`. Motivates the T12 freshness slot.
3. **judge usefulness** (secondary) = a committed claude-CLI judge (sonnet) rates `P_q,N` for the
   opening on 0–2 across three dimensions (see §6); report mean aggregate (0–6) + per-dimension.
4. **SessionStart p95 latency** through `compile_context` — must stay **<500ms** (constitutional);
   raw artifact persisted. (A guardrail, not a quality metric.)

MRR/nDCG are explicitly **NOT** priming metrics here (recorded as a secondary curiosity at most).

## 4. Per-signal ROI thresholds (the keep/drop bars — default DROP)

Decision per signal: **KEEP iff** the paired improvement over the no-signal baseline clears the bar by
**sign test p<0.05 AND mean delta ≥ threshold**; otherwise DROP and record the measured delta.

To make a brand-new metric's threshold meaningful *before* data, anchor it to the negative-control
**separation** `S = mean set-coverage@N(true) − mean set-coverage@N(control)`, which Unit C establishes
FIRST. A priming signal keeps iff its paired gain ≥ `max(absolute_floor, fraction·S)`.

| Signal | Metric it must move | Absolute floor [LOCK] | Separation rule | Default |
|---|---|---|---|---|
| recurrence-baseline prime (core) | set-coverage@N vs current undifferentiated SessionStart | **+0.10** | ≥ 25%·S | DROP unless cleared |
| freshness slot | freshness hit-rate (with set-coverage@N drop ≤ **0.02**, no cannibalization) | **+0.15** | ≥ 25%·S_fresh | DROP unless cleared |
| centrality | candidate-recall@limit (task) **or** set-coverage@N (priming) | **+0.043** (T11 gate margin) | ≥ 50%·S | DROP (T11: ranking inert @262) |
| recent-use | same as centrality | **+0.043** | ≥ 50%·S | DROP |

Verdicts are recorded as **scale-bound (262 skills)** — centrality may matter at 5k where
candidate-recall is the predictor.

## 5. Negative-control gate (Unit C — runs FIRST, before any baseline number)

The α=0 analogue for priming. **Primary = PERMUTATION control** (no second corpus needed): score each
query's true prime against a *different* query's gold set (random derangement). **Proceed iff it
CRATERS:** `mean set-coverage@N(permuted) ≤ 0.5 × mean set-coverage@N(true)` [LOCK: confirm the 0.5
crater ratio] — reuse `crater_check()` from `retrieval_metrics.py`. If it does NOT crater, the
coverage metric is **vacuous → stratum REJECTED**, no baseline recorded, no T12 verdict may ride on it
(report as INSTRUMENT-FAILURE(priming-stratum)).
- **Secondary/optional:** cross-project wrong-scope control (needs a second corpus; ties to T19/T25
  placebo) — noted as future, NOT blocking.

## 6. Judge rubric (committed verbatim; claude-CLI, sonnet)

> "You are rating a set of skills a coding-agent memory injected at the START of a session, given the
> session's opening message. For each dimension score 0/1/2:
> (a) **relevance-to-opening** — do the skills relate to what the opening is about?
> (b) **actionability** — are they concrete enough to act on (procedures/conventions), not vague?
> (c) **non-redundancy** — is the set diverse, not N near-duplicates?
> Output strict JSON `{relevance:0-2, actionability:0-2, non_redundancy:0-2, reason:"…"}`. Score only
> what is shown; do not invent." (Reference golds are judge-authoring material, never shown to arms.)

## 7. Baseline protocol (Unit D — through the PRODUCTION surface)

Measure the CURRENT SessionStart prime (dense-views default-ON) **through `compile_context`** (NOT
`find_skill`) on the new stratum, on the REAL mcp-server over HTTP, gated on `/health 200`,
`OLLAMA_EMBED_MODEL=qwen3-embedding:4b`, corpus = 262. Headline: set-coverage@N, freshness hit-rate,
judge usefulness, p95. **Verbose substratum: report the `no_match` RATE explicitly** (Finding 2
quantified — the honest before-number for T12's fix). `find_skill` numbers secondary if cheap. Raw
per-query JSON persisted (paired-ready, T11 format) under `tests/e2e/reports/`.

## 8. Validity outcomes (pre-committed)

- **VALID instrument** iff negative control craters (§5) AND anti-circularity probe ≤0.3 band (§2).
  Then the baseline is the honest before-number and T12 is graded on §3/§4.
- **INSTRUMENT-FAILURE(priming-stratum)** iff the control does not crater or overlap is too high —
  reported as such; T12 does not proceed on this stratum until fixed. (No spinning a vacuous metric.)

## 9. Numbers the OWNER locks before Unit B (all else is fixed discipline)

1. The four **absolute floors** in §4 (+0.10 / +0.15 / +0.043 / +0.043) and the separation fractions
   (25% / 25% / 50% / 50%).
2. The **crater ratio** in §5 (proposed 0.5×).
3. The **headline N** in §3 (tie to the real `compile_context` injection cap — to be confirmed from
   the code at go-time).
4. Negative-control flavor: permutation (proposed, available now) vs cross-project (future).

---

**HELD HERE.** On owner "go": lock §1–§9 into the T18 ticket verbatim, then Unit B authors the
stratum, Unit C runs the negative-control gate FIRST, Unit D measures the baseline through
`compile_context`. Until then: no stratum, no live server, no measurement, no heavy agents.
