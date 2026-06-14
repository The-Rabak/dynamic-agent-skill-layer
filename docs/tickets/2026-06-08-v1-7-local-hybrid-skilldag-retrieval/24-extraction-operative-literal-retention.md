---
ticket_id: T24
title: Extraction operative-literal retention — keep contract-bearing literals verbatim in selected skills
kind: expansion
status: ready
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: T22 RESOLUTION + T23 band report (docs/assessments/2026-06-13-t14-clband-band.md §binding-constraint)"
source_packet_ref: "NEW 2026-06-13 — from the T23 band INSTRUMENT-FAILURE root-cause (session-distillation drops un-operationalized precise literals) + owner steer: lightweight + general, NOT a doc-ingestion mode, NOT the focus"
feature_home: "crates/infrastructure/src/extraction (prompt contract) + crates/session-extractor (optional post-extraction lint)"
depends_on: []
dependency_type: none
serves:
  - General extraction quality — a skill that carries a rule must carry the rule's contract-bearing literals (enum values, status/error codes, numeric thresholds, version pins, flag names) verbatim, because using the wrong one is a real bug in real code
files:
  - crates/infrastructure/src/extraction/
  - crates/session-extractor/
  - tests/e2e/reports/efficacy/
test_command: "dogfood regression CLEAN (re-extract 2-3 organic sessions, no degradation) + literal-retention fixtures from the T23 band's dropped tokens pass"
tdd_mode: ralph
---

# Extraction operative-literal retention

## Serves

The T23 band root-caused its INSTRUMENT-FAILURE to a precise mechanism (verified in the actual drafts,
`docs/assessments/2026-06-13-t14-clband-band.md`): **session-distillation preserves the literals the
capturing session OPERATIONALIZED and drops the ones it merely referenced.** material-handler kept
`50 lb (Rule 7)` verbatim (the teach solve did weight arithmetic with it) but dropped `<1 megaohm`
(the wrist-strap rule was kept as a list item; its threshold was never exercised, so it was never
elevated into a skill body — zero occurrences across 25 drafts).

This is NOT a CL-bench artifact and NOT an injected-document problem. The same class bites real
coding work: exact enum values (`MATCH|MISMATCH|MISSING`), status/error codes (`M-WARN-01`,
`HOLD_CRITICAL_LOW`), numeric thresholds/floors (`40% RH`, a `30s` timeout), version pins, and flag
names. Using the wrong one is a real bug. T22's abstraction-exception clause asks the model to keep
such literals verbatim, but it operates at the PLACEHOLDERING step — it never fires when the literal
isn't SELECTED into a skill body in the first place. This ticket closes that upstream gap.

**Scope discipline (owner steer 2026-06-13):** lightweight and general; this is an ADDITIONAL step,
explicitly NOT the project's focus. The focus is the real-usage retrieval/compounding spine
(T18 → T12 → T15). Do NOT build a document-ingestion / verbatim-doc-as-skill mode here — that would
over-fit the CL-bench injected-doc regime, which is not the real use case.

## Scope (small, additive, default-on behavior)

- **Operative-vs-incidental literal distinction in the extraction prompt.** Sharpen T22's exception:
  - INCIDENTAL literals (repo paths, ticket ids, ephemeral values) — keep abstracting into
    `{{placeholders}}` for transfer (unchanged; this is what makes organic skills reusable).
  - OPERATIVE literals (enum members, status/error codes, numeric thresholds + units, version pins,
    named flags/keywords a future task must reproduce EXACTLY) — retain VERBATIM in the view that
    carries the rule (`procedures` / `use_when` / `evidence`), even when the capturing session did
    not compute with them. A rule that names a threshold without its value is half a skill.
- **Optional post-extraction lint (not a hot-path LLM call).** A cheap structural check: if a draft's
  body states a rule whose `evidence` span contains an operative literal (regex classes:
  number+unit, `[A-Z][A-Z0-9_]{3,}` codes, quoted status strings, `vX.Y.Z` pins) but the body drops
  it, flag the draft. Surface as a quality signal in the extraction report / dogfood gate — NOT a
  runtime gate, NOT auto-rejection.

## Scope Fence

- **Not a doc-ingestion mode.** No per-document structured extraction, no verbatim-doc-as-skill. This
  is retention WITHIN skills the extractor already chose to keep.
- **Hard dogfood-regression gate (same as T22):** re-extract 2-3 known organic sessions OFF vs ON;
  draft count + quality must not degrade. Over-retention that drags incidental repo literals back in
  (breaking organic transfer) is a FAIL. The 262-corpus prompt must not regress.
- Default-on behavior; if staging is needed, env-gate fail-loud — but the target is default-on after
  the regression diff (owner approves), same pattern as `EXTRACT_TEACH_CAPTURE`.
- No fakes; fail loud. No retrieval/ranking/floor changes (T18/T12 own those).

## Acceptance Criteria

- [ ] Extraction prompt draws the operative-vs-incidental literal distinction; operative literals are
      retained verbatim in the rule-bearing view even when un-operationalized in the session.
- [ ] Literal-retention fixtures pass: re-extracting the T23 band's five genuine-gap contexts retains
      the named tokens (`<1 megaohm`, `MATCH|MISMATCH|MISSING`, the 40% RH floor, `diagnosis`/
      `prognosis`, `M-WARN-01`) in at least one accepted draft. (These are ready acceptance fixtures.)
- [ ] Optional lint implemented and reported (if included); no hot-path LLM call.
- [ ] Dogfood regression diff CLEAN (2-3 organic sessions; no count/quality degradation; incidental
      literals still abstracted; diff persisted).
- [ ] Workspace gates green; assessment/memory updated.

## Local Context

- Root-cause evidence: `docs/assessments/2026-06-13-t14-clband-band.md` (§ "The binding constraint")
  and the material-handler drafts under `tests/e2e/reports/efficacy/clband-band/`.
- Relationship to T22: T22 made the extractor ACCEPT taught one-shot knowledge (value system) and
  stop placeholdering taught literals; T24 makes it RETAIN operative literals at selection time. T22
  is the why-capture-it fix; T24 is the keep-it-precise fix.
- Why lightweight: real taught knowledge is smaller, operative, and COMPOUNDING (a value that matters
  gets operationalized in some future session and captured then); the one-shot buried-value regime
  CL-bench stresses is the adversarial worst case, not the common case.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
