# Session handoff — reacquaint prompt (written 2026-06-12, after the T22 filing)

Paste everything below the line into the fresh session. Purpose: reacquaint the next iteration with
the project, scope, plan, pitfalls, and the exact stopping point — BEFORE the owner reassesses the
next work run. Do not start executing work from this prompt; orient, verify, then discuss.

---

## What this project is

**Dynamic Agent Skill Layer** (`/home/rabak/projects/dynamic-agent-skill-layer`, Rust workspace,
branch `feat/v-1-7`). A persistent, self-growing, human-gated skill memory for coding agents: real
claude-code sessions are captured → an LLM extraction pipeline distills durable skills (multi-view
fields, typed graph edges) → `.pending` drafts pass a HUMAN gate → corpus (Postgres + Qdrant,
qwen3-embedding:4b, model-keyed collections) → retrieval over MCP (`find_skill`,
`compile_context` SessionStart priming, `search_skill_graph`) injects them into future sessions.
The culture is measurement-first: every claim drives the REAL running mcp-server over HTTP; negative
results are recorded and acted on; pre-registration before data; no number without its raw artifact.

Your persistent memory directory auto-loads `MEMORY.md` — the entries prefixed `v17-*` are the
project spine. Trust repo files over memory where they disagree; verify before citing.

## Read in this order (then verify git state)

1. `git log --oneline -15` and `git status` — confirm HEAD ≈ `0d046e2` (T22 filing) and a clean tree.
2. `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md` — the batch ledger
   (frontmatter notes: `restructure_note`, `t22_note`; batches 1–21).
3. `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/22-teach-path-extraction.md` —
   the NEXT batch (17), with its four units A–D.
4. `docs/plans/2026-06-12-t22-teach-path-extraction-work-prompt.md` — the ready-to-run work prompt
   for Batch 17 (do NOT launch it until the owner says go).
5. `docs/assessments/2026-06-12-t14-clband-smoke.md` INCLUDING the addendum — the evidence that
   produced T22.
6. `docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md` — the CL acquisition band protocol
   (selection method, per-task lifecycle, full-benchmark scaling §8).
7. Skim: `docs/assessments/2026-06-11-v1-7-midpoint-deep-grok-assessment.md` (the governing
   assessment; measurement-integrity was the binding constraint, since lifted to ~8.5 by T11) and
   `docs/reference/retrieval-contract.md` §0 (how retrieval works today).

## State of the board (V1.7, Phase B)

**Done:** T01–T09 (Phase A; T07 skipped), T10 (262-skill dogfood corpus from 24 genuine sessions),
T11 (instrument-first re-sweep: α=0 gate cratered 100%; sparse/BM25 hybrid FALSIFIED net-negative;
qdrant_hybrid exact tie → not promoted; dense multi-view VALIDATED → `RETRIEVAL_DENSE_VIEWS`
default-ON since 7fe8912; frozen 0.80/0.80/0.90 target MET held-out 0.884–0.912; candidate-recall,
not ranking, is the lever — MRR@3==MRR@10 everywhere), T13 (no-fakes drain), T17 (boot readiness
honesty + embedding cache, 32× warm boot), T21 (workspace gates green, all three forms), T20 (the
T11 instrument IS the e2e gate now: `scripts/retrieval_sweep.py --gate`, α=0 canary, stale fixtures
retired, T11 erratum + real latency artifact p95 375ms).

**T14 (efficacy, in_progress) — the critical path.** Three sub-chapters so far:
1. **Harness smoke** (`docs/assessments/2026-06-12-t14-efficacy-harness-smoke.md`): 3-arm harness
   (ON/OFF/PLACEBO, deterministic verifiers, attribution, PASS/FAIL/UNDERPOWERED, pre-registration
   LOCKED: "ON wins ≥7/10 by sign test, no catastrophic regression") VALIDATED end-to-end — but the
   self-authored invented-rule battery does NOT discriminate (OFF wins; rules within Sonnet's
   default competence) and the production `compile_context` path no_matches verbose prompts
   (qwen3 floor 0.48 + length dilution; T18/T12 own that fix; harness uses labeled focused-query
   injection meanwhile).
2. **CL acquisition band selected**: 10 contexts (2 smoke + 8 full) + 3 alternates from
   `tencent/CL-bench` (arXiv:2602.03587; frontier avg 17.2% WITH context; pinned sha `b28a5832…`),
   chosen via 6-axis parallel evaluation for invented-ness + ON-winnability (our OFF arm has NO
   context, so OFF-fail is near-free; the scarce property is ON-winnable). Manifest:
   `tests/e2e/efficacy/clband/manifest.json`; reproducible fetch:
   `scripts/fetch_clband_contexts.py` (contexts/teach dirs are gitignored — licensed dataset
   content never enters git). Teach-session protocol: knowledge travels context-doc → genuine
   session → real extraction → human gate → corpus; NEVER hand-planted.
3. **clband smoke = NO-GO with a precise diagnosis** (session `work-2026-06-12-clband-smoke`):
   OFF pre-gate PASSED (4/4 discriminate), 2 genuine teach sessions, scope isolation proven
   dogfood-safe, 11 real drafts (≥10-drafts AC evidence) — but **fidelity gate FAILED at both
   sizes → INSTRUMENT-FAILURE(extraction)**. The post-session forensic re-read (assessment
   addendum) split this into THREE components: (a) **sanitizer drops** — the suspicious-speaker
   filter dropped system-speaker entries on every window; flywheel's knowledge doc lives in the
   system prompt, so the prose extractor plausibly never saw it (the smoke report MISSED this);
   (b) **extractor worldview** — verified verbatim, survives visibility (aether's spec was a file):
   the lesson extractor demands recurrence + discovery-through-failure and refuses taught
   knowledge — a PRODUCTION gap, not a benchmark quirk; (c) **gate mis-leveling** — the 11 drafts
   came via the preference/convention detector and preserve invented operative specifics VERBATIM
   (proof verbatim capture already exists); the gate's sentinels were document-level names that
   channel never emits.

**NEXT = T22 `teach-path-extraction` (Batch 17, ready).** Units: A forensics/visibility map →
B harness-side document delivery (never weaken the injection-defense filter) → C taught-knowledge
candidate class in the real extraction prompt (verbatim operative specifics; recurrence not required
for taught material; HARD dogfood-regression gate — re-extract 2–3 organic sessions, no degradation;
refusal≠malformed retry fix) → D two-tier sentinels (operative tier gates) + **smoke re-run = the
GO gate** for the 8-context band. Owner decision points are listed in the work prompt (biggest:
whether `EXTRACT_TEACH_CAPTURE` ships default-ON after the regression diff).

**After T22:** smoke re-run → full 8-context band (T14 verdict vs the locked pre-registration) →
T18 (Batch 18, priming instrument: session-start stratum incl. VERBOSE-opening substratum, priming
metrics — never MRR, negative control, baseline through `compile_context`) → T12 (Batch 19,
mechanism: typed `RetrievalIntent` seam; FIRST scope item = fix verbose-prompt priming —
intent-conditional floor / query-side multi-view / distillation, decided on T18's instrument; do
NOT naively lower the global 0.48 floor) → T15 (Batch 20, SWE-bench compounding) → T16 (Batch 21,
anytime). T19 (cross-project recurrence) is deferred — unmeasurable on a single-project corpus.

## Pitfalls and standing law (violations have burned us; all are in CLAUDE.md/memory — obey)

- **No stubs/fakes/placeholders in production paths or non-unit tests — fail loud.** Placebo arm is
  the one explicitly-labeled measurement control. Surface violations as todos, never paper over.
- **Measurement drives the REAL mcp-server over HTTP.** No in-process reconstruction, ever
  (a hand-rolled rig once lied 0.017 vs real 0.233).
- **Serialize ALL heavy actions** (cargo build/clippy/test, model-call storms) at the orchestrator;
  subagents explicitly forbidden — a parallel-build run crashed WSL2 and zeroed dirty files.
  Execution agents run on sonnet. Commit early — unflushed WSL2 writes are unrecoverable.
- **Never delete this session's generated outputs** (corpus/drafts/logs/reports — "also in
  PG/Qdrant" ≠ disposable). Cleanup = build artifacts + STALE scratch only; `target/` once hit 137GB.
- **Pre-registration discipline:** criteria locked before data; changes after data VOID the run;
  three outcomes PASS/FAIL/UNDERPOWERED (+ INSTRUMENT-FAILURE, split extraction vs
  injection/obedience); smoke runs are NEVER efficacy data.
- **Human gate is untouchable** — no auto-approval of `.pending` drafts, ever.
- **Repo quirks:** `docs/plans/` is GITIGNORED — `git add -f` for plan docs (established pattern).
  PG is EPHEMERAL in `docker-compose.test.yml` — the 262 corpus survives only in
  `tests/e2e/reports/replica-run/skills/`; re-seed volumes, restart mcp-server after corpus
  changes, gate every measurement window on `/health` 200 (T17 honesty), pin
  `OLLAMA_EMBED_MODEL=qwen3-embedding:4b`. Migrations are a compile-time array in `postgres.rs`,
  not dir-scanned. Never truncate `graph_state`. Bare `claude --dangerously-skip-permissions` in
  Bash is blocked by the auto-mode classifier — drive solves via `efficacy_ab.run_claude_solve`.
  clband extraction driver: `clband_extract.py` (needs `GRAPH_BUILDER_GLOBAL_ROOT`,
  `EXTRACT_SESSION_PROVIDER=claude-code`).
- **CL-band specifics:** dataset content (contexts/, teach/, .cache/) never enters git (license);
  the fetch script fails loud on dataset drift and missing sentinels; sentinels must be verified
  against the COMMON context text (two evaluator sentinels were hallucinated from rubrics before);
  alternates substitute only via the OFF pre-gate; pin solver checkpoint + bench sha per run —
  non-pretraining EXPIRES with future checkpoints.
- **Score compression lore:** the "qwen3 scores compressed ~0.016" alarm was the RRF
  `fusion_rank_score` artifact — real eq.3 scores are 0.58–0.93 (#260). Don't recalibrate the floor
  off the wrong number.
- When deploying subagents, append the project CLAUDE.md "Code Search" (semble) section to their
  prompts (repo convention).

## Where we stopped, exactly

Last commits: `0d046e2` (T22 filed: ticket + assessment addendum + index renumber + work prompt +
memory) ← `920e6f8` (TASM positioning note folded into the deepened V2 plan — arXiv:2606.11853 is
the within-context KV-compression sibling; fork framing: "compress-in-window" vs our
"distill-to-skills", adjudicated by the CL band) ← `b094ad5` (CL band selected + protocol).
Working tree clean. Nothing in flight.

**The owner's stated next step: reassess the next work run together BEFORE launching anything.**
So: orient via the reading list, verify git state matches this handoff, surface anything that looks
drifted, then present the owner with the launch decision for Batch 17 (T22) — including the open
question worth raising at reassessment: Unit A may show the flywheel document never reached a
window, in which case Units B+D alone might green flywheel and the deep worldview fix (Unit C)
carries only aether — i.e., the "deep" fix may be smaller than the NO-GO implied. Do not launch
`/workflows:work` until the owner says go.
