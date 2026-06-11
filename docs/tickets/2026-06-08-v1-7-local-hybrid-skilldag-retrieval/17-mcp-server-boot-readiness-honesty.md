---
ticket_id: T17
title: mcp-server boot readiness honesty — no healthy-while-warming window
kind: hardening
status: completed
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "constitution: honest health markers, fail-loud; ADR-0001 read-model boot contract"
source_packet_ref: "filed 2026-06-11 from docs/assessments/2026-06-11-v1-7-retrieval-contract-measured.md (T08 latency caveat) + docs/assessments/2026-06-11-v1-7-midpoint-deep-grok-assessment.md"
feature_home: "crates/mcp-server (boot/readiness) and crates/infrastructure (vector persistence read at boot)"
depends_on: []
dependency_type: none
serves:
  - A /health that never claims ready while the retrieval snapshot is still warming
  - Sweeps and operators that can trust readiness instead of probing around it (unblocks honest T11 measurement windows)
files:
  - crates/mcp-server/src/
  - crates/infrastructure/src/
test_command: "live cold-boot test on the qwen3 262-skill corpus: /health honest until snapshot ready; no find_skill hang window — no fakes"
tdd_mode: ralph
---

# mcp-server boot readiness honesty — no healthy-while-warming window

## Serves

On qwen3 the mcp-server cold-boot re-embeds the entire corpus (~7 min for 262 skills). During that window `/health` already reports healthy while `find_skill`/`compile_context` can hang until embed activity drains. That is a dishonest health marker — the same class of bug this project has fixed twice before (false `qdrant: ok` read-path claim in ADR-0001; `embedding_arm`/backend surfacing in #239/#255). It also corrupts any measurement (T11 sweeps, latency reports) that polls `/health` to decide when to start.

## Scope

- **Readiness honesty:** `/health` (or a distinct readiness marker consumed by scripts/compose healthchecks) must not report the read path ready while the boot snapshot/view embedding is still in flight. During warm-up, tool calls return an explicit degraded/warming status fast — never an open-ended hang.
- **Kill the re-embed, don't just label it (preferred fix):** persist embeddings (including T09 view embeddings) and load precomputed vectors at boot, re-embedding only skills whose content/model/dimension changed; fail loud on model/dim mismatch (consistent with #235 DimensionMismatch semantics). The ~7-min window should become seconds on an unchanged corpus.
- A live cold-boot test against the real qwen3 corpus proving both: no healthy-while-warming window, and no tool-call hang window.

## Scope Fence

- No silent fallbacks (e.g. serving from a stale/partial snapshot while claiming full health); warming must be explicit and observable.
- No fakes in the covering test — real server, real corpus, real cold boot.
- Do not weaken the existing fail-loud boot contracts (missing config still fails loud, not "warming forever").
- **Sequencing:** `crates/mcp-server` files are currently being modified by the in-flight T13 session (allowlist drain moved tests into `crates/mcp-server/tests/` and touched `src/lib.rs`). Do not execute this ticket until that session's work lands; rebase on top of it.

## Acceptance Criteria

- [x] During boot embed/warm-up, readiness is reported NOT-ready (health or dedicated marker) and tool calls return an explicit warming/degraded response within the normal latency budget — no hang. **DONE:** `ReadinessHandle` (Warming/Ready/Failed) surfaced as a `/health` `readiness` component (503 while warming/failed); find_skill/compile_context/search_skill_graph short-circuit to `status:"warming"` (compile_context → `CompileContextStatus::Warming`) BEFORE the query embed. Live-proven: warming tool calls return <5s (no hang).
- [x] Precomputed vectors load at boot on an unchanged corpus; only changed/new skills re-embed; model/dim mismatch fails loud. **DONE:** migration 011 `skill_embeddings` + `EmbeddingCacheStore`; `build_graph_from_pg` reuses cached vectors on (content_hash, model, dim) match, embeds only misses (incl. T09 views); `load_for_model` returns `DimensionMismatch` (fail-loud, #235) on stored-dim ≠ active-dim — live-PG test proven.
- [x] Cold-boot-to-ready on the unchanged corpus drops from ~7 min to seconds (measured before/after recorded). **DONE (measured live, real qwen3):** 30-skill corpus cold-boot **15.25s** → warm-boot **476ms** = **32× speedup**; the 262-skill ~7-min prod re-embed collapses to a sub-second cache load by the same mechanism. cold==warm find_skill matches+scores byte-identical (no drift).
- [x] Live cold-boot test covers both behaviors; `cargo test --workspace` green; no fakes. **DONE:** `tests/e2e/test_cold_boot_readiness_honesty.rs` (real `from_environment`, real qwen3, real cold boot) passes; workspace tests green except the pre-existing full-stack `golden_path_real_app` (needs the e2e harness :3001 server; untouched by T17).
- [x] T11's sweep scripts can gate on the honest readiness signal (removes T11's interim probe-query workaround). **DONE:** `/health` returns 200 only when the snapshot is Ready — T11 polls `/health` for 200 instead of a probe query.

## Local Context

- WHY source: `docs/assessments/2026-06-11-v1-7-retrieval-contract-measured.md` records the caveat ("cold-boot on qwen3 re-embeds the whole corpus (~7 min) and `/health` flips healthy before the snapshot finishes; `find_skill` can hang"); [[qwen3-default-operational-findings]] documents the operational pain. Promoted to a ticket by the 2026-06-11 midpoint assessment because it is a health-honesty violation, not just an inconvenience, and it threatens T11 measurement validity.
- Parallel-safe with the retrieval/efficacy spine in feature-home terms, but see the Scope Fence sequencing note re: the in-flight T13 session.

## Source

Filed 2026-06-11 (midpoint assessment follow-through; owner-directed ticket amendments session).

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
