---
unit: "Verifier + rewrite + judge authoring"
unit_number: 1
unit_kind: infra-packet
serves: "committed measurement instruments before any run (pre-reg delta #2)"
status: completed
attempt_count: 1
domains: [efficacy-harness, verifiers, clband]
session_id: work-2026-06-12-clband-smoke
---

## What Was Implemented

Authored + offline-tested the measured instruments for the 2 smoke contexts (2 candidate siblings
each), committed BEFORE any run per pre-registration delta #2.

**Teach tasks** (Session A, Unit 3): flywheel `407f5929` (sibling #1 of 12, whose question lists the
note phrases — fine for teaching); aether `7d9233cf` (the depth-2 `teach_only` fused turn).

**Measured candidate siblings** (de-referenced, committed verifiers):
- flywheel `979ec26a` + `46536e4a` — chosen because their scenarios leak NEITHER invented WORKAROUND
  PROTOCOL (other siblings restate the wobble "firm shake" rule in-question, weakening discrimination;
  these don't). 11 spares remain for OFF-pre-gate rejection.
- aether depth-4 `b0807c2c` (Turbulence-Alert bug review) + depth-6 `4768e426` (Aether→Python).
  Both measured **without prior_turns** — self-contained once the snippet is inlined — so the nested
  reference-answer leakage the plan §4 warns about is eliminated by construction.

**Verifiers (deterministic cores, ≥5 checks, knowledge rubrics only):**
- `flywheel-assembly.sh` (shared across both flywheel siblings; the invented SOP is identical, only
  the scenario differs): 5 checks — wrench "next size up"+"extra torque"; wobble "firm shake"+"retest";
  mandatory spin test (fail if skipped); note→Validation Engineer (Agent C); transport→Forklift (Agent D).
- `aether-turbulence-review.sh`: 6 checks — "Turbulence Alert" + Cause/Fix/Corrected Code subsections;
  `~average <<` (the invented `<<` assignment fixing the planted `=` bug); `outer` kept.
- `aether-python-translate.sh`: positive Python tokens (def/return/for/if/len) present AND every
  Aether token (conduit/flow/swirl/fork/`<<`/`~`/echo) removed.

**De-referencing rule applied:** question copied VERBATIM from the pinned dataset; only a frame
naming the invented system (so ON's `--inject-query summary` retrieval can find the extracted skill)
+ a "write to solution.md" instruction added. NO rule content added. Generated reproducibly by
`author_smoke_instruments.py` (re-run after Unit 4 to fill `corpus_skill_id`).

**Judge prompts:** `judge/<slug>.md` per sibling with VERBATIM CL-bench rubrics (secondary score
only; deterministic core decides pass/fail).

## TDD Evidence (Ralph Red/Green — offline verifier unit tests)
```
flywheel-assembly.sh        flywheel-good    -> exit 0  WIN (5/5 rules)
flywheel-assembly.sh        flywheel-bad     -> exit 1  LOSS (missing 'next size up')
aether-turbulence-review.sh aether-turb-good -> exit 0  WIN (6/6)
aether-turbulence-review.sh aether-turb-bad  -> exit 1  LOSS (no 'Turbulence Alert')
aether-python-translate.sh  aether-tr-good   -> exit 0  WIN
aether-python-translate.sh  aether-tr-bad    -> exit 1  LOSS (no Python 'def')
```
All 4 specs pass the harness CONTRACT schema (`efficacy_ab.py --dry-run --tasks .../clband/tasks`).

## Files Changed
- `tests/e2e/efficacy/clband/verifiers/{flywheel-assembly,aether-turbulence-review,aether-python-translate}.sh`
- `tests/e2e/efficacy/clband/fixtures/**` (good/bad pairs)
- `tests/e2e/efficacy/clband/author_smoke_instruments.py` + `tasks/*.json` + `judge/*.md`

## Orchestration note
Authored directly by the orchestrator (not delegated to execution-agent): the verifier IS the
pre-registered measurement instrument and the discrimination judgment (which checks survive
question-leakage) is the crux of the experiment — round-tripping it through a subagent risks losing
that nuance. Offline tests are non-heavy (bash). Recorded as a deliberate deviation.

## Test Results
- Verifier unit tests: 6/6 PASS. Schema validation: 4/4 PASS. No model calls spent.
