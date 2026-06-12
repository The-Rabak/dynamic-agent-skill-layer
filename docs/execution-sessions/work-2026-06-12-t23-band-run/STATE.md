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
status: in_progress
execution_shape: infra-track
current_unit: A
total_units: 5
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
| A | Instruments at scale (8 contexts: verifiers ≥5 checks + fixtures + de-ref rewrites + operative sentinels) | infra-packet | Per-context measurement eligibility; sentinel verification gate | pending | -- | unit-A-instruments.md |
| B | Band orchestrator + auto-gate + scope-guard tests (run_band.py, Steps 0–5, resumable) | infra-packet | The driver; the safety boundary | pending | -- | unit-B-orchestrator.md |
| C | The overnight run (context #1 canary → 2–8; dogfood re-probe) | infra-packet | The paired efficacy data | pending | -- | unit-C-overnight-run.md |
| D | Morning report (verdict vs LOCKED pre-reg + secondaries + attribution + closeout) | infra-packet | The verdict; assessment + closeout | pending | -- | unit-D-morning-report.md |

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
_No learnings yet (Unit 0 in progress)._
