---
unit: "Pipeline + human gate + fidelity gate"
unit_number: 4
unit_kind: infra-packet
serves: "proves (or disproves) extraction fidelity at both context sizes"
status: completed-finding
attempt_count: 1
domains: [extraction, fidelity, clband]
session_id: work-2026-06-12-clband-smoke
---

## What Was Implemented

Real extraction (`clband_extract.py`: `/ingest/transcript` → host `maintenance-worker` RUN_ONCE +
TRANSCRIPT_DRAIN, `EXTRACT_SESSION_PROVIDER=claude-code`, isolated per-context scope root) on both
teach transcripts, then the committed `fidelity_gate.sh` (deterministic manifest-sentinel coverage).

## Result — INSTRUMENT-FAILURE(extraction) at BOTH sizes (the headline smoke finding)

| context | size | drafts | sentinels present | gate |
|---|---|---|---|---|
| flywheel-assembly-agent | 4.2k | **11** | **0/4** (Flywheel Manufacturing Multi-Agent System, Scatterbrained Improviser, spin test, WORKAROUND) | **FAIL** |
| aether-language | 33.5k | **0** | 0/4 (conduit, swirl, Turbulence Alert, Fracture) | **FAIL** |

**This is NOT the plan's hypothesized size threshold (pass@4.2k / fail@33.5k).** It is a uniform
extraction-DESIGN obstacle at both sizes, by two mechanisms:

1. **aether → outright refusal.** The extractor's salience assessment (logged verbatim):
   *"candidate_count=0 … a one-off creative translation exercise for a fictional programming language
   ('Aether') … zero transferable value … nothing would recur on a future, different task."* The
   "would this recur?" grounding gate rejects invented-knowledge contexts by design → 0 drafts.
2. **flywheel → abstraction.** 11 drafts, but the dual-pass generalization turned the invented SOP
   into generic real-world principles (`Physically verify specifications`, `Preserve existing
   integrity`, `Always trace every decision to a named artifact`) + scenario preferences (batch
   numbers, the M8x20 misprint). Every invented specific — the system name, the Scatterbrained
   persona, the two WORKAROUND protocols, the spin-test mandate — was stripped. 0/4 sentinels.

The binding constraint for the teach-session protocol is the extractor's **recurrence + generalization
design** (correct and valuable for the organic dogfood corpus; wrong for capturing invented rules),
NOT context size, NOT the injection path. The smoke surfaced the real blocker on 2 contexts before 8 more.

## Consequence
- No faithful drafts exist to accept → the DP-1 human gate has nothing rule-bearing to approve
  (recommend REJECT all 11 flywheel drafts; provenance fence forbids hand-editing the rule in).
- Session B (Unit 5) CANNOT run as designed — there are no rule-bearing skills to inject; no
  ON/OFF/PLACEBO efficacy reading is obtainable for either context.
- GO/NO-GO for the full 8-context band: **NO-GO** as currently wired — all 8 are invented-knowledge
  contexts and would hit the same filter.

## Artifacts (raw)
- `tests/e2e/reports/efficacy/clband-smoke/extract_{flywheel-assembly-agent,aether-language}.json`
- `.../clband-smoke/logs/worker-*.log` (the verbatim refusal assessment is in the aether log)
- `.../clband-smoke/fidelity_gate_result.txt`
- 11 flywheel `.pending` drafts under `.../scopes/clband-flywheel-assembly-agent/.skills/` — these
  are REAL drafts from a REAL captured session (count toward T14's ≥10-real-drafts AC), but
  sentinel-unfaithful for THIS experiment.

## Test Results
- Extraction: flywheel rc=0 (11 drafts), aether rc=0 (0 drafts, logged refusal). Both fidelity FAIL.
- No auto-approval performed (fence honored). STOP → owner (DP-1 + GO/NO-GO).
