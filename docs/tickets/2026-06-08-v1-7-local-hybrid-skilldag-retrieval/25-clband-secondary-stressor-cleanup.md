---
ticket_id: T25
title: CL-band → clean secondary stressor — verifier-based fidelity gate + task-design + placebo/circuit-breaker
kind: measurement
status: ready
status_note: "LOW priority / conditional — CL-bench is DEMOTED from primary efficacy gate to one optional adversarial stressor. Do this only when a CL re-run is actually wanted (e.g. T15's optional CL arm); not on the critical path."
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: T23 band report (docs/assessments/2026-06-13-t14-clband-band.md §Recommendations) + CL-band plan §4"
source_packet_ref: "NEW 2026-06-13 — from the T23 band's instrument/task-design findings + owner reprioritization (CL-bench is the wrong PRIMARY gate; keep it only as a clean secondary stressor)"
feature_home: "tests/e2e/efficacy/clband + scripts/ (harness only — NO production crates)"
depends_on: []
dependency_type: none
serves:
  - Makes the CL band a TRUSTWORTHY secondary stressor (no false-negatives, no degenerate controls, no self-inflicted timeouts) so that IF it is ever re-run as an adversarial arm, its result is clean — without it being the primary efficacy gate
files:
  - tests/e2e/efficacy/clband/
  - tests/e2e/efficacy/clband/manifest.json
  - scripts/
test_command: "verifier-based fidelity gate recovers quartermaster (verifier PASS → MEASURED); all 8+3 contexts have real instruments; placebo never self-donates; circuit-breaker halts on correlated failure"
tdd_mode: ralph
---

# CL-band → clean secondary stressor

## Serves

The T23 band proved the harness plumbing but produced 0 clean efficacy points, and its instruments
had four fixable defects. CL-bench is now DEMOTED (owner decision 2026-06-13): it tests the
injected-document, one-shot, buried-value regime, which is NOT the real use case — so it is no longer
the primary efficacy gate (that role moves to T15 SWE-bench compounding). This ticket exists so that
IF CL is re-run as an optional adversarial arm (T15's conditional CL-bench-shaped arm), it runs on
clean instruments — and so the band's hard-won findings aren't lost. It is LOW priority and
conditional; it is NOT on the real-usage critical path.

## Scope (harness only — no production crate changes)

- **Verifier-based fidelity gate (the cheap, decisive fix).** Replace the exact-substring
  operative-sentinel gate with running each context's committed deterministic VERIFIER against the
  concatenation of accepted drafts (exactly what Session B's ON arm receives). This is the
  authoritative, tolerant instrument; it recovers quartermaster (its verifier PASSED on the drafts —
  the substring sentinel `100 percent of requirement` was reworded) and ties the gate to what Session
  B actually measures. Pre-registerable as an instrument amendment before any re-run.
- **Task-design fixes.**
  - Remove/raise the dartman solve cap. The 1200s wall-clock guillotine VIOLATES the standing
    no-arbitrary-caps-on-churners rule and confounded the only measured context — drain-until-done
    with a genuine-stuck detector, not a fixed timeout.
  - ezlang's depth-4 sibling is a self-documenting comprehension task the bare agent solved (OFF=win,
    non-discriminating) — replace it with a generative sibling that requires the invented syntax, or
    drop ezlang.
  - Author instruments (verifier ≥5 checks + good/bad fixtures + operative sentinels verified against
    the common context text) for the 3 alternate contexts, so the OFF-pre-gate substitution path is
    real instead of a dead end.
- **Placebo robustness.** Draw placebo mass from fidelity-FAILED scopes too (irrelevant-domain skills
  are valid placebo mass), so the cross-scope control never degenerates to self-donation when few
  contexts pass fidelity (the dartman degeneracy).
- **Systematic-failure circuit breaker.** If the first K contexts fail the SAME gate the SAME way,
  HALT and escalate rather than burning the whole band — per-context continue is right for
  independent noise, wrong for a systematic instrument defect (the band ran 8 doomed contexts when
  context #1's fidelity-RED already predicted the rest).

## Scope Fence

- **CL-bench is a SECONDARY stressor, not the gate.** This ticket does NOT re-run the band on its own
  and produces NO efficacy verdict. Any re-run is T15's optional CL arm, owner-initiated.
- Harness only; no production crate / retrieval / ranking changes. Extraction quality is T24.
- Standing rules: no fakes; measurement drives the real mcp-server over HTTP; auto-gate stays
  clband-* scoped and corpus-safe; human gate untouchable in production.

## Acceptance Criteria

- [ ] Fidelity gate runs the committed verifier against accepted drafts; quartermaster (and any other
      false-negative) is recovered (verifier PASS → MEASURABLE); the gate matches Session B's instrument.
- [ ] dartman solve cap removed/raised to drain-until-done + stuck detector (no arbitrary cap).
- [ ] ezlang sibling replaced (generative, OFF-discriminating) or ezlang dropped, recorded.
- [ ] The 3 alternate contexts have committed instruments (verifier + fixtures + verified sentinels).
- [ ] Placebo never self-donates (draws from a fidelity-failed scope when needed); covered by a test.
- [ ] Circuit-breaker halts the band on K correlated same-gate failures; covered by a test.

## Local Context

- Findings + ready fixtures: `docs/assessments/2026-06-13-t14-clband-band.md` and
  `tests/e2e/reports/efficacy/clband-band/` (per-context drafts, verifier reasons, auto_gate.json).
- The verifier-based gate is the single highest-leverage fix here: it converts at least one context
  (quartermaster) from "excluded" to a real datapoint with zero extraction changes.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
