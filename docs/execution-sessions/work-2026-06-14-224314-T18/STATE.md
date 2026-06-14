---
source_type: ticket
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/18-priming-instrument-session-start-stratum.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "T18 (Batch 19) — split from T12 (restructure 2026-06-12); lead of the elevated real-usage spine T18→T12→T15 (reprioritization 2026-06-13)"
brainstorm_ref: ""
started: 2026-06-14
status: paused
pause_reason: "OWNER HOLD — drive to the pre-registration gate, then stop before stratum authoring / live measurement / heavy agents. Awaiting owner go/no-go on the staged pre-registration."
execution_shape: infra-track
current_unit: 1
total_units: 4
session_id: work-2026-06-14-224314-T18
---

## WHY Linkage
- Canonical WHY source: parent plan `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
  (`## Agent usefulness targets`; `## Retrieval Flow` — `compile_context` is the zero-touch SessionStart
  surface, must stay <500ms and conservative). No brainstorm/architecture artifact (both refs null).
- This execution serves: an honest, priming-appropriate MEASUREMENT INSTRUMENT for the SessionStart
  prime, so T12's priming signals earn or lose their place on evidence — and so the production
  `compile_context` verbose-prompt `no_match` (T14 smoke Finding 2) is quantified, not worked around.
  Lead of the real-usage spine: T18 (instrument) → T12 (fix) → T15 (measure compounding through it).
- Success-criteria focus: T18 AC — session-start stratum (incl. verbose substratum) authored;
  metrics + per-signal ROI thresholds + judge rubric pre-registered BEFORE authoring/data; negative
  control runs FIRST and craters; baseline prime measured through `compile_context` on the real server.

### TDD Contract
- Effective mode: Ralph-driven (ticket tdd_mode: ralph) — but this is a MEASUREMENT ticket: the
  "tests" are the negative-control gate (must crater BEFORE any baseline number) + anti-circularity
  token-overlap probe + the pre-registered decision rules. Required evidence: persisted raw per-query
  artifacts under tests/e2e/reports/ (T11 format); measurement drives the REAL mcp-server over HTTP.
- Exceptions: no production crate changes (scripts/fixtures/reports only); the mechanism is T12.

### Constitution Context
- No docs/constitution.md in repo; plan records constitution_version 2.1.0, no waivers. Governing rules
  = machine-wide CLAUDE.md (no stubs/fakes — fail loud) + project standing rules (measurement drives
  the real app over HTTP; heavy actions serialized by the orchestrator; subagents on sonnet, forbidden
  from cargo build/clippy/test + model-call storms; never delete this session's outputs; pin
  OLLAMA_EMBED_MODEL=qwen3-embedding:4b; gate windows on /health 200; SessionStart p95 <500ms).

### Architecture Handoff (plan-derived; no separate artifact)
- Feature home: `scripts/` (the T20 shared measurement lib — `retrieval_metrics.py`,
  `retrieval_sweep.py`) + `tests/fixtures/` + `tests/e2e/reports/`. NO crate changes.
- Seam to honor: `compile_context` is the production SessionStart surface (the priming measurement
  MUST go through it, not `find_skill`); priming is bounded/zero-touch (<500ms, no LLM on hot path).
- Instrument home rule (T20): extend the shared lib + fixture; do NOT spawn a new one-off script family.
- Deletion test: the priming metrics (set-coverage / freshness / judge) are NEW additions to
  `retrieval_metrics.py`; the task metrics (mrr/ndcg/candidate-recall/sign_test/crater_check) stay as-is.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| A | Pre-registration (metrics + ROI thresholds + judge rubric + negative-control design) | infra-packet | The locked ruler T12 is graded against; owner sign-off gate | **draft-staged (awaiting owner lock)** | 0 | unit-A-preregistration-DRAFT.md |
| B | Author session-start stratum (thin/vague + VERBOSE substrata; multi-gold; anti-circularity probe) | infra-packet | The priming distribution the T11 fixture lacks | **HELD (pending owner go)** | -- | -- |
| C | Negative-control gate run (wrong-scope/permuted prime must crater set-coverage; runs FIRST) | infra-packet | Proves the coverage metric is non-vacuous before any verdict | **HELD (pending owner go)** | -- | -- |
| D | Baseline prime measured through `compile_context` (incl. verbose no_match quantified); raw artifacts | infra-packet | T12's honest before-number | **HELD (pending owner go)** | -- | -- |

## Owner decision points (STOP and ask) — THIS IS THE HOLD
1. **Lock the pre-registration?** Review `unit-A-preregistration-DRAFT.md` — metric definitions,
   per-signal ROI thresholds (numbers are the owner's to confirm; pre-registration discipline requires
   them locked BEFORE the stratum is authored and BEFORE any data), the judge rubric, and the
   negative-control design. On go, it is committed verbatim into the T18 ticket and Units B–D run.
2. **Negative-control flavor:** permutation control (query-vs-other-query's-gold, no second corpus —
   available now) as primary, vs cross-project wrong-scope (needs a second corpus; ties to T19/T25).
3. **Headline N for set-coverage@N:** tie to the production `compile_context` injection cap (confirm
   the cap), reporting the coverage curve around it.

## Learnings Brief
_Unit A drafted (pre-registration). Units B–D held at the owner gate. Live stack was down at staging
time (HTTP 000) — a go-time prerequisite (up + /health 200 + qwen3 pinned + corpus = 262), not needed
to produce the pre-registration._
