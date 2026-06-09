---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/02-qwen3-embedder-rebuild-safety.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "## Execution Slices > Slice 2"
brainstorm_ref: null
started: 2026-06-09T05:46:26Z
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-06-09-084626
batch: 2
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: Make `qwen3-embedding:4b` a measurable local dense-retrieval ARM (not the default) without external APIs or mixed-vector corruption, so T04 can compare arms honestly.
- Success-criteria focus: Qwen arm returns correctly-dimensioned embeddings; rebuild fails loud on dimension divergence; reports identify embedder model + dimension; nomic path still works; LIVE held-out report proves the qwen arm on the real server.

### TDD Contract
- Effective mode: Ralph-driven TDD (plan overrides local). Loop: red-green-refactor.
- Required evidence: Unit (dimension/metadata guards, model-keyed collection naming, env wiring) + E2E (real server: reboot mcp-server with OLLAMA_EMBED_MODEL=qwen3-embedding:4b, rebuild graph with qwen vectors, gated held-out find_skill report). User-approved live run.
- Exceptions: None.

### Constitution Context
- Version 2.1.0. T02 IS approval-sensitive (embedding model change + schema migration + Qdrant collection/dimension change). USER APPROVED proceeding with batch 2 (2026-06-09) AND chose the coexistence strategy: model-keyed collection + dimension discovered from the live model.
- Local-first preserved: qwen3-embedding:4b is local Ollama; no external API. nomic stays DEFAULT (scope fence).
- No stubs/fail-loud: dimension mismatch and mixed-vector states MUST fail loud, never silently reuse.

### Architecture Handoff
- Feature homes: crates/infrastructure/src/embeddings/ollama.rs, crates/graph-builder/src/graph/build.rs, crates/mcp-server/src/lib.rs, crates/infrastructure/migrations/.
- DECISION (user-approved): Qdrant collection is MODEL-KEYED (e.g. `skills__<model-slug>`) with vector size = the dimension DISCOVERED from the live Ollama embed response (NOT hardcoded). nomic (768) and qwen (2560) collections coexist side-by-side; switching arm = different collection, no clobber, no re-embed of the other arm.
- Persist embedding model identity to PG (new migration): model name, dimension, and digest where available — readable by graph/reports.
- Fail-loud guard: `ensure_collection` currently returns early on an existing collection WITHOUT checking dimension (qdrant.rs:130-ish) — add an explicit observed-vs-expected dimension check so a wrong-dim collection cannot be silently reused.
- Current hardcodes to replace: mcp-server build_embedding_service() model:"nomic-embed-text" (lib.rs:674); ensure_collection("...",768) at lib.rs:642 and qdrant.rs:669/695.
- Seam: live measurement still drives the real mcp-server over HTTP via find_skill; reports carry the arm block (extend T01's `arm` with the discovered `dimension`).

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T02 Local Qwen3 embedder backend + rebuild safety | expansion | Measurable local qwen dense arm + dimension/metadata safety; unlocks T04 honest arm comparison | completed | 1 | unit-01-qwen3-embedder-rebuild-safety.md |

## Result Summary
- qwen3-embedding:4b arm measured live (2560-dim, model-keyed collection `skills__qwen3-embedding-4b`): held-out judge-aug MRR 0.767, nDCG@3 0.709, no-match 1.0, latency ~312ms mean / 409ms p95. nomic default confirmed still working (768-dim, MRR 0.767, nDCG@3 0.749, ~123ms). HONEST DELTA: qwen neutral-to-worse + 3x slower → nomic stays the better default arm so far. Not faked green.
- Scope note (flagged to owner): registered orphaned migration 007 (skill_generality) alongside new 008; idempotent + additive, safe, but approval-sensitive + adjacent concern.
- Pre-existing debt surfaced (NOT T02): 6 clippy `await_holding_lock` errors in infrastructure/src/scope.rs test code (commit 7483309).

## Learnings Brief (carried from batch 1)
- `OLLAMA_EMBED_MODEL` is NOT yet read by the server — build_embedding_service() hardcodes nomic-embed-text. T02 wires it.
- T01 added the report `arm` block {backend, embedder_model, dense, sparse, rerank} + `latency_ms`. T02 should add `dimension` to the arm block and ensure the qwen arm report shows model + dimension.
- Compose `ollama-model-check` only pulls nomic; qwen3-embedding:4b (~2.5GB) must be pulled into the stack's ollama (host ollama port is 11444→11434; stack uses internal ollama:11434).
- Baseline to beat (nomic snapshot_dense held-out): judge-aug MRR 0.767, no-match 1.0.
