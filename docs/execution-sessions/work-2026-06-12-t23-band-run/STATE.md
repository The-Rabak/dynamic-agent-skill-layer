---
source_type: ticket
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/23-automated-clband-run.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "T23 (NEW 2026-06-12) — owner GO on the band (post-T22) + owner directive: fully automated unattended overnight run, no manual gating of ~150 benchmark drafts"
brainstorm_ref: ""
band_protocol_ref: docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md
go_evidence_ref: docs/assessments/2026-06-12-t14-clband-smoke.md (T22 RESOLUTION)
started: 2026-06-12
status: completed
execution_shape: infra-track
current_unit: 5
total_units: 5
outcome: "Band ran end-to-end unattended. VERDICT (T14's, vs LOCKED >=7/10): INSTRUMENT-FAILURE — 0 clean efficacy points; efficacy UNANSWERED. Harness PROVEN (auto-gate scope-guarded, scope isolation, retrieval, canary + dartman built+retrieved, dogfood re-probe = 262 pristine). Binding constraint = EXTRACTION FIDELITY (refines T22): 5/8 genuine extraction gaps (value-precise tokens dropped), 1/8 fidelity-gate false-negative (quartermaster recoverable), 1/8 non-discriminating (ezlang), 1/8 timeout-confounded (dartman). Report docs/assessments/2026-06-13-t14-clband-band.md. NO mid-run protocol change; pre-reg intact. Recommend extraction value-preservation + verifier-based fidelity gate + task-design fixes + re-run."
session_id: work-2026-06-12-t23-band-run
solver_checkpoint: "claude-code 2.1.175, --model sonnet (smoke was 2.1.173 — solver bump; OFF pre-gate re-runs per context per plan §1 expiry rule, so auto-handled)"
dataset_sha: b28a5832a09b0d96c0cf4c22e90d7c60ede25b80
---

## WHY Linkage
- Canonical WHY source: docs/assessments/2026-06-12-t14-clband-smoke.md (T22 RESOLUTION = GO gate) + T23 ticket
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- Band protocol: docs/plans/2026-06-12-t14-cl-acquisition-band-plan.md (§4 lifecycle Steps 0–5)
- This execution serves: Execute T14's 8-context CL acquisition band end-to-end, fully automated and
  unattended overnight, under a pre-registered auto-gate amendment (Unit 0). T14 owns the
  pre-registration and the verdict; T23 owns instruments-at-scale + orchestrator + the run + the
  morning report. This is the first paired efficacy data of the project — the verdict the whole V1.7
  efficacy chapter exists to produce.
- Success-criteria focus: T23 acceptance criteria (8 ACs in the ticket).

### TDD Contract
- Effective mode: Ralph-driven (ticket tdd_mode: ralph) for the scope-guard auto-gate code (Unit B:
  scope-assertion unit tests RED→GREEN first) and verifier fixtures (Unit A: good/bad fixture pair
  green before each context runs).
- Required evidence: unit tests (pytest scope-guard + verifier fixture self-tests) + live e2e (the
  band run itself drives the REAL mcp-server over HTTP; per-context fidelity gate + OFF pre-gate +
  paired Session B). Measurement drives the REAL stack.
- Exceptions: Unit 0 is a pre-registration amendment (docs only); Unit C is the live run (e2e
  evidence = the artifact-backed report); Unit D is synthesis.

### Constitution Context
- No docs/constitution.md in repo; governing rules = machine-wide ~/.claude/CLAUDE.md (no
  stubs/fakes — fail loud) + project standing rules: measurement drives the REAL mcp-server over
  HTTP; heavy actions serialized by the orchestrator (subagents forbidden from cargo build/clippy/
  test + model-call storms); execution agents on sonnet; never delete this run's outputs; never
  truncate graph_state; workspace gates green.

### Architecture Handoff (explicit, from T14 plan §4 + T22 RESOLUTION)
- Feature homes: tests/e2e/efficacy/clband (band orchestrator + auto-gate + instruments) + scripts/
  (efficacy_ab.py harness primitives) — NO production crates touched.
- Untouchable boundary: the production human gate + the 262 dogfood corpus. Auto-accept ONLY under
  clband-* scopes; hard scope-guard assertion before every rename; fail loud otherwise.
- Mechanism truth (no fakes): acceptance = real rename SKILL.md.pending → SKILL.md
  (scripts/efficacy_draft_acceptance.py structural def) + real scope rebuild; measurement drives the
  REAL mcp-server over HTTP; OFF pre-gate via efficacy_ab.run_claude_solve; extraction via
  clband_extract.py (EXTRACT_SESSION_PROVIDER=claude-code, GRAPH_BUILDER_GLOBAL_ROOT).
- Seam under investigation (bg research agent ad7f75e0): how an accepted clband skill becomes
  RETRIEVABLE by the running mcp-server under its clband-<name> scope (snapshot reload? re-ingest?
  restart?). Session B was cancelled in the smoke, so this path is unexercised — context #1 is the
  live canary.

## Unattended policy (pre-committed in Unit 0; no STOP-and-ask overnight)
- HARNESS-LEVEL breakage (crash, scope leak, /health failure, dataset drift, auto-gate guard trip)
  ⇒ STOP, preserve state + checkpoint, write a morning stop report.
- Per-context INSTRUMENT-FAILURE (fidelity RED, or ON-failing-with-verified-injection) ⇒ record with
  taxonomy (extraction vs injection/obedience), no efficacy point, CONTINUE the band.
- OFF pre-gate passes (non-discriminating sibling) ⇒ drop sibling; context losing ALL siblings ⇒
  substitute next alternate (the only substitution path).
- NEVER delete run outputs; drain-until-done; no arbitrary time/token caps.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 0 | Pre-registration amendment (auto-gate, roster lock, unattended policy) | infra-packet | Legal foundation — amendment lands BEFORE first band datum | completed | 1 | unit-0-preregistration.md |
| A | Instruments at scale (8 contexts: verifiers ≥5 checks + fixtures + de-ref rewrites + operative sentinels) | infra-packet | Per-context measurement eligibility; sentinel verification gate | completed (dddeb82) | 1 | unit-A-instruments.md |
| B | Band orchestrator + auto-gate + scope-guard tests (run_band.py, Steps 0–5, resumable) | infra-packet | The driver; the safety boundary | completed (bb14c03,032f040) | 1 | unit-B-orchestrator.md |
| C | The overnight run (context #1 canary → 2–8; dogfood re-probe) | infra-packet | The paired efficacy data | completed — INSTRUMENT-FAILURE | 1 | (run.log + band_results.json + per-context artifacts) |
| D | Morning report (verdict vs LOCKED pre-reg + secondaries + attribution + closeout) | infra-packet | The verdict; assessment + closeout | completed | 1 | docs/assessments/2026-06-13-t14-clband-band.md |

## Owner decisions ALREADY MADE (do not re-ask)
1. GO for the band (2026-06-12).
2. Fully automated, unattended overnight; NO human gating of band drafts.
3. Auto-accept-all in clband-* scopes ONLY, via the real rename path + real scope rebuild,
   pre-registered as the Unit 0 amendment BEFORE any band data. Production/dogfood gate unchanged.
4. EXTRACT_TEACH_CAPTURE stays default-ON (T22 DP-1).
5. Smoke contexts are not verdict data; no fresh smoke re-capture; context #1 of the band is the
   live canary.

## Readiness (verified 2026-06-12 ~20:37Z)
- docker: mcp-server, graph-builder, redis, ollama, postgres, qdrant ALL up + healthy.
- /health: healthy=true; embedding_arm=qwen3-embedding:4b dim=2560 collection=skills__qwen3-embedding-4b;
  retrieval_backend=snapshot_dense; extraction_provider=ollama; pg/redis/ollama reachable.
- target/debug/maintenance-worker present (built Jun 12 22:31, fresh). mcp-server binary present.
- claude CLI 2.1.175. ollama has qwen3-embedding:4b + gemma4:12b.
- All 10 contexts materialized under tests/e2e/efficacy/clband/contexts/ (system.md/context.md/tasks.json).
- T23 registered as Batch 18, status ready, depends_on T22 ✅.

## Learnings Brief
- [scope-mechanism] **The ON-arm retrieval path is LIVE-PROVEN (canary PASS).** The proven recipe
  (smoke DP-2 Option A): clband skills live in named volume `dynamic-agent-skill-layer_test-project-skills`
  at `/skills/project/clband-<name>/{.git,.skills/...}`; volume writes go via one-off
  `docker run --rm -v <vol>:/skills/project alpine` (service mounts are `:ro`). The `.git` marker makes
  `compile_context repo_path=/skills/project/clband-<name>` resolve to that subdir; retrieval filters by
  `source_path.starts_with(scope_path)` → returns ONLY that scope's skills (dogfood + other clband
  scopes excluded). graph-builder POLLS the volume (~15s) → full rebuild → PG/Qdrant → Redis
  `graph.rebuilt` → mcp-server `reload_and_swap` (ArcSwap, NO restart). Validated end-to-end with a
  throwaway canary (write→accept→retrieve isolated→remove→restore 262). `scope_rebuild.py --canary`.
- [gotcha] compile_context returns `status=duplicate_suppressed` for a repeated (session_id, prompt)
  pair → readiness polls MUST vary session per attempt (fixed in `wait_retrievable`). Real Session B
  uses unique per-task session ids, so measurement is unaffected.
- [retrieval] `find_skill` is UNSCOPED (hardcoded `retrieve(prompt, None)`) → ON/PLACEBO MUST inject via
  `compile_context` with a clband `repo_path`. PLACEBO = a DIFFERENT context's clband scope (matched
  mass), trivially available because Pass 1 builds all scopes before Pass 2 measures.
- [design] run_band.py is TWO-PASS: Pass 1 builds each context's scope (Step 0 OFF pre-gate → teach →
  extract → fidelity gate → auto-gate accept → rebuild → wait_retrievable); Pass 2 measures surviving
  siblings (ON own-scope + PLACEBO donor-scope; OFF REUSED from the Step-0 pre-gate = identical bare
  solve). Context #1 = the live canary for the full path. Checkpointed/resumable per (context, step).
- [safety] The AUTO-GATE scope guard (`assert_clband_path`) rejects any path not under
  `/skills/project/clband-<...>/` (dogfood/global/production) → fail loud. 24 unit tests green.
- [build] Unit A instruments are 8 PARALLEL sonnet execution-agents (file-disjoint per context),
  each self-testing its verifier (good=0/bad=1) + grep-verifying operative sentinels verbatim. The
  orchestrator MERGES `instruments/<name>.json` sentinels into manifest.json + re-verifies (the gate).
  teach_delivery.py generalized to be data-driven from instruments/<name>.json `doc_file`.

## Live-run findings (Unit C, 2026-06-13)
- **Canary (context #1 material-handler-sops) — full path runs; fidelity RED = GENUINE partial-extraction
  failure (correct exclusion).** OFF pre-gate: both siblings OFF=loss (real discrimination). Teach rc=0
  (252 KB transcript). Extract: 25 drafts. Fidelity gate: 3/7 operative sentinels "missing" → RED →
  INSTRUMENT-FAILURE(extraction), band CONTINUED (corpus stayed 262, no scope created — auto-gate never
  fired). **Diagnosis (grep + the authoritative verifier run against the concatenated drafts):** 6/7
  rules SURVIVED — 4 verbatim + 2 REWORDED (`50 pounds`→`50 lb`; `10-minute UV sanitization cycle`→
  `UV transfer chamber 10-min cycle`) that the EXACT-substring sentinel gate false-negatived — but
  `<1 megaohm` (wrist-strap resistance) was **GENUINELY DROPPED**. The real verifier ALSO fails on the
  drafts (on the megaohm check), so ON would get the same value-less drafts and fail in Session B →
  **excluding material-handler is CORRECT.** This is real extraction fidelity loss of a hyper-specific
  numeric value (a refinement of T22: taught-capture preserves rules/procedures but can still drop the
  most-specific constants the deterministic verifier requires). NO mid-run protocol change; pre-reg intact.
- **Unit D classification method (per fidelity-RED context):** run `verifiers/<name>.sh` against the
  concatenated accepted drafts (`scope/.skills/**/*.pending`, persisted, never deleted). Verifier FAILS
  too → genuine extraction gap (correct exclusion). Verifier PASSES → strict-sentinel FALSE-NEGATIVE
  (context was measurable; recoverable via a verifier-based fidelity gate re-run). Report this per
  excluded context; recommend a verifier-based fidelity gate for any re-run.
- **Watch-item:** if MOST contexts fidelity-fail on genuine value-drops, the band yields little/no
  efficacy data → the honest verdict is INSTRUMENT-FAILURE(extraction)-dominated + the per-context
  genuine-gap-vs-false-negative split (NOT a gate bug to silently fix). The owner decides any re-run.

## Build status (Units A + B, 2026-06-13)
- Unit 0: COMMITTED (9be303b).
- Unit B core BUILT + validated: `scope_rebuild.py` (canary PASS live), `test_scope_rebuild.py`
  (24 green), `run_band.py` (compiles/imports; --plan pending all instruments), teach_delivery
  generalized. Pending: merge Unit-A sentinels → manifest.json + re-verify + `--plan` + commit.
- Unit A: 5/8 instrument agents done (material-handler-sops, ezlang-language, 123corp-hr-policy,
  drywave-3000-manual, quartermaster-hold-inventory) — all self-tested good=0/bad=1, sentinels
  grep-verified. 3 running (source-integrity-agent, dpms-agent-m, dartman-game).
