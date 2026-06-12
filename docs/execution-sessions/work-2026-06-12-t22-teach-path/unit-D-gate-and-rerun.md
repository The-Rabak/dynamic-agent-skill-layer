---
unit: "Unit D — Two-tier sentinels + smoke re-run (the GO gate)"
unit_number: 4
unit_kind: infra-packet
serves: "Re-level the gate to operative rules; re-run the smoke as the GO gate; recommend GO/NO-GO"
status: completed
attempt_count: 1
domains: [clband-harness, fidelity-gate, e2e]
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/22-teach-path-extraction.md
session_id: work-2026-06-12-t22-teach-path
---

## What Was Implemented
- `manifest.json`: two-tier sentinels for both smoke contexts — `sentinels_document` (reported) +
  `sentinels_operative` (gating), derived VERBATIM from the committed verifiers. aether operative is
  the TRANSLATE-sibling keyword mapping (the sibling actually taught), with 'Turbulence Alert' (a
  different sibling) kept in the document tier only.
- `fidelity_gate.sh`: gates on `sentinels_operative` (fallback to legacy `sentinels`), reports
  `sentinels_document`; broadened skill-file glob to include `*.pending`.
- `run_smoke_rerun.sh`: replays the genuine captured transcripts through the fixed pipeline (Unit B
  delivery + Unit C taught-capture, EXTRACT_TEACH_CAPTURE=on) into isolated re-run scopes, runs the gate.

## Result — GO gate GREEN (both contexts PASS)
| context | operative | verdict | drafts |
|---|---|---|---|
| flywheel | 7/7 (next size up, extra torque, firm shake, retest, spin test, Validation Engineer, Forklift) | PASS | 19 |
| aether | 5/5 (conduit, flow, fork, swirl, `<<`) | PASS | 19 |

Captures verified genuine (spot-read): flywheel `adaptive-continuation-over-stoppage` ("use the next
size up and apply extra torque" verbatim); aether `aether-assignment-and-operator-syntax` ("assignment
is `<<` (Flow operator), NOT `=`", spec §7.2) + `aether-keyword-statement-mappings` (full invented
keyword set) — PROSE-channel taught skills (the channel that previously refused 3×).

Gate-mechanics validation: the OLD preference-channel smoke drafts correctly FAIL 7/7 operative — the
exact T22 failure — confirming the new gate is properly leveled.

## Decisions
- DP-2 (drafts gate): owner chose LEAVE AS .pending EVIDENCE (no acceptance; isolated scratch scopes).
- DP-3 (GO/NO-GO): recommendation = GO; owner HOLDING pending review of the session summary.

## Fences honored
- Replay (not re-capture) labeled; re-capture available via run_teach_session.py.
- Scope isolation re-probed: corpus still 262, zero clband/dogfood leakage.
- No auto-approval of drafts.

## Test Results
- run_smoke_rerun.sh rc=0; both gates exit 0. Raw: tests/e2e/reports/efficacy/clband-rerun/. Attempts: 1.
