---
ticket_id: T22
title: Teach-path extraction — capture taught knowledge verbatim (unblocks the T14 CL band)
kind: expansion
status: done  # 2026-06-12 — all 4 units delivered; smoke re-run GREEN (GO gate); GO recommended (owner reviewing)
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: T14 CL-band plan §4 (docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md) + smoke NO-GO (docs/assessments/2026-06-12-t14-clband-smoke.md incl. 2026-06-12 addendum)"
source_packet_ref: "NEW 2026-06-12 — from the clband smoke INSTRUMENT-FAILURE(extraction) + the follow-up three-component re-diagnosis"
feature_home: "crates/session-extractor + crates/infrastructure extraction prompt contract (+ tests/e2e/efficacy/clband harness for delivery/gate fixes)"
depends_on: []
dependency_type: none
serves:
  - The loop's ability to be TAUGHT — capture of one-shot, document-grounded, idiosyncratic knowledge (the CL-bench thesis applied to OUR extraction)
  - Unblocks the T14 full 8-context acquisition band (GO gate = smoke re-run green)
files:
  - crates/session-extractor/
  - crates/infrastructure/src/extraction/
  - tests/e2e/efficacy/clband/
  - tests/e2e/efficacy/clband/manifest.json
  - scripts/
test_command: "tests/e2e/efficacy/clband/fidelity_gate.sh green on a re-run of BOTH smoke contexts (operative-tier sentinels) + dogfood extraction regression diff clean"
tdd_mode: ralph
---

# Teach-path extraction — capture taught knowledge verbatim

## Serves

The clband smoke proved the teach-session protocol works end-to-end EXCEPT at extraction: both smoke
contexts hit INSTRUMENT-FAILURE(extraction) — and the follow-up forensic re-read of the worker logs
split the single "extractor design blocks invented rules" headline into **three separable
components** (see the assessment's 2026-06-12 addendum):

1. **Plumbing (missed by the smoke report):** the extraction-input sanitizer logged
   `transcript entry dropped: speaker matched suspicious-speaker filter (system impersonation)` on
   EVERY window of both contexts. Flywheel's knowledge document lives in the system prompt
   (`knowledge_home=system`) — the prose extractor likely never saw the rule document at all; its
   "lessons are embedded in instructions" dismissals were rendered over a stripped transcript.
2. **Worldview (verified verbatim, survives visibility):** for aether the spec was a workspace FILE
   the agent read, and the extractor still refused with reasoned assessments — "nothing would recur
   on a future, different task", "no failure/fix cycle, no iteration". The lesson extractor's value
   system demands recurrence + discovery-through-failure; taught knowledge has neither. This is a
   PRODUCTION gap, not a benchmark quirk: a user teaching their org's conventions in a real session
   hits the same wall.
3. **Gate leveling:** the capture that DID happen (11 drafts) came through the preference/convention
   detector and preserved invented operative specifics VERBATIM ("M8x20 misprinted label — verify
   with ruler", "batch FW-2025-0118", "torque callout sketch v2") — an existence proof that verbatim
   one-shot capture is already in the system's repertoire. The fidelity gate failed on
   DOCUMENT-level sentinels (system names) the preference channel was never going to emit; the
   Session B verifiers need OPERATIVE-level rules.

## Scope (four ordered units)

- **Unit A — Forensics first (no fixes).** Replay extraction-input construction over the CAPTURED
  smoke transcripts (`tests/e2e/reports/efficacy/clband-smoke/transcripts/`) and produce an exact
  **visibility map**: what the suspicious-speaker filter dropped (count + content class per window),
  whether the flywheel SOP ever reached any window, and whether tool-read file contents (aether's
  spec) enter extraction windows at all. This apportions blame between Units B and C and is the
  evidence base for both.
- **Unit B — Harness-side document delivery (no extractor changes).** Make the teach-session
  materialization deliver the knowledge document in an extraction-visible form (user-turn content
  and/or confirmed tool-result visibility) — expected home: `tests/e2e/efficacy/clband/
  run_teach_session.py` + the capture path. Do NOT weaken the suspicious-speaker filter (it is
  injection defense). Re-extract from re-captured (or replayed) teach sessions; re-gate.
- **Unit C — Taught-knowledge capture (the core).** Extraction-prompt change in the real pipeline:
  **"taught knowledge" becomes a first-class candidate class** — when a session contains a document
  or user teaching a system/convention/procedure, idiosyncratic constants/names/codes/procedures are
  captured verbatim; recurrence is NOT required for taught material; "explicitly stated in the
  instructions/document" is a capture trigger, not a rejection reason. Plus a retry-semantics fix:
  a reasoned refusal (assessment + zero candidates) is a distinct outcome from malformed output —
  stop burning 3 identical retries on deliberate refusals.
- **Unit D — Gate re-leveling + smoke re-run (the GO gate).** Two-tier sentinels in
  `tests/e2e/efficacy/clband/manifest.json` (document-level names reported, OPERATIVE-level
  rules gating; operative sentinels derived from each context's verifier checks), then re-run the
  FULL smoke (both contexts, Steps 1–4, reusing the committed instruments; `fidelity_gate.sh` is
  the acceptance test). Green → recommend GO for the 8-context band (owner decides).

## Scope Fence

- **No benchmark special-casing.** The taught-knowledge class ships as production behavior,
  motivated on product grounds (users teach agents conventions — that is the layer's job). If it
  needs staging, env-gate it fail-loud (`EXTRACT_TEACH_CAPTURE`), but the target end-state is
  default-ON after the regression gate passes — owner approves the default.
- **Dogfood regression gate (hard):** before the taught-knowledge prompt change can merge, re-extract
  2–3 known sessions from the 24-session organic corpus and diff draft count/quality against their
  known-good outputs. The same prompt produces the 262 corpus; degrading it to pass CL is forbidden.
- Do NOT weaken or special-case the suspicious-speaker injection defense; fix delivery, not the filter.
- No auto-approval anywhere; re-run drafts go through the owner gate.
- No efficacy verdict from the re-run smoke (it remains pipeline validation; the pre-registration is
  untouched).
- No fakes/stubs; fail loud (machine-wide rule). No crates/retrieval ranking changes (T18/T12 own that).

## Acceptance Criteria

- [x] Unit A visibility map persisted (per-window dropped-content accounting for both smoke
      transcripts). Flywheel-SOP-never-seen hypothesis **refuted** (doc mostly visible 8/9; failure =
      worldview) for flywheel, **confirmed** for aether (visibility). Artifacts: `tests/e2e/reports/
      efficacy/clband-smoke/visibility/` (commit `671b412`).
- [x] Teach-session delivery fixed (`teach_delivery.materialize()` injects doc as a user turn); replay
      proof shows window content contains the document text — flywheel 8/9→9/9, aether 4/8→6/8 operative
      visible (commit `360f7cd`).
- [x] Taught-knowledge candidate class implemented in the real prompt path (`EXTRACT_TEACH_CAPTURE`,
      default ON); reasoned refusals no longer retried (assessment-threaded) (commit `f1647d5`).
- [x] Dogfood regression diff clean (3 organic sessions; draft delta +1 total, quality equivalent;
      `tests/e2e/reports/efficacy/dogfood-regression/ANALYSIS.md`).
- [x] Two-tier sentinels in the manifest; operative tier derived from verifier checks; gate gates on
      operative, reports document (Unit D).
- [x] **Smoke re-run green:** both contexts pass the operative fidelity gate (flywheel 7/7, aether 5/5;
      `fidelity_gate.sh` exit 0). GO recommendation recorded; raw artifacts `tests/e2e/reports/efficacy/
      clband-rerun/`. (Owner holding the band-launch decision pending review.)
- [x] All claims artifact-backed; assessment updated (T22 RESOLUTION section); workspace gates green
      (fmt + clippy bare + clippy --features test-utils). DP-2: drafts left as .pending evidence.

## Local Context

- Smoke evidence: `docs/assessments/2026-06-12-t14-clband-smoke.md` (+ addendum),
  `tests/e2e/reports/efficacy/clband-smoke/` (transcripts, worker logs with verbatim refusals,
  fidelity result, 11 preference-channel drafts).
- Mechanics from the smoke session (memory `v17-t14-clband-smoke-extraction-blocker`): clband
  extraction driver `clband_extract.py` (isolated scope ingest → host maintenance-worker
  RUN_ONCE+TRANSCRIPT_DRAIN, `EXTRACT_SESSION_PROVIDER=claude-code`, needs
  `GRAPH_BUILDER_GLOBAL_ROOT`); drive solves via `efficacy_ab.run_claude_solve` (bare
  `claude --dangerously-skip-permissions` is blocked by the auto-mode classifier).
- Related dream contract: DS-025 one-shot acquisition seeds skills directly and therefore could not
  catch this — this ticket closes the gap between the dream contract and the real loop.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
