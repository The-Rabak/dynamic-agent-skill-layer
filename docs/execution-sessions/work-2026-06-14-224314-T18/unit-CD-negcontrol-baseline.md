---
unit: "Units C+D — Negative-control gate + baseline prime through compile_context"
unit_number: 3
unit_kind: infra-packet
serves: "Proves the coverage metric is non-vacuous (C), then delivers T12's honest before-number (D)"
status: completed
attempt_count: 1
domains: [measurement, retrieval, python, live-stack]
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/18-priming-instrument-session-start-stratum.md
session_id: work-2026-06-14-224314-T18
---

## What Was Implemented
- `scripts/retrieval_metrics.py` += `set_coverage_at_n`, `freshness_hit_rate` (+10 self-tests → 56, all PASS).
- Drove the REAL mcp-server `compile_context` (`http://127.0.0.1:3001/mcp`, `repo_path:"project"`,
  unique session_id/call) over all 22 session_start queries; injected skills parsed from the real
  `## Skill:` headers in the compiled output (verified: parsed `injected_names` == headers, set-equal).
- Raw artifacts in `tests/e2e/reports/retrieval/`: `session_start_raw_compile_context.json` (383KB, 22
  real responses, 19 ok / 3 no_match), `session_start_negcontrol.json`, `session_start_baseline.json`.

## Unit C — Negative-control gate: PASS → INSTRUMENT-VALID
| | mean set-coverage@3 |
|---|---|
| True (injected vs own gold) | **0.0685** |
| Permuted (cyclic-shift-1 derangement) | **0.0321** |
| Separation S | **0.0365** |
| rel-drop | **0.532** (> 0.50 gate) → craters |
Pre-registered gate honored: permuted craters (53.2% drop). **Marginal pass with LOW absolute coverage**
— which is itself the signal: the current prime is barely above the popularity floor. (With S=0.0365 the
per-signal ABSOLUTE floors 0.10/0.15/0.043 dominate the separation-fractions — T12 is graded on those.)

## Unit D — Baseline (the before-number T12 must beat)
| metric | thin (11) | verbose (11) | overall |
|---|---|---|---|
| set-coverage@3 | 0.110 | 0.027 | **0.0685** |
| no_match rate | 0.182 | 0.091 | 0.136 |
| freshness hit-rate | 0.727 | 0.273 | — |
| p95 latency | 399ms | **734ms** | 608ms |

## Three concrete findings for T12 (the payoff)
1. **Coverage is poor** (0.0685 ≈ 21% of the achievable-at-N=3 ceiling, gold sets mean 9.5 vs 3 injected).
2. **Finding-2 REFINED:** on the dogfood session-start distribution, verbose openings do NOT predominantly
   no_match (verbose 9% < thin 18%) — they retrieve the WRONG skills (topic-density dilution → 4× worse
   coverage). So T12's verbose problem here is DILUTION/mis-ranking, not no-retrieval. (Pure no_match was
   the CL-bench off-domain distribution.) Query-side multi-view / max-over-segments (T12 mechanism b) is
   the matched remedy.
3. **Raising N is INERT** — the 0.48 floor caps the candidate pool at ≤3 for these queries, so the
   diagnostic find_skill curve is flat (@3=@5=@8). T12 must use an intent-conditional floor (mechanism a)
   or a recurrence/freshness signal that surfaces below-threshold skills — NOT a bigger window.
   PLUS: verbose p95 (734ms) already BREACHES the 500ms SessionStart budget → T12 must watch latency.

## Validity outcome (pre-registered)
VALID instrument: negative control craters AND anti-circularity probe (0.024) ≤0.3 band. Baseline is the
honest before-number; T12 graded on §3/§4 of the locked pre-registration. Judge usefulness (secondary)
DEFERRED (avoid model-call storm); rubric is recorded in the pre-registration.

## Test Results
- `python3 scripts/retrieval_metrics.py --self-test` → ALL TESTS PASSED (56). Live measurement: 22/22 real
  compile_context responses persisted. Attempts: 1.
