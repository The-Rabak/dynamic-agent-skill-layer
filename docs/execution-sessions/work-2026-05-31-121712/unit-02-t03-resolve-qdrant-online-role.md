---
unit: "T03 — Resolve Qdrant's online role (Option A + CQRS docs + honest health)"
unit_number: 2
unit_kind: hardening
serves: "SC-V1.5-F (honest dispositions / no stub paths) + plan SC-2/SC-8"
status: completed
attempt_count: 1
domains: [rust, retrieval, infrastructure, health, docs, architecture]
batch: 2
plan_file: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
ticket_file: docs/tickets/2026-05-31-skill-layer-v1-5/03-resolve-qdrant-online-role.md
session_id: work-2026-05-31-121712
---

## What Was Implemented
- `healthy_markers()`/`degraded_marker()` in `crates/retrieval/src/orchestrator.rs` no longer claim `qdrant: "ok"`/`postgres: "ok"` (also dropped `redis`) on the read path; added `skill_snapshot_sync: ok` to report CQRS read-model status. Doc blocks added.
- Named deletion-guard test `read_path_health_markers_do_not_claim_qdrant_or_postgres_as_live_dependencies` (fails if the false claim reappears).
- `crates/retrieval/src/qdrant_search.rs`: doc comment marking `search_qdrant` as purely in-memory cosine over `RetrievalSnapshot` (kept the name to avoid breaking call sites; explains it + references ADR-0001/Option B).
- `crates/infrastructure/src/health.rs`: renamed the Qdrant probe key `"qdrant"` → `"qdrant_write_side"` with an explanatory comment (not a read-path dependency).
- Created `docs/architecture/adr-0001-online-graph-source-v1-5.md` (Option A; consequences = 5000 cap + Qdrant unused at read time; V2 trigger for Option B = cap exceeded OR team-scope; DS-003 contract).
- Created `docs/reference/online-retrieval-cqrs.md` (write/read split, honest health, DS-003 resilience contract, V2 path).
- DS-003 contract DEFINED (not implemented): Qdrant down ⇒ `compile_context` still `Ok`/`NoMatch`, only `qdrant_write_side` degrades. T10 implements the test.

## Files Changed
- `crates/retrieval/src/orchestrator.rs` — health markers + named test
- `crates/retrieval/src/qdrant_search.rs` — doc comment (in-memory clarification)
- `crates/infrastructure/src/health.rs` — `qdrant` → `qdrant_write_side` label
- `docs/architecture/adr-0001-online-graph-source-v1-5.md` — created
- `docs/reference/online-retrieval-cqrs.md` — created

## TDD Evidence
- **Red:** `cargo test -p retrieval read_path_health_markers...` → FAIL: panic "healthy_markers must not include 'qdrant'" — proves the false read-path claim existed.
- **Green:** `cargo test -p retrieval` → 16 passed (incl. new test) after dropping qdrant/postgres + adding skill_snapshot_sync.
- **Post-Refactor Green:** `cargo test -p retrieval` → 16 passed after `cargo fmt -p retrieval` (formatting only, no behavior change).

## Test Results
- Command: `cargo test -p retrieval`; Result: PASS (16/16); Attempts: 1. Re-verified by orchestrator in combined-tree run.

## Notes for Review
- Ticket listed `dual_scope.rs` + `infrastructure/src/vector/qdrant.rs` as files; the actual in-memory search lives in `qdrant_search.rs` and the false marker in `orchestrator.rs`/`health.rs`, so the agent labelled where the claim actually was. No online Qdrant query introduced; `SkillRetriever` trait untouched.
- Agent also dropped `redis` from markers (justified: not a read-path dep). Within the "honest health" intent; flag for review confirmation.
- Pre-existing rustfmt drift in `infrastructure` files (qdrant.rs, extraction/*) left untouched per scope fence.
