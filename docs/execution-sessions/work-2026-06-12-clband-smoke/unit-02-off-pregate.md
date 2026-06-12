---
unit: "OFF pre-gate"
unit_number: 2
unit_kind: fix-item
serves: "empirical non-pretraining + discrimination per candidate sibling"
status: completed
attempt_count: 1
domains: [efficacy-harness, clband, solves]
session_id: work-2026-06-12-clband-smoke
---

## What Was Implemented

Ran the OFF pre-gate (plan §4 Step 0): each candidate measured sibling solved by a BARE agent
(`claude --print --model sonnet`, no skill layer, no context, no injection) against its committed
deterministic verifier. Reused the validated harness `scripts/efficacy_ab.py --arms off`. Solver:
`claude-code 2.1.173, --model sonnet`. Dataset sha `b28a5832...`. Serialized (one solve at a time).

## Result — ALL 4 candidates CRATER on OFF (none rejected)

| sibling | OFF outcome | deterministic reason |
|---|---|---|
| clband-flywheel-979ec26a | **loss** | missing wrench workaround 'next size up' |
| clband-flywheel-46536e4a | **loss** | missing wrench workaround 'next size up' |
| clband-aether-turbulence-b0807c2c | **loss** | missing the invented 'Turbulence Alert' section |
| clband-aether-translate-4768e426 | **loss** | Aether 'conduit' still present — not translated to def |

Persisted: `tests/e2e/reports/efficacy/clband-smoke-offpregate/{report.json,report.txt}`.

**Interpretation (this is the discrimination the self-authored battery lacked):** the aether
mechanism is exactly as predicted — OFF does not recognize `=` as a bug (it is ordinary assignment
in every mainstream language), so it never emits a Turbulence Alert. flywheel OFF produces a
competent assembly plan but never the invented "next size up + extra torque" workaround. Every
candidate's invented specifics are genuinely not producible without the rules.

**Gate decisions:** 0 siblings pass OFF ⇒ **0 rejected**, no alternate substitution, DP-3 (aether
double-rejection) NOT triggered. Both aether held-outs survive ⇒ the large-context half is provable.
All 4 carry forward to teach/measure (Session B will use 1–2 per context).

## Limitation (honest)
The harness auto-cleans the per-solve temp workspace, so the OFF solution.md texts are not persisted;
the recorded evidence is the deterministic verifier outcome + one-line reason per task in report.json
(a selection gate, not measured paired data). Session B (Unit 5) persists full attribution.

## Test Results
- OFF pre-gate: 4/4 OFF=loss (discrimination confirmed). Report written. No INSTRUMENT-FAILURE
  possible here (off-only). The harness "UNDERPOWERED" line is an off-only artifact (on=n/a→ties);
  ignored — smoke produces no verdict.
