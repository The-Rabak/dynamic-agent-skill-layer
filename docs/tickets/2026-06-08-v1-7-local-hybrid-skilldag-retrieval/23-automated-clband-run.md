---
ticket_id: T23
title: Automated CL-band run — unattended 8-context band under a pre-registered auto-gate amendment
kind: execution
status: ready
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: T14 CL-band plan §4 lifecycle + §8 scaling (docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md) + T22 RESOLUTION (docs/assessments/2026-06-12-t14-clband-smoke.md)"
source_packet_ref: "NEW 2026-06-12 — owner GO on the band (post-T22) + owner directive: the entire band runs automated, unattended, overnight; no manual gating of ~150 benchmark drafts"
feature_home: "tests/e2e/efficacy/clband (band orchestrator + auto-gate) + scripts/ (no production crates)"
depends_on: [T22]
dependency_type: hard
serves:
  - Executes T14's 8-context CL acquisition band end-to-end (T14 owns the pre-registration and the verdict; T23 owns automation + the run + the report)
  - The first paired efficacy data of the project — the verdict the whole V1.7 efficacy chapter exists to produce
files:
  - tests/e2e/efficacy/clband/
  - tests/e2e/efficacy/clband/manifest.json
  - scripts/
  - docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md (pre-reg amendment only)
test_command: "band orchestrator completes Steps 0–5 for all 8 contexts unattended (resumable checkpoints); verdict vs the LOCKED pre-registration; dogfood corpus re-probe reads exactly 262; workspace gates green"
tdd_mode: ralph
---

# Automated CL-band run — unattended, auto-gated, full 8 contexts

## Serves

T22's smoke re-run is GREEN and the owner has called GO. The owner's directive (2026-06-12): the
band must run **fully automated and unattended overnight** — the owner will not manually review
~150 benchmark `.pending` drafts. The human gate stays untouchable in production and for the
dogfood corpus; for the **clband benchmark scopes only**, gating is replaced by a pre-registered
**auto-accept-all** policy, amended into the protocol BEFORE any band data exists (which is the
only window in which amendments are legitimate — after the first paired datum, changes VOID).

Honesty notes carried into the design:
- Auto-accept-all is the *reproducible* gate policy: any selective auto-filter would put an
  unvalidated judge inside the measured pipeline. Accept-all also makes the ON arm face the
  unpruned draft set (possible retrieval dilution) — if ON wins anyway, the result is conservative
  relative to a human-gated production deployment. `gate_mode` is recorded verbatim in the report.
- Acceptance drives the REAL mechanism (rename `SKILL.md.pending` → `SKILL.md`, then the real
  scope rebuild) — never a DB insert or in-process shortcut.

## Scope (four ordered units)

- **Unit 0 — Pre-registration amendment (FIRST commit, before any band run).** Amend the T14
  ticket pre-reg + CL-band plan §4 Step 2: for `clband-*` benchmark scopes only,
  `gate_mode=auto-accept-all` (programmatic rename via the real acceptance action, uniform across
  all contexts and arms, every acceptance logged). Production and dogfood human gates unchanged.
  Also pre-commit: the band roster (8 contexts + 3 alternates, substitution only via OFF
  pre-gate), and the unattended continue/stop policy (see Unit C).
- **Unit A — Instruments at scale (per context, committed BEFORE that context's measured runs).**
  Generalize `author_smoke_instruments.py`: per sibling a deterministic verifier (≥5 checks) with
  good/bad self-test fixtures; pre-registered de-referenced question rewrites; the claude-CLI
  judge prompt with verbatim rubrics (secondary metric); two-tier sentinels with the OPERATIVE
  tier derived from verifier checks AND verified against the COMMON context text (the
  hallucinated-sentinel lesson — fail loud on any sentinel not present verbatim).
- **Unit B — Band orchestrator + auto-gate.** One driver (`run_band.py`) executing Steps 0–5 per
  context, sequentially (heavy actions serialized): OFF pre-gate (`efficacy_ab.run_claude_solve`,
  must FAIL to qualify; alternates substitute only here) → live teach session
  (`run_teach_session.py` + `teach_delivery.materialize` at ingest) → real extraction
  (`clband_extract.py`, `EXTRACT_SESSION_PROVIDER=claude-code`, `GRAPH_BUILDER_GLOBAL_ROOT`) →
  **auto-gate** (rename inside the context's `clband-<name>` scope ONLY — hard scope-guard
  assertion, fail loud on any non-clband path; unit-tested) → scope rebuild + `/health` 200 gate →
  `fidelity_gate.sh` (RED ⇒ record INSTRUMENT-FAILURE(extraction) for that context, no efficacy
  point, CONTINUE the band) → Session B paired ON/OFF/PLACEBO with per-pull attribution
  (focused inject-query mode, labeled). Checkpointed + resumable per (context, step);
  drain-until-done (no arbitrary time/token caps); solver checkpoint + dataset sha pinned and
  recorded; every raw artifact persisted under `tests/e2e/reports/efficacy/clband-band/`.
- **Unit C — The overnight run.** Context #1 is the live canary (first fresh end-to-end pass of
  the post-T22 pipeline). Unattended policy, pre-committed: harness-level breakage (crash, scope
  leak, `/health` failure, dataset-drift) ⇒ STOP and preserve state for morning resume;
  per-context INSTRUMENT-FAILURE ⇒ record and continue (contexts are independent). Then contexts
  2–8. Post-run: dogfood isolation re-probe (corpus reads exactly 262).
- **Unit D — Morning report.** Band verdict vs the LOCKED pre-registration ("ON wins ≥7/10 by
  sign test, no catastrophic regression"; outcomes PASS/FAIL/UNDERPOWERED with per-context
  INSTRUMENT-FAILURE taxonomy extraction-vs-injection/obedience). Pre-registered secondaries:
  paired turns-to-solve, token cost, judge-rubric score, ON-vs-PLACEBO stated explicitly.
  Attribution logs, `gate_mode` labeled, dogfood-isolation evidence, all numbers artifact-backed.
  Assessment doc + T14/T23/index/memory closeout.

## Scope Fence

- **Auto-accept ONLY under `clband-*` scopes.** Hard assertion before every acceptance; fail loud
  otherwise. The production human gate and the 262 dogfood corpus are untouched (post-run re-probe
  is an acceptance criterion).
- **Amendment window:** Unit 0 lands before the first band datum; afterwards NOTHING about
  criteria, roster, instruments, or gate policy changes (changes VOID the run).
- **No retrieval ranking/floor changes** (T18/T12 own those). Injection mode = focused
  inject-query, labeled; no `compile_context` claims.
- **Smoke contexts (flywheel, aether) are NOT verdict data** — pipeline validation only, per the
  locked pre-reg.
- Standing law: no fakes/stubs — fail loud; measurement drives the REAL mcp-server over HTTP;
  heavy actions serialized by the orchestrator (subagents forbidden from cargo build/clippy/test
  and model-call storms; execution agents on sonnet); never delete this run's outputs; never
  truncate graph_state; workspace gates stay green.

## Acceptance Criteria

- [ ] Unit 0 amendment committed BEFORE any band run; the report cites `gate_mode` verbatim.
- [ ] Per-context instruments committed before that context's measured runs; every operative
      sentinel verified verbatim against the common context text; verifier good/bad fixtures green.
- [ ] OFF pre-gate raw outputs persisted for every measured sibling; substitutions only via the
      pre-gate; solver checkpoint + dataset sha recorded.
- [ ] Auto-gate log lists every acceptance with its scope assertion; zero non-clband paths
      touched; scope-guard unit tests green.
- [ ] Fidelity gate per context before any measured solve; RED ⇒ INSTRUMENT-FAILURE(extraction)
      recorded, that context contributes no efficacy point.
- [ ] Session B ran all three arms paired per surviving sibling with per-pull attribution.
- [ ] Band verdict vs the locked pre-registration with secondaries; every number artifact-backed
      under `tests/e2e/reports/efficacy/clband-band/`.
- [ ] Interrupted runs resume from the last (context, step) checkpoint without re-burning
      completed work (demonstrated or unit-tested).
- [ ] Dogfood corpus re-probe reads exactly 262 post-run; workspace gates green.

## Local Context

- GO evidence: `docs/assessments/2026-06-12-t14-clband-smoke.md` T22 RESOLUTION (smoke re-run
  GREEN: flywheel 7/7, aether 5/5 operative; dogfood regression CLEAN; `EXTRACT_TEACH_CAPTURE`
  default-ON owner-approved).
- Acceptance mechanism: a draft is accepted iff `SKILL.md.pending` is renamed to `SKILL.md`
  (structural definition in `scripts/efficacy_draft_acceptance.py`); rejected tombstones exist
  (`writer.rs`); the rebuild ingests accepted skills per scope.
- Per-context cost (from the smoke): OFF pre-gate ~2–4 solves + 1 teach session + 1 extraction
  cycle + 3 arms × 1–2 siblings ≈ 8–12 model sessions; ×8 contexts ≈ 60–90 sessions serial
  (~5–9 h) — overnight-feasible, which is why resumability is an AC, not a nice-to-have.
- Known open hazards inherited: verbose-prompt priming no_match (T18/T12; why injection is
  focused-mode labeled); preamble-drop latent bug (filed follow-up, off critical path);
  draft-count inflation under the taught-knowledge class (19/context at the smoke re-run —
  watch retrieval dilution in ON attribution).
- Publishability note (NOT this ticket's scope): a full-benchmark §8 run adds an in-window
  reference arm (the paper's native setting) and CL-bench-native all-rubrics judge scoring as
  primary; tonight's band is the internal efficacy gate, not a benchmark claim.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Band protocol: `docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
