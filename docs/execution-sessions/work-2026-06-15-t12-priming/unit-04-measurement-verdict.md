---
unit: "Re-measure on T18 instrument + per-signal keep/drop verdicts"
unit_number: 4
unit_kind: hardening
serves: "the measured evidence + owner verdicts; graded against the T18 LOCKED pre-registration"
status: completed
attempt_count: 1
domains: [measurement, retrieval, live-stack, python]
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/12-trigger-aware-retrieval-priming-mode.md
session_id: work-2026-06-15-t12-priming
---

## Method (measurement drives the REAL mcp-server over HTTP)
Rebuilt + restarted mcp-server with the T12 code; gated on `/health` ready. New driver
`scripts/t12_priming_sweep.py` calls the production `compile_context` surface twice per stratum query
(unique session_id) — once WITHOUT trigger (`Task`/baseline = the T18 before-number) and once WITH
`trigger:"session_start"` (`Priming` ranker) — over the 22-query T18 `session_start` stratum. Metrics via
the T20 shared lib `scripts/retrieval_metrics.py`. Negative-control gate runs FIRST. Raw artifacts:
`tests/e2e/reports/retrieval/t12_priming_{default,norec_nofresh,highrec}.json`.

## Negative-control gate (T18 §5, runs FIRST) — PASS
Permutation derangement of gold sets over the PRIMED injected sets: true mean cov@3 **0.0805** vs
permuted **0.0302** → rel-drop **62.5% > 50% gate → INSTRUMENT-VALID** for the primed arm. (T18 baseline
arm reproduced EXACTLY: cov@3 0.068513 ✓ — the harness is consistent.)

## Headline result (graded against T18 LOCKED thresholds, cited verbatim)
| arm | cov@3 | thin | verbose | no_match | fresh-hit thin/verbose | p95 thin/verbose |
|---|---|---|---|---|---|---|
| baseline (Task, T18) | 0.0685 | 0.110 | 0.027 | 13.6% | 0.727 / 0.273 | 284 / 560 ms |
| **primed (default)** | **0.0805** | 0.124 | 0.037 | **0%** | **0.909** / 0.364 | 597 / **2239** ms |

**Paired baseline-vs-primed (cov@3):** mean delta **+0.012**, primed_better 3, baseline_better 2, tie 17,
**sign-test p = 1.0**. Diagnostic: primed coverage@5 (the full 5-skill prime) = 0.101.

## Per-signal keep/drop verdicts (T18 thresholds verbatim; ablation-isolated)
1. **recurrence-baseline** — threshold *"+0.10 absolute set-coverage (→ ≥0.17) by paired sign test
   p<0.05"*. Measured Δcov@3 = **0.000** at recurrence_weight ∈ {0.10, 0.60} (cov@3 0.0805 identical to
   weight=0). → **DROP.** Usage priors are sparse/near-uniform on this 262 corpus → the boost cannot
   reorder the top-3. Extends T11's "ranking inert @262" to the recurrence signal. Scale-bound (262).
2. **freshness slot** — threshold *"+0.15 hit-rate with ≤0.02 coverage cannibalization"*. The SLOT's
   ISOLATED contribution = **0.000**: freshness hit-rate is IDENTICAL with slots ON (1) vs OFF (0)
   (thin 0.909, verbose 0.364 in every arm). → **DROP.** The freshness hit-rate gain over baseline
   (thin +0.18) is a side-effect of the lower floor + N=5 surfacing more skills, NOT the reserved slot.
   Also confirmed: wall-clock `created_at` is near-uniform on this together-rebuilt corpus (Unit 3 flag).
3. **centrality / recent-use** — threshold *"+0.043 (default DROP — T11 ranking-inert @262)"*. Not
   implemented; **DROP by default.** The measured recurrence-inertia corroborates ranking-inertia @262.
4. **intent floor (0.30) + query-side multi-view** — the ONLY active lever. cov@3 +0.012 (**FAILS the
   +0.10 recurrence-baseline bar**, sign p=1.0). BUT delivers two real wins: **no_match eliminated
   (13.6%→0%)** — the production prime is never empty (the FIRST-scope-item goal) — and freshness
   hit-rate up. **COST: verbose p95 ~2240ms (≈4× the T18 baseline), breaching the 500ms fence badly.**

## Honest overall verdict
- **Primary coverage@3 bar (+0.10): NOT MET** (+0.012, p=1.0). All four ranking signals are inert or
  sub-threshold on this corpus/scale.
- **Latency fence (500ms): BREACHED** on verbose (~2240ms) — `embed_batch` of up to 8 segments is
  sequential on the Ollama side; the multi-view cost violates T18 constraint #3 (must not worsen verbose).
- **Real but secondary wins:** no_match elimination (13.6%→0%) directly fixes the motivating production
  failure; freshness hit-rate up (from floor+N, not the slot).
- Negative control valid; Task path byte-identical (existing gates green).

## Recommendation to owner (do NOT default-ON as-is)
The ranker does NOT clear its coverage bar and the verbose latency breaches the fence → **do NOT flip
default-ON in this state.** Two coherent paths for owner choice (see the work message). The shippable
kernel is the no_match-elimination (lower Priming floor) IF latency is fixed (cap segments / lazy
multi-view); the reranking signals (recurrence, freshness, centrality) are all DROPPED on measured deltas.

## Test Results
- `scripts/t12_priming_sweep.py` ran 3 server configs (default + 2 ablations) over the live server; raw
  per-query artifacts persisted. retrieval_metrics self-test still 56/56. Server restored to default config.
