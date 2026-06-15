# T12 work prompt — trigger-aware retrieval priming mechanism (Batch 20)

Run `/workflows:work` on ticket
`docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/12-trigger-aware-retrieval-priming-mode.md`.
Branch `feat/v-1-7`. Suggested session id: `work-2026-06-15-t12-priming`.

## Mission

Build the trigger-aware priming MECHANISM and make the production SessionStart prime actually useful —
measured against T18's locked instrument and thresholds. This is the highest-value real-usage retrieval
fix and it **blocks T15** (the primary efficacy gate must measure compounding *through* a working
priming path, not around it). T18 already proved the current prime is weak (set-coverage@3 = 0.0685)
and narrowed the design space — do NOT re-discover that; build on it.

## Read first (in order)

1. The T12 ticket — esp. `## What T11 settled` (candidate-recall is the lever; ranking is inert at
   262; no new candidate sources; bounded primed set) and the `## Scope` mechanisms.
2. **T18 results** — `docs/tickets/.../18-priming-instrument-session-start-stratum.md` `## Results &
   T12 hand-off`, the session `docs/execution-sessions/work-2026-06-14-224314-T18/`, and the LOCKED
   pre-registration (`unit-A-preregistration-DRAFT.md`) — these thresholds grade your work; cite them
   verbatim, do not change them.
3. Memory `v17-t18-priming-instrument-baseline` and `v17-t11-hybrid-verdict-dense-views-win`.
4. Plan `## Agent usefulness targets` + `## Retrieval Flow` (compile_context is zero-touch, <500ms).

## T18 hand-off — the three measured constraints (build to these, don't relitigate)

1. **Raising N is INERT.** The 0.48 relevance floor caps the candidate pool at ≤3 for session-start
   queries (diagnostic curve flat @3=@5=@8). A bigger `max_results` window buys nothing. The lever is
   an **intent-conditional floor** (Priming intent uses a lower/no floor over a bounded top-N) and/or a
   **recurrence/freshness signal that surfaces below-threshold skills**.
2. **Verbose openings fail by DILUTION, not no_match** (verbose no_match 9% < thin 18%, but verbose
   coverage 0.027 vs thin 0.110). The matched remedy is **query-side multi-view / max-over-segments**
   (segment the verbose prompt, embed segments, max-over-segments) — the symmetric twin of T09's
   doc-side multi-view win. Don't chase a no_match that mostly isn't there on this distribution.
3. **Latency already breaches budget:** verbose p95 = 734ms vs the 500ms SessionStart limit. Your
   changes must IMPROVE (or at least not worsen) verbose latency — **no LLM call on the hot path**;
   segmentation must be cheap.

Before-number to beat: **set-coverage@3 = 0.0685**. The locked recurrence-baseline keep-threshold is
**+0.10 absolute (→ ≥0.17)** by paired sign test p<0.05; freshness +0.15 hit-rate with ≤0.02 coverage
cannibalization; centrality/recent-use +0.043 (default DROP — T11 ranking-inert). Separation S=0.0365
was small, so the absolute floors govern.

## Scope (the ticket's units — implement to its Acceptance Criteria)

- Typed `RetrievalIntent` (`Priming` vs `Task`) threaded through the retrieval orchestrator + the
  compiler SessionStart path — NOT another env flag. **`Task` path byte-identical** (proven by the
  existing quality gate staying green).
- FIRST: fix verbose priming via the mechanisms above (intent-conditional floor and/or query-side
  multi-view). Priming ranker = recurrence-baseline project skills + a bounded freshness slot.
- Centrality/recent-use ONLY if they clear their pre-registered bars; default DROP; record the delta.
- Re-measure on the T18 instrument: re-run the **negative control** with the new ranker, then
  baseline-vs-primed paired + sign-test on the session_start stratum; cite T18 thresholds verbatim;
  persist raw artifacts. Keep/drop each signal on the measured delta.

## Owner decision points (STOP and ask)

1. **Default-ON flip:** if the new priming ranker clears its thresholds, whether it ships as the
   production SessionStart default (the T11-style "owner flag flip") — present the measured
   primed-vs-baseline + the sign-test before flipping.
2. Present the per-signal keep/drop verdicts (with deltas) before finalizing which signals ship.

## Hard fences

- **Do NOT naively lower the global 0.48 floor** — it protects the T11-measured negative rejection
  (no_match precision 0.92 on `Task`). Any floor change is **Priming-intent-scoped only**, measured.
- No new candidate sources / no broadened candidate generation (T11 constraint). Bounded primed set —
  never "inject more context" as a strategy. No cross-project recurrence (T19, deferred).
- SessionStart p95 within 500ms; no LLM on the hot path. `Task` path unchanged.
- No efficacy claims here (that's T15); this ticket measures the priming instrument only.

## Standing rules (this is a Rust crate change — heavier than T18)

- **Serialize ALL heavy actions.** T12 touches `crates/retrieval` + `crates/compiler` → cargo
  build/clippy/test required. Run builds ONE AT A TIME; **NO parallel execution agents** (the WSL2
  crash rule); only the orchestrator builds. Execution agents on **sonnet**.
- **Ralph TDD** (ticket tdd_mode: ralph): failing unit tests first → minimal impl → refactor →
  post-refactor green; unit + e2e evidence. Measurement drives the REAL mcp-server over HTTP.
- After crate changes you MUST rebuild + restart mcp-server to measure:
  `docker compose -f docker-compose.test.yml build mcp-server && docker compose -f docker-compose.test.yml up -d mcp-server`,
  then gate on `/health` ready before measuring.
- Commit early (WSL2 unflushed-write loss). Never delete this session's outputs. Workspace gates
  (clippy both forms + fmt) stay green. Pin `OLLAMA_EMBED_MODEL=qwen3-embedding:4b`.

## Mechanics (verified this session — reuse, don't rediscover)

- Stack is UP. mcp-server on **:3001** (graph-builder :8080). Measurement endpoint
  `http://127.0.0.1:3001/mcp` JSON-RPC `tools/call`. `compile_context` args `{prompt, session_id
  (UNIQUE per call — dedup), repo_path:"project"}`; injected skills parse from `## Skill:` headers in
  `result.additional_context`. Corpus = 262, snapshot_dense, `max_results` default 3.
- The T18 instrument: `scripts/retrieval_metrics.py` (`set_coverage_at_n`, `freshness_hit_rate`,
  `crater_check`, `sign_test`) + the `session_start` stratum (kind=="session_start", 22 queries) in
  `tests/fixtures/retrieval_quality_262_corpus_labeled.json`. Raw baseline artifacts under
  `tests/e2e/reports/retrieval/`. PG creds `skill_layer`/`skill_layer_test`.

When deploying subagents, append the project CLAUDE.md "Code Search" (semble) section to their prompts.
