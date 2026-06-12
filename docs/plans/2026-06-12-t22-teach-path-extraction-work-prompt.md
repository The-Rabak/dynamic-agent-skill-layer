# /workflows:work prompt — Batch 17: T22 teach-path extraction (2026-06-12)

Paste everything below the line into the next `/workflows:work` session.

---

## Mission

Execute **T22 — teach-path extraction** (ticket:
`docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/22-teach-path-extraction.md`, read it
in full FIRST — its four units A–D are the work plan). Goal: make the real extraction pipeline able
to capture **taught knowledge** (one-shot, document-grounded, idiosyncratic rules) verbatim, then
re-run the clband smoke as the **GO gate** for the T14 8-context acquisition band. This is the
critical path: the whole efficacy chapter is blocked behind it.

## The evidence base (do NOT re-derive; verify by reading)

The clband smoke (session `work-2026-06-12-clband-smoke`, report
`docs/assessments/2026-06-12-t14-clband-smoke.md` **including the 2026-06-12 addendum**) hit
INSTRUMENT-FAILURE(extraction) at BOTH context sizes. The addendum's forensic re-read split the
failure into three components — your units map onto them:

1. **Sanitizer drops (Unit A/B):** worker logs show `transcript entry dropped: speaker matched
   suspicious-speaker filter (system impersonation)` on EVERY extraction window of both contexts
   (`tests/e2e/reports/efficacy/clband-smoke/logs/worker-*.log`). Flywheel's knowledge document
   lives in the SYSTEM prompt → the prose extractor plausibly never saw the rules at all.
2. **Extractor worldview (Unit C):** verbatim refusals even where the document WAS visible (aether
   spec was a workspace file): "nothing would recur on a future, different task", "no failure/fix
   cycle". The lesson extractor demands recurrence + discovery-through-failure; taught knowledge has
   neither. Production-relevant: a user teaching org conventions hits the same wall.
3. **Gate mis-leveling (Unit D):** the 11 drafts that DID emerge came via the preference/convention
   detector and preserve invented operative specifics VERBATIM ("M8x20 misprinted label — verify
   with ruler", "batch FW-2025-0118", "torque callout sketch v2") — proof verbatim capture is
   achievable. The fidelity gate failed on DOCUMENT-level sentinels (system names) that channel
   never emits; Session B verifiers need OPERATIVE-level rules.

Smoke positives to reuse, not rebuild: OFF pre-gate discriminates (4/4), scope isolation
(marker-subdir + scoped compile_context) is dogfood-safe, instruments are committed under
`tests/e2e/efficacy/clband/` (`verifiers/`, `fidelity_gate.sh`, `clband_extract.py`,
`run_teach_session.py`, `tasks/`, `judge/`, `fixtures/`).

## Work units (in order)

**Unit A — Forensics (no fixes yet).** Replay extraction-input construction over the CAPTURED smoke
transcripts (`tests/e2e/reports/efficacy/clband-smoke/transcripts/*.jsonl`) and persist a
**visibility map**: per window, what the suspicious-speaker filter dropped (count + content class),
whether the flywheel SOP text ever reached any window, whether aether's tool-read spec content
enters windows. Home of the filter: `crates/infrastructure/src/extraction/` (prompt contract).
Deliverable: a JSON/MD artifact under `tests/e2e/reports/efficacy/clband-smoke/` + a verdict
confirming or refuting "flywheel document never seen". Historical rhyme to check against: the old
"sanitizer drops assistant turns" P0 (memory `extraction-drops-assistant-turns-bug`).

**Unit B — Document delivery (harness-side, no extractor changes).** Fix
`tests/e2e/efficacy/clband/run_teach_session.py` / the capture path so the knowledge document
reaches extraction in a legitimate, extraction-visible form (user-turn content and/or confirmed
tool-result visibility). **Do NOT weaken the suspicious-speaker filter** — it is injection defense;
fix delivery, not the defense. Evidence: re-extracted window contents demonstrably contain document
text.

**Unit C — Taught-knowledge candidate class (the core; real pipeline, production-motivated).**
Extraction-prompt change in the real path (`crates/session-extractor` +
`crates/infrastructure/src/extraction/`): when a session contains a document or user TEACHING a
system/convention/procedure, capture idiosyncratic constants/names/codes/procedures **verbatim**;
recurrence is NOT required for taught material; "explicitly stated in the document" is a capture
trigger, not a rejection reason. Also: a reasoned refusal (assessment + zero candidates) is a
distinct outcome from malformed output — stop the 3× identical-retry burn on deliberate refusals.
**HARD GUARDRAIL — dogfood regression gate:** before this merges, re-extract 2–3 known sessions
from the 24-session organic corpus and diff draft count/quality vs known-good output; the same
prompt produces the 262 corpus and may not degrade. If staging is needed, env-gate fail-loud
(`EXTRACT_TEACH_CAPTURE`); the intended end-state is default-ON — **owner approves the default**
(decision point).

**Unit D — Two-tier sentinels + smoke re-run (the GO gate).** Amend
`tests/e2e/efficacy/clband/manifest.json`: per context, `sentinels_document` (current system-name
tier, reported) + `sentinels_operative` (derived from each context's verifier checks — the
constants/rules Session B actually needs; gating tier). Update `fidelity_gate.sh` accordingly.
Then **re-run the full smoke** (both contexts, teach → extract → owner-gated acceptance →
fidelity gate; reuse all committed instruments; per the smoke session's mechanics notes drive
solves via `efficacy_ab.run_claude_solve`, extraction via `clband_extract.py` with
`EXTRACT_SESSION_PROVIDER=claude-code` + `GRAPH_BUILDER_GLOBAL_ROOT` set). Optionally proceed to a
Session B ON-arm probe on one surviving sibling as a bonus sanity check (labeled diagnostic, no
efficacy claim). Deliverable: GO/NO-GO **recommendation** for the 8-context band — the owner
decides.

**Closeout:** assessment update (`docs/assessments/` — new doc or addendum), T14 + T22 tickets,
index Batch 17 status, memory, surgical cleanup, commits per unit (conventional commits).

## Hard fences

- **No benchmark special-casing**: Unit C ships as production behavior justified on product grounds;
  degrading organic extraction to pass CL is forbidden (regression gate is hard).
- **No weakening the suspicious-speaker injection defense.**
- **No auto-approval** of `.pending` drafts — owner gate (decision point).
- **No efficacy verdict** from the re-run smoke; pre-registration untouched.
- **No crates/retrieval ranking/floor changes** (T18/T12 own those).
- **Scope isolation**: clband scopes must not contaminate the 262 dogfood corpus (re-probe after).
- Standing rules: no fakes/stubs — fail loud; measurement drives the REAL stack over HTTP; heavy
  actions SERIALIZED by the orchestrator (subagents forbidden from cargo build/clippy/test and
  model-call storms); execution agents on sonnet; never truncate graph_state; no arbitrary caps on
  churners; never delete this session's outputs (cleanup = build artifacts + STALE scratch only);
  workspace gates (clippy both forms + fmt) stay green.

## Owner decision points (STOP and ask)

1. Unit C default: `EXTRACT_TEACH_CAPTURE` default-ON vs env-gated, after the regression diff is in.
2. Human gate on re-run `.pending` drafts (present with operative-sentinel coverage summary).
3. GO/NO-GO for the 8-context band (recommend with evidence; owner decides).
4. If Unit A refutes the document-visibility hypothesis AND Unit C alone still fails fidelity —
   stop and present options before inventing new mechanism.

## Done means

- [ ] Visibility map persisted; flywheel-document hypothesis confirmed/refuted with evidence.
- [ ] Document delivery fixed; window contents verifiably include the document.
- [ ] Taught-knowledge class implemented; refusal≠malformed retry fix in; unit tests green.
- [ ] Dogfood regression diff persisted and clean (2–3 organic sessions).
- [ ] Two-tier sentinels in manifest + gate; operative tier derived from verifiers.
- [ ] Smoke re-run: both contexts through the full lifecycle; `fidelity_gate.sh` verdicts recorded
      with raw artifacts; GO/NO-GO recommendation written.
- [ ] Assessment + tickets + index + memory updated; gates green; commits clean; surgical cleanup.
