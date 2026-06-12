# T23 work prompt — automated CL-band run (Batch 18, unattended overnight)

Run `/workflows:work` on ticket
`docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/23-automated-clband-run.md`.
Branch `feat/v-1-7`. Session id suggestion: `work-2026-06-12-t23-band-run`.

## Mission

Execute T14's 8-context CL acquisition band end-to-end, fully automated and unattended, under the
pre-registered auto-gate amendment (Unit 0). The owner sleeps; the run must be complete (or
checkpoint-resumable with a stop reason) by morning. T14 owns the pre-registration and the verdict;
this session owns instruments-at-scale, the orchestrator, the run, and the morning report.

## Read first (in order)

1. The T23 ticket (units, fences, ACs — they are the contract).
2. `docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md` §4 (lifecycle Steps 0–5), §5–6, §8.
3. `docs/assessments/2026-06-12-t14-clband-smoke.md` — T22 RESOLUTION (what is proven) + the
   original NO-GO + addendum (what failed before and why).
4. `docs/execution-sessions/work-2026-06-12-t22-teach-path/` (STATE + unit files) — the smoke
   re-run mechanics you are scaling (run_smoke_rerun.sh, teach_delivery, fidelity_gate, manifest).
5. `tests/e2e/efficacy/clband/` — manifest.json (roster, 8 full contexts + 3 alternates),
   author_smoke_instruments.py, clband_extract.py, run_teach_session.py, efficacy harness entry
   points in `scripts/efficacy_ab.py`.

## Owner decisions ALREADY MADE (do not re-ask)

- GO for the band (2026-06-12).
- Fully automated: NO human gating of band drafts. Auto-accept-all in `clband-*` scopes ONLY,
  via the real rename acceptance path (`SKILL.md.pending` → `SKILL.md`) + real scope rebuild.
  Pre-registered as the Unit 0 amendment BEFORE any band data. Production/dogfood gate unchanged.
- `EXTRACT_TEACH_CAPTURE` stays default-ON (T22 DP-1).
- Smoke contexts are not verdict data; no fresh smoke re-capture required — context #1 of the
  band is the live canary.

## Unattended policy (pre-commit in Unit 0; no STOP-and-ask overnight)

- Harness-level breakage (crash, scope leak, `/health` failure, dataset drift, auto-gate guard
  trip) ⇒ STOP, preserve state + checkpoint, write a stop report for the morning.
- Per-context INSTRUMENT-FAILURE (fidelity RED, or ON-failing-with-verified-injection) ⇒ record
  with taxonomy (extraction vs injection/obedience), no efficacy point for that context, CONTINUE.
- OFF pre-gate passes (non-discriminating sibling) ⇒ drop sibling; context losing all siblings ⇒
  substitute next alternate (the only substitution path).
- NEVER delete run outputs; drain-until-done; no arbitrary time/token caps.

## Hard fences (from the ticket — violations void the run)

- Unit 0 amendment lands BEFORE the first band datum; afterwards nothing changes (criteria,
  roster, instruments, gate policy).
- Auto-accept asserts `clband-*` scope before EVERY rename; fail loud otherwise; unit-tested.
- Operative sentinels verified verbatim against the COMMON context text (hallucinated-sentinel
  lesson); verifier good/bad fixtures must pass before that context runs.
- Injection = focused inject-query mode, labeled; no compile_context claims; no retrieval
  ranking/floor changes (T18/T12 own those).
- Measurement drives the REAL mcp-server over HTTP; gate every window on `/health` 200; pin
  `OLLAMA_EMBED_MODEL=qwen3-embedding:4b`; record solver checkpoint + dataset sha (solver change
  re-runs the OFF pre-gate).
- Heavy actions serialized by the orchestrator; subagents forbidden from cargo build/clippy/test
  and model-call storms; execution agents on sonnet; commit early and often (WSL2).
- Mechanics gotchas: bare `claude --dangerously-skip-permissions` is blocked — drive solves via
  `efficacy_ab.run_claude_solve`; clband extraction needs `GRAPH_BUILDER_GLOBAL_ROOT` +
  `EXTRACT_SESSION_PROVIDER=claude-code`; never truncate graph_state; restart/rebuild scope after
  corpus changes.

## Shape of the night (estimate ~5–9 h serial)

Unit 0 (amendment + policies, commit) → Unit A (8× instruments, commit per context or as one
pre-run commit) → Unit B (orchestrator + auto-gate + scope-guard tests, commit) → Unit C
(context #1 canary, then 2–8; checkpoint commits as contexts complete) → Unit D (morning report:
verdict vs LOCKED pre-reg "ON wins ≥7/10 sign test, no catastrophic regression" with
PASS/FAIL/UNDERPOWERED + per-context INSTRUMENT-FAILURE; secondaries; attribution; gate_mode
labeled; dogfood 262 re-probe; assessment + tickets + index + memory + surgical cleanup).

When deploying subagents, append the project CLAUDE.md "Code Search" (semble) section to their
prompts (repo convention).
