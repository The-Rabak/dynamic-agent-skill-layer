# T14 CL Acquisition Band — SMOKE assessment (2026-06-12)

**What this is.** The pipeline-validation smoke for the T14 CL acquisition band (plan §5): the two
smoke contexts (`flywheel-assembly-agent` 4.2k, `aether-language` 33.5k) run through the full
teach-session lifecycle (OFF pre-gate → teach Session A → real extraction → fidelity gate). It
**gates the full 8-context band**.

**This is explicitly NOT efficacy data.** Per the LOCKED pre-registration, no PASS/FAIL/UNDERPOWERED
efficacy verdict may be — or is — claimed from this smoke. The pre-registered criterion stays
untouched and unscored:

> "ON wins ≥ 7 of 10 paired tasks by sign test, with no catastrophic regression on any single task."

**Solver checkpoint:** `claude-code 2.1.173, --model sonnet`. **Dataset:** `tencent/CL-bench` sha
`b28a5832a09b0d96c0cf4c22e90d7c60ede25b80` (fetch re-verified, no drift; sentinels OK both contexts).
**Session:** `work-2026-06-12-clband-smoke` (commits `0e38875`, `dfe34ee`, `1502c16`, `be95a23`, + this).

---

## Headline finding (P0): the teach-session protocol is blocked at EXTRACTION, at both sizes

Both smoke contexts **FAIL the fidelity gate → INSTRUMENT-FAILURE(extraction)**. The blocker is the
extractor's **recurrence + generalization design**, not context size and not the injection path.

| context | size | knowledge_home | drafts | sentinels present | fidelity |
|---|---|---|---|---|---|
| flywheel-assembly-agent | 4.2k | system | 11 | **0/4** | **FAIL** |
| aether-language | 33.5k | user (fused) | **0** | 0/4 | **FAIL** |

Two distinct failure mechanisms — both rooted in the same design:

1. **aether → outright refusal.** The extractor's salience assessment, logged verbatim
   (`logs/worker-aether-language-200722-s1.log`):
   > *"candidate_count: 0 … The session is a one-off creative translation exercise for a fictional
   > programming language ('Aether') defined in a local spec file. There is no real toolchain, no
   > failure/fix cycle, no iteration, and the 'language' has no existence outside this spec. Nothing
   > in the session would recur on a future, different task."*

   The "would this recur on a future task?" grounding gate is **designed** to reject one-off /
   non-transferable knowledge. An invented language is its textbook reject case → 0 drafts.

2. **flywheel → abstraction.** 11 drafts were produced, but the dual-pass generalization converted
   the invented SOP into **generic real-world principles** and dropped every invented specific:
   - kept (examples): `Physically verify specifications before installation — never trust labels
     alone` [best_practice]; `Preserve existing integrity — never modify pristine components`
     [principle]; `Always trace every decision to a named authoritative artifact` [principle]; plus
     scenario preferences (`Use batch FW-2025-0118`, the `M8x20` misprint).
   - stripped: the system name **Flywheel Manufacturing Multi-Agent System**, the **Scatterbrained
     Improviser** persona, both **WORKAROUND** protocols (wrench → next size up + extra torque;
     wobble → firm shake + retest), and the mandatory **spin test** gate. → **0/4 sentinels**.

   The invented procedures were *present and used* in the teach session's `solution.md` (verified:
   `next size up`, `firm shake`, `retest`, `spin test`) — extraction then generalized them away.

**Why this matters more than the size hypothesis.** The plan (§5, §9) anticipated a *size threshold*
("pass at 4.2k, fail at 33.5k — a publishable extraction finding"). The smoke **falsifies that
framing**: the small context also failed, by abstraction rather than refusal. The binding constraint
is the salience/generalization design that makes the extractor *good at organic dogfood corpus-building*
— it distills transferable skills and discards fictional specifics. The teach-session protocol needs
the opposite: faithful capture of invented, non-recurring specifics. These goals are in direct tension.

---

## Per-lifecycle results

### Step 0 — OFF pre-gate (discrimination): PASS for all 4 candidate siblings
Bare-agent OFF solves (no layer, no context) against the committed deterministic verifiers. **All 4
OFF = loss** (`reports/efficacy/clband-smoke-offpregate/report.json`):

| sibling | OFF | deterministic reason |
|---|---|---|
| flywheel 979ec26a | loss | missing wrench workaround 'next size up' |
| flywheel 46536e4a | loss | missing wrench workaround 'next size up' |
| aether b0807c2c (depth-4) | loss | missing the invented 'Turbulence Alert' section |
| aether 4768e426 (depth-6) | loss | Aether 'conduit' still present — not translated to def |

0 siblings rejected; the aether mechanism is exactly as predicted (OFF does not recognize `=` as a
bug, so it never emits a Turbulence Alert). **The smoke proves discrimination is real and craters
where the earlier self-authored battery did not** — the invented specifics are genuinely not
producible without the rule. (This is the positive half of the smoke.)

### Step 1 — Session A teach sessions: PASS (both genuine)
Two real claude-code sessions (sonnet, serialized) each worked the teach task USING its knowledge
document. Both wrote `solution.md` with the invented rules verifiably in use; transcripts captured
(`reports/efficacy/clband-smoke/transcripts/`): flywheel 65.8 kB, aether 114.5 kB.

### Step 2–3 — Pipeline + fidelity gate: FAIL (the headline, above)

### Step 4–5 — Session B paired ON/OFF/PLACEBO: NOT RUN (cannot, by construction)
With no rule-bearing skills in either scope, there is nothing to inject; no ON/OFF/PLACEBO efficacy
reading is obtainable. Skipped, not failed.

---

## Provenance / fences (all honored)
- **No efficacy verdict** claimed (smoke ≠ efficacy data).
- **No planting**: the rule never entered the corpus except via session-capture → extraction; when
  extraction dropped it, the answer was *report it*, never hand-edit a skill into shape.
- **No crate / ranking / floor changes** (the verbose-prompt fix remains T18/T12; the smoke used the
  labeled focused inject-query workaround — moot, as Session B did not run).
- **Scope isolation (DP-2):** verified live before any data — a `compile_context` query scoped to an
  empty clband subdir returned `no_match`/0 skills (zero dogfood leak); broad `/skills/project`
  returned dogfood. Mechanics recorded in `unit-00-preflight.md`. The container `/skills/project`
  was never populated with clband skills (Session B cancelled); closeout removed the empty clband
  scaffolding and re-probed — the 262 dogfood corpus is pristine and uncontaminated.

## Draft-acceptance (T14 ≥10-real-drafts AC)
The flywheel teach session produced **11 real `.pending` drafts** from a real captured session
(`draft_acceptance.json`). Per owner direction (2026-06-12) these were accepted (`.pending` →
`SKILL.md`) as ≥10-real-drafts AC evidence, **explicitly labeled non-faithful-for-clband** (0/4
sentinels). Acceptance here reflects the drafts' plausibility as general skills — a measure SEPARATE
from the clband fidelity gate, which FAILED.

---

## GO / NO-GO recommendation for the full 8-context band

**Recommendation: NO-GO** for the full 8-context acquisition band as currently wired.

**Reasons:**
1. All 8 full contexts are invented-knowledge contexts (Game Mechanics, Programming Syntax, Math
   Formalism, Tech Standards, Legal/Reg, Operational/Workflow Procedures). The extractor's
   salience/generalization filter that blocked both smoke contexts would block them identically —
   either by refusal (fiction with no toolchain) or by abstraction (invented specifics → generic
   principles). Running 8 more would reproduce this finding 8 times at 8× the cost.
2. The blocker is upstream of everything the band is meant to measure. Until extraction can faithfully
   carry invented specifics, there is no rule-bearing skill to inject, so no efficacy data point is
   obtainable regardless of retrieval, injection, or scoring quality.

**Gating work before any full run (proposed follow-up — a new ticket / T18-adjacent):**
- A **teach-mode / verbatim extraction path** for the acquisition-band protocol that bypasses or
  inverts the recurrence ("would this recur?") and generalization ("distill to a transferable
  principle") gates when the input is a designated teaching context — capturing the invented rule's
  literal specifics (names, codes, procedures, sentinels) instead of abstracting them away. The
  fidelity gate built this batch (`fidelity_gate.sh`) is the ready acceptance test for that path.
- Re-run THIS smoke (both contexts) against that path as the GO gate; only then run the 8.

**What the smoke positively established (carry forward):**
- The OFF pre-gate + deterministic verifiers discriminate cleanly on genuinely novel CL-bench rules
  (the gap the self-authored battery had). The instrument design is sound.
- Scope isolation via marker subdirs + scoped `compile_context` works and is dogfood-safe.
- The teach→capture half of the pipeline works (genuine sessions, faithful `solution.md`); the break
  is precisely and only at extraction-salience.

---

## Artifacts (every number above traces to one of these)
- `tests/e2e/reports/efficacy/clband-smoke-offpregate/{report.json,report.txt}` — OFF pre-gate.
- `tests/e2e/reports/efficacy/clband-smoke/transcripts/` — 2 teach transcripts + 2 teach solutions.
- `tests/e2e/reports/efficacy/clband-smoke/extract_{flywheel-assembly-agent,aether-language}.json`,
  `logs/worker-*.log` — extraction (the aether refusal assessment is verbatim in its log).
- `tests/e2e/reports/efficacy/clband-smoke/fidelity_gate_result.txt` — sentinel coverage (0/4, 0/4).
- `tests/e2e/reports/efficacy/clband-smoke/scopes/clband-flywheel-assembly-agent/.skills/` — 11
  accepted (labeled non-faithful) skills; `draft_acceptance.json`.
- `docs/execution-sessions/work-2026-06-12-clband-smoke/` — full per-unit session log + STATE.
- Instruments (committed pre-run): `tests/e2e/efficacy/clband/{verifiers,tasks,judge,fixtures}/`.
