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

---

## #155 — strengthen DS-003..007 dream-state tests — ⚠️ PARTIAL (honesty core done + verified; deep brutalization remains)

This todo mixes two layers. The **test-honesty core** (the "no more lies" part) is fixed and verified live.
The **architectural brutalization** (migrate DS-003..007 off in-process `from_environment` onto the real-app
HTTP harness; real PG/Redis/process-kill fault injection; drift-at-scale; the actual p95 perf fix) is a large
V2 effort — effectively the full brutal-suite plan — and is honestly NOT done here.

### Honesty defects — FIXED + verified
- **`report.rs::build()`** already derives the overall outcome from real assertion results (failed assertion
  → Failed; failed section → Failed; nothing-recorded → Failed; else Passed) and exposes `assert_contract`
  for inline derivation. Verified by reading `tests/e2e/report.rs:202-229` — the keystone honesty was already
  in place (prior #095/#145 work). No hardcoded outcome remains in the builder.
- **DS-003 silent-skip** (the `if let Some(qdrant_write_component)` flagged by the sweep) is already safe: a
  hard `assert!(qdrant_write_component_present, …)` at `test_dream_state_contract.rs:304` runs BEFORE the
  `if let`, so the health-degradation assertion can never be silently skipped; the 2s sleep was already
  replaced by `poll_until` bounded polling.
- **DS-006** (`sustained_watcher_and_extraction_saturation…`): replaced `assert!(ok_count + no_match_count > 0)`
  (which masked the ok=0/no_match=N failure mode) with a brutal `assert!(ok_count > 0, …)` + an
  `assert_contract("saturation_yields_ok_retrievals", …)`; removed the hardcoded `AssertionResult::Passed`.
- **DS-007** (`high_qps_compile_context_load_meets_p95_and_error_budget_targets`): replaced the `TODO(2.x)` +
  hardcoded `Passed` with explicit, fail-able assertions on a p95 budget (500ms) AND an error budget (0
  Degraded under fault-free load), both recorded via `assert_contract` and enforced with `assert!`.

### Live verification (real stack, `--ignored`, env per scripts/run-e2e-tests.sh)
- `cargo build --tests` → green.
- **DS-006 live → PASS**: the `ok_count > 0` brutal assert holds (saturation yields real OK retrievals).
  `test result: ok. 1 passed`.
- **DS-007 live → FAIL (honest, intended)**: the new p95 assert fires on a REAL measurement —
  `p95 latency 1147ms exceeds the 500ms warm-path budget (p50=682ms p99=1191ms max=1191ms min=170ms)`.
  Previously this scenario ALWAYS passed (hardcoded). It now correctly fails, exposing the serialized
  embedding hot-path bottleneck. Per the mandate, it stays red until the perf is actually fixed. (Default CI
  runs `--skip ignored`, so this does not break the default gate; the `--ignored` live tier shows the true gap.)

### Remaining (large, NOT done — tracked, keep #155 open)
- Migrate DS-003..007 off in-process `McpServerApp::from_environment` onto the real-app HTTP harness
  (`tests/e2e/harness/`, drive the real mcp-server :3001) — the "really e2e" requirement.
- Real fault injection: DS-003 PG+Redis faults; DS-004 real process-kill + multi-restart of relay/server with a
  real backlog (assert replayed==enqueued, 0 lost/dup, seeded skills retrievable); DS-005 inject known PG/Qdrant
  divergences + real `OutboxReconciler::reconcile_once`, assert gaps_closed==gaps_injected at scale.
- DS-007 PERFORMANCE FIX: the actual embedding hot-path remediation (cache / warmed session-start) so the p95
  budget is met, plus explicit warm-vs-cold regime separation. The honest test above now drives this work.

---

## #159 — measurement-ground the two unmeasured placeholder timeout constants — ✅ RESOLVED

**Fix:** The two production timeout constants previously self-documented as "UNMEASURED placeholders" are now
grounded in a REAL measurement on the reference host and reframed as deliberate conservative ceilings (not
latency targets). Also fixed a model-name inconsistency (`granite4:3b` → `gemma4:e4b`, the actual default).

**Measurement (live stack, host Ollama :11444, `gemma4:e4b` ~9.6GB CPU, moderate extraction prompt, num_predict=256):**
- cold-start (model load) generation: **65.6s**
- warm generation: **37.2s, 37.2s** (consistent across runs)

This validates `timeout_ms: 120_000` (inner extraction ceiling) as ~1.8× observed cold-start headroom, and
`DEFAULT_TIMEOUT_SECS: 180` (outer worker-pool) as the required 1.5× margin. Values unchanged — they were
defensible; only the "unmeasured/placeholder" framing and the stale model name were wrong.

**Files:** `crates/infrastructure/src/extraction/ollama.rs:37-42` (comment rewritten with the measurement),
`crates/session-extractor/src/worker_pool.rs:8-13` (comment rewritten, model fixed). Env/builder overrides
verified real: `OLLAMA_EXTRACTION_TIMEOUT_MS` (providers/ollama.rs:17) and
`ExtractionWorkerPoolConfig::with_timeout` (worker_pool.rs:45).

**Verification:** `rg -ni 'unmeasured|placeholder' crates/*/src | grep -iv test` → only the benign
`rebuild.rs:256` comment that describes using a real version *instead of* a hardcoded placeholder.
`cargo build -p infrastructure -p session-extractor` → green.

---

## #160 — live data-plane suite: silent pass + fixed sleeps — ✅ RESOLVED (honesty); surfaced real bug #162

**Fix (`tests/e2e/test_live_data_plane_roundtrip.rs`):**
- **Silent pass:** the `extract_session_inline_live` contract was recorded `Passed` UNCONDITIONALLY even when
  no `.pending` draft was written. Replaced with a real derivation: `pending_written && origin_ok` →
  `assert!` + `assert_contract` (fails loud when no draft).
- **Fixed sleeps:** replaced all three `thread::sleep(Duration::from_secs(2))` (qdrant-stopped, ollama-stopped,
  qdrant-restarted) with bounded polls on the real condition (Qdrant `/collections` / Ollama `/api/tags`
  reachable/unreachable, 30 × 500ms), mirroring the existing Phase-5 readiness poll. Removed the now-unused
  `thread` import.
- **Unbacked claim:** Phase 4 recorded a hardcoded `Passed` describing "compile_context still Degraded" without
  checking. Added a real `compile_context` call + `assert!` + `assert_contract`
  (`qdrant_restore_alone_does_not_recover_read_path`) that proves it.

**Live verification (real stack, `--ignored`):** `test result: FAILED. 4 passed; 1 failed`.
- **4 PASS** — including `degraded_and_recovery_cycle_preserves_reason_codes_and_recovers_cleanly`, which
  exercises ALL THREE new bounded-poll fault injections AND the new Phase-4 still-Degraded check. The sleep→poll
  and unbacked-claim fixes are verified working.
- **1 FAIL (intended, honest)** — `extract_session_live_inline_payload_writes_pending_and_emits_completion_events`
  now fails loud: `pending_written=false … extraction produced nothing`. The previously-hardcoded `Passed` was
  hiding that **live inline extraction writes no `.pending` draft**. This is a REAL defect the honesty fix
  exposed; filed as **#162** (P1). Per the no-stubs mandate, the test stays red until the real extraction bug is
  fixed — not by relaxing the assertion.

**Net:** #160's honesty scope is complete and verified (no silent pass, no fixed sleeps, no unbacked Passed).
The one now-failing test is the honest surfacing of #162, not a regression introduced by this change.
