---
unit: "T02 — Local Qwen3 embedder backend and rebuild safety"
unit_number: 1
unit_kind: expansion
serves: "Slice 2 — better dense retrieval without external APIs, delivered safely (no mixed vectors, loud dimension guards, observable model metadata). Model-backend foundation T04 compares against."
status: completed
attempt_count: 1
domains: [infrastructure, embeddings, qdrant, graph-builder, mcp-server, migrations, measurement]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/02-qwen3-embedder-rebuild-safety.md
session_id: work-2026-06-09-084626
approval_sensitive: true
owner_approval: "User approved batch 2 (2026-06-09) + chose model-keyed-collection coexistence strategy."
---

## What Was Implemented

Made `qwen3-embedding:4b` a measurable LOCAL dense-retrieval arm alongside `nomic-embed-text`, with no mixed-vector corruption. nomic stays the default.

- **Embedder configurable from env:** `embedding_model_from_env()` (mcp-server) + graph-builder `main.rs` read `OLLAMA_EMBED_MODEL`, treating blank (`${VAR:-}` docker interpolation) as absent, defaulting to `nomic-embed-text`. `docker-compose.test.yml` passes `OLLAMA_EMBED_MODEL` through to both `mcp-server` and `graph-builder`.
- **Live dimension discovery:** `OllamaEmbeddingService::discover_dimension()` probes a real embed call and returns `EmbeddingModelInfo { model_name, dimension }` from the actual vector length (not hardcoded). Confirmed live: nomic=768, qwen3-embedding:4b=2560.
- **Model-keyed Qdrant collection (owner-approved design):** `model_keyed_collection_name("skills", model) -> skills__<slug>` so nomic(768) and qwen(2560) collections coexist; arm switch selects a different collection, no clobber. `QDRANT_COLLECTION` test-isolation override (#164) still wins when set.
- **Fail-loud dimension guard:** `QdrantError::DimensionMismatch { collection, observed, expected }`; `ensure_collection` now parses the existing collection's `config.params.vectors.size` and errors instead of silently reusing a wrong-dimension collection. Propagated as FATAL in both mcp-server and graph-builder (not swallowed by Qdrant degraded-mode warnings).
- **Migration 008** (`008_embedding_model_metadata.sql`): `embedding_model_metadata` table (single active row) recording model_name, dimension, collection, model_digest, updated_at.
- **Report arm block extended:** `scripts/retrieval_quality_live.py` `build_arm_metadata()` now includes the live-probed `dimension`.

## Files Changed
- `crates/infrastructure/src/vector/qdrant.rs` — model_keyed_collection_name, DimensionMismatch, dimension guard, 3 new tests
- `crates/infrastructure/src/embeddings/ollama.rs` — EmbeddingModelInfo, discover_dimension, 2 new tests
- `crates/infrastructure/src/lib.rs` — exports
- `crates/infrastructure/src/persistence/postgres.rs` — registered migrations 007 + 008; ordering test → 001..008
- `crates/infrastructure/migrations/008_embedding_model_metadata.sql` — created
- `crates/mcp-server/src/lib.rs` — embedding_model_from_env, build_qdrant_adapter_with_model, dimension logging, 3 new tests
- `crates/graph-builder/src/main.rs` — OLLAMA_EMBED_MODEL env, discover_dimension, model-keyed collection
- `docker-compose.test.yml` — OLLAMA_EMBED_MODEL pass-through (both services)
- `scripts/retrieval_quality_live.py` — dimension in arm block + live Ollama probe
- `tests/e2e/reports/v17-qwen3__held_out.json`, `v17-nomic__held_out.json` — live arm reports (gitignored)

## Problems Encountered / Scope Notes
- **Blank env from docker-compose `${VAR:-}`** emits `""`, which `std::env::var` returns as `Ok("")` (not `Err`). Fixed all readers to treat blank as absent (use `env_or`/trim-empty), matching the existing helper semantics.
- **SCOPE EXPANSION (flagged to owner): orphaned migration 007.** `007_skill_generality.sql` shipped from prior #172/#178 work but was NEVER registered in the migration runner (neither the `include_str!` const nor the `MIGRATIONS` array entry existed) — so the `generality`/`generality_rationale` columns were never being created. The agent registered both 007 and 008 to close the gap. The migration is idempotent + additive (`IF NOT EXISTS`, NULL default, no table rewrite), so applying it is safe; but it DOES newly enable that dormant schema. This is approval-sensitive (schema migration) and is a separate concern from T02 — surfaced in the orchestrator report for owner awareness.

## Patterns Discovered
- docker-compose `${VAR:-}` → `Ok("")`; treat blank as absent everywhere.
- Migration registration is MANUAL (`MIGRATIONS` const in postgres.rs); no auto-discovery — a shipped `.sql` is dormant until registered (this is how 007 was orphaned).
- Model-keyed collection (`skills__<slug>`) + observed-vs-expected dimension guard makes silent wrong-dimension reuse impossible.

## TDD Evidence
- **Red:** `cargo test -p infrastructure "vector::qdrant::tests"` (pre-impl) → FAIL (compile): `cannot find function 'model_keyed_collection_name'`; `no variant named 'DimensionMismatch'`. Behaviors absent.
- **Green:** qdrant 14/14 (incl. model_keyed_collection_name, ensure_collection_fails_loud_on_dimension_mismatch, ensure_collection_succeeds_when_existing_dimension_matches); embeddings 7/7 (incl. discover_dimension, EmbeddingModelInfo); mcp-server embedding_model 3/3 (unset/blank/qwen). Verified by orchestrator independently.
- **Post-Refactor Green:** after blank-env fix + migration registration + fmt → infrastructure embeddings 7/7, graph-builder 17+2, mcp-server lib all pass. Re-run by orchestrator: green.
- **E2E (real server, owner-approved live run):**
  - qwen arm: `arm={backend:snapshot_dense, embedder_model:qwen3-embedding:4b, dimension:2560, ...}`, held-out judge-aug MRR=0.767, nDCG@3=0.709, no_match=1.0, latency mean 312ms / p95 409ms; gate PASSED. Server log: `collection: skills__qwen3-embedding-4b`.
  - nomic default confirmed still works: `dimension:768`, MRR=0.767, nDCG@3=0.749, no_match=1.0, latency ~123ms; `collection: skills__nomic-embed-text`.
  - HONEST DELTA: qwen is neutral on MRR, slightly WORSE on nDCG@3 (0.709 vs 0.749), and ~3× slower. Not faked green. Recorded as a real, useful negative/neutral result; nomic remains the better default arm so far.

## Test Results
- Rust unit gate (orchestrator-run): infrastructure embeddings 7/7; qdrant 14/14; graph-builder 17+2; mcp-server embedding_model 3/3. fmt clean on touched crates.
- E2E: both arms measured on the real mcp-server; gate exit 0.
- Attempts: 1

## Known Pre-Existing Debt (NOT introduced by T02 — surfaced per honesty rule)
- `cargo clippy -p infrastructure --all-targets` fails with 6 `await_holding_lock` errors in `crates/infrastructure/src/scope.rs` TEST code (lines 335–555), last modified by commit 7483309 (v1.5.1). Pre-dates T02; does not block `cargo test`. Cleanup todo recommended (hold the MutexGuard in a sync block / drop before await, or use an async-aware mutex in those test helpers).
