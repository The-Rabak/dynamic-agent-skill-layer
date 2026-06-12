# /workflows:work prompt — Batch 16 continuation: T14 CL acquisition band SMOKE (2026-06-12)

Paste everything below the line into the next `/workflows:work` session.

---

## Mission

Execute the **T14 CL acquisition band — SMOKE segment** per
`docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md` (read it FIRST, in full; it is the
protocol of record). This is the first time a taught novel rule travels through the real
extraction pipeline end-to-end: two CL-bench contexts (`flywheel-assembly-agent`,
`aether-language`) go through OFF pre-gate → teach Session A → real pipeline → fidelity gate →
paired Session B (ON/OFF/PLACEBO). The smoke **gates the full 8-context band**; it is
pipeline-validation, **explicitly NOT efficacy data** — no PASS/FAIL/UNDERPOWERED verdict may be
claimed from it.

Ticket: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md`
(status `in_progress`; read the **Pre-Registration** block + the **2026-06-12 amendment** + the
**CL-bench policy** bullet — they are binding law for this session).

## Why this exists (one paragraph of history you must not re-derive)

The 2026-06-12 T14 build+smoke (`docs/assessments/2026-06-12-t14-efficacy-harness-smoke.md`)
validated the 3-arm harness end-to-end (9/9 solves) but found (P1) the self-authored invented-rule
battery does NOT discriminate — OFF won every task because the rules were within Sonnet's default
competence — and (P1) the production `compile_context` path no_matches verbose prompts (qwen3 floor
+ length dilution; only focused queries retrieve). The owner's response: import discrimination from
CL-bench (`tencent/CL-bench`, arXiv:2602.03587 — 1,899 expert tasks; frontier models average 17.2%
WITH the context in-window; published past every model's cutoff) via the **teach-session protocol**
— the knowledge is never planted, it travels: context document → genuine claude-code session →
real extraction → `.pending` → human gate → corpus. 10 contexts were selected on 2026-06-12 (2
smoke + 8 full + 3 alternates), sentinels mechanically verified, dataset pinned. The smoke pair
deliberately brackets the extraction-size envelope: flywheel = 4.2k chars / knowledge in the
SYSTEM prompt / 12 flat siblings; aether = 33.5k chars / knowledge in the USER document / nested
multi-turn siblings (depths 2,4,6; the depth-2 task is `teach_only` — its question is fused into
the context).

## Artifacts of record (read before any execution)

- `docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md` — THE protocol (§4 per-task lifecycle,
  §5 smoke definition, §6 pre-registration deltas, §9 risks).
- `tests/e2e/efficacy/clband/manifest.json` — the band: pinned sha
  `b28a5832a09b0d96c0cf4c22e90d7c60ede25b80`, verified sentinels per context, eval scores, roles.
- `scripts/fetch_clband_contexts.py` — materializes contexts to
  `tests/e2e/efficacy/clband/contexts/<name>/{system.md,context.md,tasks.json}` (gitignored;
  fetch-on-demand; fails loud on drift/missing-sentinels). Already proven 13/13 green.
- `tests/e2e/efficacy/CONTRACT.md` + `scripts/efficacy_ab.py` — the validated 3-arm harness
  (materialize → live HTTP injection → claude-code solve → deterministic verifier → attribution →
  gate; reuses `scripts/retrieval_metrics.py` sign_test). 51 unit tests, `--self-test`, `--dry-run`.
- `tests/e2e/reports/efficacy/smoke-sensitivity-141049/` — the prior smoke's report format.

## Work units (in order; singleton heavy actions — see standing rules)

**Unit 0 — Preflight + pre-registration deltas (NO runs before this lands).**
1. Fold the plan's §6 pre-registration deltas into the T14 ticket verbatim-by-reference (band
   roster fixed; alternate substitution only via OFF pre-gate; verifiers/rewrites/judge prompts
   committed before each measured run; INSTRUMENT-FAILURE taxonomy extraction-vs-injection/obedience;
   solver checkpoint + dataset sha recorded; injection mode labeled per run). Commit this BEFORE
   any measured run so the lock is in git history first.
2. `uv run --with pyarrow python3 scripts/fetch_clband_contexts.py --only 7833ca0b bc874bce`
   (or all 13). Verify sentinel-OK output.
3. Bring up the live stack; corpus gotchas from T11 (memory `v17-t11-hybrid-verdict-dense-views-win`):
   PG is EPHEMERAL in `docker-compose.test.yml` — the 262 corpus survives only in
   `tests/e2e/reports/replica-run/skills/`; re-seed volumes from there; pin
   `OLLAMA_EMBED_MODEL=qwen3-embedding:4b`; gate every measurement window on **/health 200**
   (T17 honesty). Restart mcp-server after any corpus change.
4. Verify the scope mechanism before designing Session A: the plan requires **one skill-layer
   project scope per context** (`clband-flywheel-assembly-agent`, `clband-aether-language`),
   isolated from the 262 dogfood corpus. Project scoping exists (container project-scope #154,
   DS-009 multi-repo isolation) — confirm how a fresh workspace dir maps to a scope and that
   retrieval can be scoped to it; record the mechanics in the report. If scope isolation cannot be
   achieved with existing code, STOP and surface to the owner — do not hack scope semantics inside
   this batch.

**Unit 1 — Verifier + rewrite authoring (committed BEFORE any run that uses them).**
For each smoke context, from `contexts/<name>/tasks.json`: identify the teach task (aether: the
`teach_only` depth-2 task; flywheel: sibling #1 of 12) and select 2 candidate measured siblings
each. For each candidate sibling author: (a) the **de-referenced question** (replace "the attached
document/above" with the named system — "the Aether language", "the Flywheel Manufacturing
Multi-Agent System rules"; NEVER add content); (b) the **deterministic verifier core** — ≥5 exact
string/number/ordering/structure checks compiled from the CL-bench rubrics (drop persona-only
rubrics from the core; knowledge rubrics dominate); (c) the committed claude-CLI **judge prompt**
with the verbatim rubrics (secondary score only). Aether's nested siblings: prior reference answers
LEAK rules — apply the plan's §4 rule (measure without `prior_turns` if the de-referenced question
is self-contained; else include them identically in ALL arms). Unit-test the verifiers offline
(good/bad fixture pairs, like the existing 10). Commit.

**Unit 2 — OFF pre-gate (Step 0).** Bare agent (no layer, no context), each candidate sibling,
against its verifier. Any sibling OFF passes → REJECTED as non-discriminating (record it; pick the
next sibling — flywheel has 11 spares, aether has 2 total so a double-rejection there is itself a
P1 finding about the band). This is also the empirical not-in-pretraining check. Expected: OFF
craters on invented specifics; if OFF somehow passes everything, STOP — that is a selection-failure
finding, report it, do not proceed to teach sessions.

**Unit 3 — Session A teach sessions (2).** Fresh workspace per context containing the knowledge
document (merge per `knowledge_home`: flywheel = system.md is the document; aether = context.md).
Run a GENUINE claude-code working session whose prompt is the teach task's question (aether: the
fused depth-2 turn verbatim) — the agent must actually work the task with the document; the
transcript must show the rules being used. Hooks capture under the context's scope. Not a
paste-and-quit; if the session answers without engaging the document, redo with a prompt that
requires engagement (and record the redo).

**Unit 4 — Pipeline + fidelity gate (Steps 2–3).** Real extraction on the captured sessions with
`EXTRACT_SESSION_PROVIDER=claude-code` → `.pending` drafts → **STOP: surface the drafts to the
owner for the human gate** (never auto-approve — lifecycle governance is untouchable) → corpus +
rebuild for each clband scope → **fidelity gate**: every manifest sentinel for the context present
across the scope's accepted skill texts (committed grep-level script, fail loud). Fidelity failure
= **INSTRUMENT-FAILURE(extraction)**, a P0 extraction finding with the context size attached —
report it prominently; that context produces no Session B data point and no "layer doesn't help"
reading. A pass at 4.2k + fail at 33.5k is a publishable size-threshold finding, not a failure of
the session. These drafts also count toward T14's ≥10-real-drafts AC — record them.

**Unit 5 — Session B paired runs (Step 4).** For each surviving sibling (1–2 per context):
ON / OFF / PLACEBO, serialized. ON = real mcp-server injection from the context's own scope,
**focused inject-query mode** (`--inject-query summary` class — Finding 2 workaround), injection
mode LABELED in the report. PLACEBO = injection from the OTHER smoke context's scope at matched
token mass (flywheel skills for an aether question), explicitly labeled control. Capture per-pull
attribution (skill ids, scores, token mass) + the pre-registered secondaries (paired turns,
tokens). ON failing a sibling whose skills attribution shows were injected =
**INSTRUMENT-FAILURE(injection/obedience)** — distinct from Unit 4's class; classify precisely.

**Unit 6 — Report + closeout.** `docs/assessments/2026-06-12-t14-clband-smoke.md` in the house
style (every number with its persisted raw artifact under `tests/e2e/reports/efficacy/clband-smoke-*/`;
verbatim pre-registration citations; honest negative findings first-class). Must state explicitly:
smoke ≠ efficacy data; what the full band is now gated on; the GO/NO-GO recommendation for the
8-context full run with reasons. Update: T14 ticket (smoke status note), batch index if scope
state changed, memory. Then cleanup per standing rule (build artifacts + STALE scratch ONLY —
NEVER this session's transcripts/drafts/reports/corpus; "also in PG/Qdrant" ≠ disposable).

## Hard fences (violations void the session)

- **NO efficacy verdict** from this smoke (pre-registration forbids it; the ≥7/10 criterion is
  untouched and unscored).
- **NO pre-registration changes** after any measured run exists in this session; Unit 0.1 lands first.
- **NO planting**: skills enter the corpus only via session-capture → extraction → human gate. If
  extraction fails fidelity, the answer is report-it, never hand-edit a skill into shape.
- **NO auto-approval** of `.pending` drafts — owner gate, full stop.
- **NO fakes/stubs** anywhere (machine-wide rule): verifiers real and unit-tested; placebo is the
  explicitly-labeled real-stack control; missing wiring fails loud.
- **Scope isolation**: clband skills must NOT leak into the 262 dogfood scope or vice versa
  (the 262 corpus is the organic band's substrate and T18's instrument — contaminating it is a
  cross-ticket incident). Verify isolation with a probe query before Session B.
- **NO changes to crates/retrieval ranking/floor behavior** in this batch — the verbose-prompt fix
  belongs to T18/T12; this session uses the labeled focused-query workaround.
- Standing rules: measurement drives the REAL mcp-server over HTTP (no in-process reconstruction);
  all heavy actions (builds, solves, extraction) SERIALIZED by the orchestrator — subagents are
  explicitly forbidden from cargo build/clippy/test or model-call storms; execution agents run on
  sonnet; never truncate graph_state; no arbitrary time/token caps on churners — drain until done.

## Owner decision points (STOP and ask)

1. Human gate on `.pending` drafts (Unit 4) — present them with the sentinel-coverage summary.
2. If scope isolation needs new mechanism (Unit 0.4).
3. If both aether siblings fail the OFF pre-gate or fidelity gate — the smoke's large-context half
   is then unprovable as designed; options (swap an alternate large context vs. report-and-stop)
   are the owner's call.
4. GO/NO-GO for the full 8-context band rides on this report — recommend, don't decide.

## Done means

- [ ] Pre-registration deltas committed BEFORE first measured run (git history proves order).
- [ ] Contexts fetched, sentinels verified (script output in report).
- [ ] Verifiers + rewrites + judge prompts committed before their runs; offline-tested.
- [ ] OFF pre-gate run + per-sibling accept/reject recorded with raw outputs.
- [ ] 2 genuine teach sessions captured under isolated clband scopes.
- [ ] Real extraction + owner-gated acceptance + fidelity gate verdicts (both sizes) with raw data.
- [ ] Session B paired ON/OFF/PLACEBO on surviving siblings, attribution + secondaries persisted.
- [ ] Report with GO/NO-GO recommendation; ticket/index/memory updated; surgical cleanup done;
      everything committed (conventional commits, batch-scoped).
