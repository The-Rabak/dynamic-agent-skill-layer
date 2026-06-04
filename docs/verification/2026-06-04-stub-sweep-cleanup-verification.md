# Stub/fake/placeholder cleanup — logged verification (2026-06-04)

Sequential execution of the cleanup todos filed by the 2026-06-04 project-wide stub sweep, each with
logged, real-infra verification. Order: #158 → #155 → #159 → #160 → #161.

Standing rule enforced throughout: no stubs/fakes/placeholders in production logic paths; fail loud instead.
(See `~/.claude/CLAUDE.md` machine-wide rule + project memory `no-stubs-in-production-paths`.)

---

## #158 — eradicate the 8-dim stub embedder from all 3 production paths — ✅ RESOLVED

**Fix:** Replaced `DeterministicEmbeddingGenerator` (8-dim token-hash sync stub) with the real async
`infrastructure::OllamaEmbeddingService` (`nomic-embed-text`, 768-dim) in the graph-builder rebuild loop,
the admin "trigger full rebuild" tool, and the maintenance merge/retire passes. `build_skills_from_scope_roots`
is now `async`, takes `&dyn domain::EmbeddingService`, embeds via one order-preserving `embed_batch`, and
**fails loud** on any embedding error (new `GraphBuildError::Embedding`). Deleted the sync `EmbeddingGenerator`
trait and the 8-dim stub. Added a test-only deterministic **768-dim** `EmbeddingService` (`DeterministicEmbeddingService`)
for offline tests. Fixed `graph-builder/src/main.rs` `ensure_collection(..., 8)` → `768`. All three runtimes
already receive `OLLAMA_URL` in their compose env — **no docker-compose/env change required**, and production
boot now fails loud if `OLLAMA_URL` is unset (no fallback embedder).

**Files:** `crates/graph-builder/src/graph/{embeddings,build,rebuild}.rs`, `crates/graph-builder/src/main.rs`,
`crates/admin/src/tools.rs`, `crates/mcp-server/src/admin_wiring.rs`, `crates/maintenance/src/runtime.rs`,
+ 7 test files updated to the new signatures (offline → deterministic 768-dim double; live e2e → real Ollama).

### Static verification
- `cargo build --workspace --tests` → green.
- `cargo test -p graph-builder -p admin -p maintenance --lib` → `23 passed; 0 failed`.
- `rg 'DeterministicEmbeddingGenerator' crates/ tests/` → empty (type deleted).
- `rg 'vec![0.0_f32; 8]|trait EmbeddingGenerator|impl EmbeddingGenerator' crates/*/src` → empty (8-dim stub + sync trait gone).
- `rg 'ensure_collection.*, *8)' crates/*/src` → empty (no 8-dim collection creation).

### Live verification (real containers, docker-compose.test.yml stack)
Baseline (old binary, up 56m): Qdrant `skills` collection 768-dim, **points_count: 0**; outbox `vector.upsert`
= 7 failed + 1 pending, **all 8-dim**, `last_error = "qdrant endpoint returned unexpected status 400 Bad Request"`.

1. Rebuilt the graph-builder image with the fixed binary (`docker compose build graph-builder` → green, release musl).
2. Recreated the container → healthy; boot logs show it connecting to `ollama:11434` (real embedder) and `qdrant:6333`.
3. Seeded a NEW global skill (`qdrant-writeside-liveproof-158`) into the real `test-global-skills` volume via sidecar.
4. The real graph-builder rebuild (poll loop) embedded it: `graph rebuilt graph_version:31 skills_count:7`, published `graph.rebuilt:31`.
5. **Result:** a new `vector.upsert` event with **dim=768, status=published**; Qdrant **points_count: 0 → 1**.
6. Deleted the 8 stale 8-dim residue events (old-binary garbage, unrecoverable), seeded a 2nd new skill, let it rebuild.
7. **Final clean state:** outbox `vector.upsert` = **8 events, all dim=768, all `published`** (0 failed, 0 pending);
   Qdrant **points_count: 8 (768-dim)** == active skill count (8). Write-side fully alive.

**Conclusion:** the production write path now emits real 768-dim Ollama embeddings end-to-end; Qdrant durable
write-side accepts them; `points_count` tracks the active skill set. The stub is gone from every production path
and cannot be re-wired without re-adding a deleted type. Verified against the real running stack.

_Note: maintenance merge/retire path is fixed in code (real embedder injected, fails loud on missing `OLLAMA_URL`)
and unit-green, but is not exercised by the test stack (the `maintenance-worker` runs only in the prod compose);
its live exercise is out of scope for this stack._
