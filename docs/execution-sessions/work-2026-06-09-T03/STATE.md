---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/03-expanded-skill-format-multiview-fields.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "## Execution Slices > Slice 3"
brainstorm_ref: null
started: 2026-06-09
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-06-09-T03
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: extend portable SKILL.md so retrieval can match tasks, exact artifacts, prerequisites, and invariants beyond name+description+tags — the source material for T04 (BM25/dense multi-view) and T05 (typed edges).
- Success-criteria focus: "Structured extraction format must grow before graph quality can grow" (Design Decision 5); roundtrip writer↔reader fidelity; backward compatibility.

### TDD Contract
- Effective mode: Ralph-driven TDD
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun
- Required evidence: unit (writer/parser/migration constants) + e2e (real writer→reader roundtrip; live migration apply/skip on live postgres)
- Exceptions: None

### Constitution Context
- v2.1.0. Approval-sensitive: schema migrations. OWNER APPROVED expanding T03 to persist multi-view fields to the `skills` row via migration 009 (user decision 2026-06-09).
- Portable scope preserved: new fields live in portable SKILL.md frontmatter, not harness sidecars.
- Human gate preserved: skills still land as `.pending`; no bypass of rename-to-approve.

### Architecture Handoff
- Artifact: plan-derived handoff (parent plan ## Architectural Context, ## Proposed V1.7 Architecture, ## Design Decisions).
- Feature homes: crates/session-extractor (writer), crates/graph-builder/src/extraction (parser), plus necessary cross-crate plumbing (crates/domain candidate struct, crates/infrastructure persistence migration + rebuild INSERT).
- Multi-view fields: use_when, avoid_when, artifacts, tools, invariants, requires, produces (+ optional edge_hints). All OPTIONAL; missing => empty/default, never failure.
- Migration floor is 008 -> new migration is 009_*, WRITE-AHEAD pattern (mirror 007): populated via rebuild INSERT, reader deferred to T04/T05, no production SELECT yet.
- Inherited 228-244 obligations: bump migration_set_is_ordered_001_through_008 -> _009 and its id/sql lists; bump live_run_migrations_applies_then_skips_on_second_boot to assert all 9 ids. Columns on existing `skills` table => no new TRUNCATE_ALL_TABLES_SQL entry needed (skills already listed).
- Do NOT stuff unbounded bodies into one embedding view (multi-view bounded text only). Do NOT concatenate full bodies.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T03 Expanded skill format & multi-view extraction fields (+ skills-row persistence) | expansion | source data for T04 hybrid retrieval + T05 typed edges | completed | 1 | unit-01-expanded-skill-format-multiview-fields.md |

## Learnings Brief
- [persistence] Adding required fields to a shared struct (`LiveGraphSkillRecord`) breaks every construction site — validate with `cargo test --workspace --all-targets --no-run`, never crate-scoped. The first agent missed admin + ~13 fixtures by scoping its validation.
- [persistence] WRITE-AHEAD schema (007 precedent) is the honest pattern for columns written before any reader exists: nullable, idempotent `pg_attribute` guard, documented future consumer. Migration 009 follows it; reader deferred to T04/T05.
- [extraction] `SynthesisCandidate` wire type in `seams.rs` must carry any new candidate field or it is silently dropped after synthesis.
- [gate] `cargo clippy --workspace --all-targets -D warnings` is RED at branch HEAD (0cf77a6) due to PRE-EXISTING dead-code in `tests/e2e/harness/` + `tests/e2e/support/` (QdrantObserver/RedisObserver/run_docker/ScopeEnvGuard etc.). Proven via stash. Not a T03 regression; T03-owned crates are clippy-clean. Likely a real merge blocker the owner must clear separately.
- [env] Live Postgres for `--ignored` tests: host port 15432 (`postgres://skill_layer:skill_layer@127.0.0.1:15432/skill_layer`), container `dynamic-agent-skill-layer-postgres-1`. The live migration test self-isolates in a scratch schema.
