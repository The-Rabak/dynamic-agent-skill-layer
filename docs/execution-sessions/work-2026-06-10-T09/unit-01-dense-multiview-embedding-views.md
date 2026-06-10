---
unit: "T09 Dense multi-view embedding views (e_task/e_needs/e_negative)"
unit_number: 1
unit_kind: expansion
serves: "Let DENSE retrieval exploit T03's multi-view fields (not just sparse/BM25); unblocks the full hybrid verdict in T11"
status: completed
attempt_count: 2
domains: [retrieval, embeddings, mcp-server, measurement-harness]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/09-dense-multiview-embedding-views.md
session_id: work-2026-06-10-T09
---

## What Was Implemented

Dense multi-view embedding views, built from T03's structured fields, behind a default-OFF flag.

- **New `crates/retrieval/src/dense_views.rs`** — single source of truth for the view→field mapping (mirrors how `bm25::skill_lexical_document` centralizes the BM25 field policy):
  - `SkillDenseViewFields<'a>` input struct.
  - `build_e_task` (use_when + subunit procedure text + artifacts + tools), `build_e_needs` (requires + invariants), `build_e_negative` (avoid_when). Each bounded to `DEFAULT_DENSE_VIEW_CHAR_CAP` (4096, env-overridable `DENSE_VIEW_CHAR_CAP`, fail-loud parse), char-boundary-safe truncation, whitespace-normalized.
  - `fuse_dense_views(prompt, e_summary_cosine, e_task_emb, e_needs_emb) -> f32` = **max-over-views** of {e_summary, e_task, e_needs}. `e_negative` has **no parameter** — structurally (compile-time) excluded from the positive α fusion, per the plan (it's a conflict/negative signal, not a positive match view).
- **`crates/retrieval/src/orchestrator.rs`** — `SeededSkill` gains `e_task_embedding`/`e_needs_embedding`/`e_negative_embedding` (in-memory only). New `BoolFlag` newtype (fail-loud `FromStr`: accepts `1/true/on/yes` + `0/false/off/no`, rejects anything else) and `RetrievalConfig.dense_views_enabled` (default `false`) parsed from `RETRIEVAL_DENSE_VIEWS`. New `DenseViewsMetadata` (view_names, embedding_dim, skill_count_with_views) on `RetrievalSnapshot` + `with_dense_views_metadata`; surfaced in `RetrievalOutcome.health` markers (`dense_views_built/dim/skill_count`).
- **`crates/retrieval/src/dual_scope.rs`** — the α (`l1_semantic`) seam in BOTH `perform_scope_search` (snapshot arm) and `perform_scope_search_with_qdrant_candidates` (qdrant arm): when `dense_views_enabled` → `fuse_dense_views`, else the **exact** original `e_summary` cosine expression. Flag OFF ⇒ byte-for-byte unchanged ranking.
- **`crates/mcp-server/src/lib.rs::build_graph_from_pg`** — builds the three view texts per skill via the shared helper, embeds each in its own `embed_batch` with a **fail-loud length guard** (mirrors the existing e_summary/subunit guards), populates the new `SeededSkill` fields, and attaches `DenseViewsMetadata` to the snapshot. Views are built **unconditionally** at boot (T04 BM25 precedent) so the ON/OFF sweep is a pure mcp-server env-flip + restart — no graph rebuild between arms.
- **Measurement harness (T01 surface, extended for T09's required `test_command`):**
  - `docker-compose.test.yml` — forwards `RETRIEVAL_DENSE_VIEWS: ${RETRIEVAL_DENSE_VIEWS:-}` to the mcp-server container.
  - `scripts/retrieval_quality_sweep.py` — `RETRIEVAL_DENSE_VIEWS` added to `_ARM_ENV_KEYS` so the harness forwards it; the dense-views ON/OFF arm flips this var + `reboot_mcp`.

## Files Changed
- `crates/retrieval/src/dense_views.rs` — created
- `crates/retrieval/src/lib.rs` — module + re-exports
- `crates/retrieval/src/orchestrator.rs` — SeededSkill fields, BoolFlag, RetrievalConfig flag, DenseViewsMetadata, health markers, tests
- `crates/retrieval/src/dual_scope.rs` — α fusion seam (both arms), test fixtures
- `crates/retrieval/src/graph_search.rs` — test fixture update
- `crates/mcp-server/src/lib.rs` — T09 embedding block + fail-loud guards + metadata attach
- `crates/retrieval/src/sparse.rs` — incidental `cargo fmt` cleanup (pre-existing T04 whitespace debt in the same crate)
- `docker-compose.test.yml` — RETRIEVAL_DENSE_VIEWS passthrough
- `scripts/retrieval_quality_sweep.py` — dense-views arm env key

## Problems Encountered
### Problem 1: `.unzip3()` does not exist in Rust std
- **Error:** method not found building three per-skill view-text vectors.
- **Root cause:** triple-tuple iterators can't be `.unzip3()`.
- **Fix:** manual `for` loop pushing to three pre-allocated `Vec`s.

### Problem 2: ~17 existing `SeededSkill` test literals broke after adding 3 fields
- **Error:** "missing fields e_needs_embedding, e_negative_embedding, e_task_embedding".
- **Root cause:** struct-literal construction in tests across dual_scope.rs/graph_search.rs.
- **Fix:** added `Vec::new()` for the three new fields to every existing test construction.

### Problem 3 (orchestrator, post-agent): flaky latency test + fmt
- **Error:** `*_meets_parallel_latency_envelope` failed once under load (1/78); pre-existing `cargo fmt` debt in T05/T04 files.
- **Root cause:** timing-sensitive envelope test (flaky, passed on rerun); T04/T05 commits were not fmt-clean.
- **Fix:** confirmed green on clean rerun (78/0). Formatted only the T09 crates (`retrieval`, `mcp-server`); left the unrelated pre-existing fmt debt in `graph-builder/edges.rs`, `infrastructure/rebuild.rs`, `qdrant.rs` for a separate cleanup (flagged below).

## Patterns Discovered
- `BoolFlag` newtype is now a reusable fail-loud boolean env-flag primitive exported from the `retrieval` crate.
- Build-views-unconditionally-at-boot (gate only the READ) mirrors the T04 BM25 index pattern and keeps arm sweeps a pure env-flip.
- **FINDING (separate from T09):** the workspace currently fails `cargo fmt --check` at HEAD due to un-formatted code committed in the T04/T05 batches (`crates/graph-builder/src/graph/edges.rs`, `crates/graph-builder/src/graph/rebuild.rs`, `crates/infrastructure/src/persistence/rebuild.rs`, `crates/infrastructure/src/vector/qdrant.rs`). This is a pre-existing final-gate blocker, not introduced by T09. Needs a dedicated fmt cleanup commit before the V1.7 final gate.

## Test Results
- Command: `cargo test -p retrieval && cargo test -p mcp-server --lib`
- Result: PASS (retrieval 78/0; mcp-server --lib 33/0) — independently re-run by the orchestrator post-format.
- Attempts: 2

## TDD Evidence
- **Red**
  - Command: `cargo test -p retrieval` before the impl existed.
  - Result: FAIL (compilation: `BoolFlag`/`DenseViewsMetadata` undefined; missing `SeededSkill` view fields). The behavioral `dense_views::tests` (field inclusion/exclusion, boundedness, max-fusion, empty-views==e_summary) define the contracts.
  - Evidence-quality note: in Rust, adding a struct field makes the Red a compile failure rather than a pure behavioral red. The behavioral guarantees are nonetheless proven by Green (the fusion + flag-OFF-invariant tests are real behavioral assertions).
- **Green**
  - Command: `cargo test -p retrieval && cargo test -p mcp-server --lib`
  - Result: PASS — 78 + 33. `dense_views_default_is_false_preserving_pre_t09_behaviour` + `fuse_dense_views_with_empty_extra_views_returns_summary_cosine` prove the flag-OFF invariant; `fuse_dense_views_returns_max_over_positive_views` proves ON fusion.
- **Post-Refactor Green**
  - Command: `cargo test -p retrieval` after `cargo fmt -p retrieval -p mcp-server`.
  - Result: PASS (78/0; one flaky latency-test blip resolved on clean rerun). Format-only change; behavior preserved.

## Problem 4 (live bring-up): blank-view embedding crashed mcp-server at boot
- **Error:** mcp-server `Exited (1)` at boot with `Error: "embedding input is invalid: text input must not be blank"`.
- **Root cause:** dense views build UNCONDITIONALLY at boot. The current 234-corpus has ALL multi-view fields empty (confirmed in PG: use_when/avoid_when/requires/invariants populated count = 0), so `build_e_needs` (requires+invariants) and `build_e_negative` (avoid_when) produce blank text. The T09 boot block embedded the three view-text batches wholesale (`embed_batch(&["",""...])`), and the Ollama embedder's fail-loud guard correctly rejects a blank string → boot crash. A skill with no requires/invariants/avoid_when legitimately has NO e_needs/e_negative — that is ABSENCE, not an error.
- **Fix (committed staged, type-checks clean):** new `embed_dense_view_skipping_blank` helper in `crates/mcp-server/src/lib.rs` — embeds only the non-blank view texts and scatters results back into a full-length vector; blank positions get an empty `Vec<f32>`. `fuse_dense_views` already treats an empty view embedding as absent (0.0 → max falls back to e_summary), so flag-ON on a sparse corpus == flag-OFF == baseline. Fail-loud preserved on the non-blank count. Dim capture now scans the first non-empty vector across all three views.
- **Status:** staged in working tree; `cargo check -p mcp-server` green. NOT yet rebuilt into the container (folding into one rebuild after the extraction-prompt redesign is reviewed, per owner decision to pivot to building the multi-view extraction prompts first).

## Live Real-Server Sweep (orchestrator-driven — separate from the agent's unit work)
- Status: PENDING — blocked on the rebuild (deferred) and superseded in priority by the owner's pivot: the current corpus has EMPTY multi-view fields because NO extraction prompt elicits them (T03 wired plumbing only). Building the multi-view extraction prompts (ground-up redesign) is now the active work; the dense-views ON/OFF sweep on an empty-field corpus would be a confirmed ≈0 by construction. Meaningful validation moves to the freshly-extracted multi-view corpus. The dense-views ON/OFF arm is wired into the harness and runnable; the orchestrator runs the live sweep after committing the implementation (commit-first to protect against the documented WSL2 dirty-tree crash risk).
- Expectation (owner-acknowledged): the current 234-skill held-out corpus predates T03, so most multi-view fields are empty → the measured ON-vs-OFF delta is expected ≈ 0. Default stays OFF regardless. The meaningful multi-view validation is **T11** (depends on T10's multi-view-rich corpus + T09 + T06).
