---
unit: "T05 Typed skill graph storage and cold-start edge proposals"
unit_number: 1
unit_kind: expansion
serves: "Typed graph edges as SEPARATE evidence (matches/neighbors/conflicts) for SkillDAG-style retrieval — unlocks T06; never a ranking multiplier (#208 lesson)."
status: completed
attempt_count: 2
domains: [domain-model, graph-builder, persistence, migrations]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/05-typed-skill-graph-edge-storage.md
session_id: work-2026-06-10-T05
---

## What Was Implemented

Typed inter-skill graph edge storage + deterministic cold-start edge proposals.

1. **Domain relation semantics (centralized, single source of truth)** — `EdgeType`
   (`depends_on`, `specializes`, `composes_with`, `similar_to`, `conflicts_with`) and
   `EdgeOrigin` (`cold_start_deterministic`, `cold_start_proposal`, `manual`,
   `agent_derived`) enums in `crates/domain` with `as_db_str`/`from_db_str` (fail-loud
   parse), `is_walkable` (only `conflicts_with` is non-walkable), `is_backbone`
   (`depends_on`/`specializes`), and `is_trusted` (proposals are not trusted).

2. **Schema** — migration `010_skill_edges.sql` (salvaged from the first, crashed agent
   dispatch; reviewed and kept): `skill_edges` table with typed/origin CHECK
   constraints, FK to `skills` `ON DELETE CASCADE`, `UNIQUE(source,target,edge_type)`,
   confidence/reason/JSONB-evidence/timestamps, source/target indexes.

3. **Cold-start edge construction** (`crates/graph-builder/src/graph/edges.rs`) —
   deterministic, structured-field-only (no external API):
   - `depends_on` from `requires`↔`produces` token overlap (exact match → confidence
     ≥ 0.9 → auto-committed as `cold_start_deterministic`; owner decision 2026-06-10).
   - Mutual overlap (A↔B) demoted to a single `composes_with` edge (proposal) so the
     directed-acyclic backbone is preserved without silently dropping signal.
   - `similar_to` from `tools`/`artifacts` Jaccard ≥ 0.5 (proposal tier unless 1.0).
   - `validate_backbone_acyclic` fails loud on backbone cycles (2-cycle and longer
     chains) and self-loops — the "invalid cycles fail clearly" acceptance contract.
   - `build_validated_cold_start_edges` = generate + validate (used by rebuild).

4. **Persistence** (`crates/infrastructure/src/persistence/rebuild.rs`) —
   `LiveGraphEdgeRecord` (write DTO) + `PersistedGraphEdgeRecord` (read DTO);
   `PostgresRebuildCoordinator::replace_skill_edges` (replace-semantics, own tx after
   skills are committed, `ON CONFLICT` idempotent, JSONB evidence);
   `PostgresGraphSnapshotStore::list_skill_edges` (observability — all origins).

5. **Rebuild wiring** — `PostgresDurableGraphState::persist_graph_mutation` now generates
   validated cold-start edges from the built skills and persists them after the snapshot
   commit. A cycle/self-loop fails the rebuild loudly rather than committing a bad graph.

6. **Migration registration + gates** — registered `010` in `MIGRATIONS` +
   `include_str!`; bumped `migration_set_is_ordered_001_through_010`, the live count gate
   `live_run_migrations_applies_then_skips_on_second_boot` (all 10 ids), added
   `skill_edges` to `TRUNCATE_ALL_TABLES_SQL` + its guard test, added
   `migration_010_declares_skill_edges_table_with_typed_constraints` structural test.

## Files Changed
- `crates/domain/src/types.rs` — `EdgeType` + `EdgeOrigin` enums and semantics
- `crates/domain/src/lib.rs` — export `EdgeType`, `EdgeOrigin` + 6 unit tests
- `crates/graph-builder/src/graph/edges.rs` — NEW: proposer + validator + 10 unit tests
- `crates/graph-builder/src/graph/mod.rs` — declare `edges` module
- `crates/graph-builder/src/graph/rebuild.rs` — generate + persist edges in write path
- `crates/infrastructure/src/persistence/rebuild.rs` — edge DTOs, write/read methods, live roundtrip test
- `crates/infrastructure/src/persistence/postgres.rs` — register migration 010, update gates/tests
- `crates/infrastructure/src/lib.rs` — export edge DTOs
- `crates/infrastructure/migrations/010_skill_edges.sql` — typed-edge schema (salvaged + kept)

## Decisions (owner-approved 2026-06-10)
- **Postgres-only** edge storage (no filesystem export this slice).
- **Auto-commit high-confidence** deterministic edges (≥0.9 → trusted); lower-confidence
  stay observable proposals. All carry origin + evidence + confidence.
- Separate `replace_skill_edges` method (not a new `LiveGraphSnapshotMutation` field) to
  avoid touching ~13 mutation construction sites — smaller, isolated diff.

## Scope honesty / deferrals (not stubs — documented)
- `specializes` and `conflicts_with` are valid stored/classified/validated edge types but
  have no reliable deterministic structured signal, so T05 does not auto-PROPOSE them;
  reserved for richer signals / agent classification in T06+. The types, storage, and
  semantics are fully real and exercised by tests.
- A separate append-only edge mutation-history table is deferred to T06 (when manual/agent
  edits, which actually need mutation history, land). T05 records created_at/updated_at on
  each edge per "store … timestamps".

## Test Results
- `cargo test -p graph-builder` — PASS (28 unit incl. 10 edge tests; 5 e2e ignored=live)
- `cargo test -p infrastructure persistence` — PASS (36; 3 ignored=live)
- `cargo test -p domain` — PASS (13 incl. 6 edge-semantics tests)
- `cargo check --workspace --all-targets` — PASS (no consumer regressions)
- `cargo clippy -p domain -p infrastructure -p graph-builder --lib -D warnings` — clean
  for all three touched crates (pre-existing `too_many_arguments` in untouched
  `crates/retrieval/src/dual_scope.rs:454` is NOT from this work).

## TDD Evidence
- **Red**: edge-semantics + proposer/validator unit tests written first; before the
  `EdgeType`/`EdgeOrigin` impls and `edges.rs` existed they fail to compile/resolve
  (missing behavior, not env noise). Migration gates `*_001_through_010` /
  `migration_010_*` fail before 010 is registered.
- **Green**: all unit suites above PASS after implementation (28 / 36 / 13).
- **Post-Refactor Green**: after cleaning the `let _` workaround in the mutual-overlap
  branch and finalizing exports, full re-run stayed green + clippy clean on touched crates.

## Live verification — PASSED against real Postgres (2026-06-10, docker now up)
Run against `docker-compose.test.yml` postgres on `127.0.0.1:15432`:
- `live_run_migrations_applies_then_skips_on_second_boot` (asserts all 10 ids apply then
  skip) — **PASS**
- `live_replace_and_list_skill_edges_roundtrip` (type/origin/reason/JSONB-evidence persist
  + conflicts_with stored + replace-not-append semantics) — **PASS**
- Full live persistence suite (4 tests incl. pre-existing rollback + embedding-metadata) —
  **PASS, no regression**.
Command: `DATABASE_URL=postgres://skill_layer:skill_layer@127.0.0.1:15432/skill_layer_test \
  cargo test -p infrastructure --lib persistence -- --ignored --test-threads=1`

## Patterns Discovered
- The rebuild write path fully DELETEs+re-INSERTs skills each cycle, and `skill_edges` FKs
  cascade — so deterministic cold-start edges are correctly regenerated every rebuild;
  any future durable (manual/agent) edges must NOT rely on survival across rebuilds.
- Migration count gates live in 3 places in `postgres.rs` (ordering test, live id-list,
  TRUNCATE guard) — all three must move together (the #238 hardcoded-count precedent).
