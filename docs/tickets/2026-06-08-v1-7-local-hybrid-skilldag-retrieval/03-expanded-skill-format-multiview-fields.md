---
ticket_id: T03
title: Expanded skill format and multi-view extraction fields
kind: expansion
status: completed
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
source_packet_ref: "## Execution Slices > Slice 3"
feature_home: "crates/session-extractor and crates/graph-builder/src/extraction"
depends_on:
  - T02
dependency_type: hard
serves:
  - Better dense and sparse matching source data
  - Typed edge construction source data
files:
  - docs/reference/skill-md-format.md
  - crates/session-extractor/src/writer.rs
  - crates/infrastructure/src/extraction/prompt_contract.rs
  - crates/graph-builder/src/extraction/rules.rs
  - tests/integration/test_skill_md_roundtrip.rs
test_command: "cargo test -p session-extractor && cargo test -p graph-builder && cargo test -p maintenance --test test_skill_md_roundtrip"
tdd_mode: ralph
---

# Expanded skill format and multi-view extraction fields

## Serves

Extend portable `SKILL.md` data so retrieval can match tasks, exact artifacts, prerequisites, and invariants without relying only on `name + description + tags`.

## Scope

- Add optional structured fields/sections such as `use_when`, `avoid_when`, `artifacts`, `tools`, `invariants`, `requires`, and `produces`.
- Update extraction writer output and graph-builder parser to preserve the fields.
- Update the canonical format doc.
- Preserve backward compatibility for existing skills.

## Scope Fence

- Do not require migration of every existing `SKILL.md` before retrieval works.
- Do not bypass `.pending` approval.
- Do not stuff full unbounded bodies into one embedding view.

## Acceptance Criteria

- Real writer output roundtrips through the real parser.
- Missing optional fields parse as empty/default, not failure.
- Old skills remain valid.
- The docs state which fields feed retrieval views and which are advisory only.

## Shared / Global Notes

`SKILL.md` is the portable interchange format. Add optional fields conservatively and document reader precedence to avoid writer/reader drift.

## Local Context

- WHY source: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`.
- This ticket serves: create the source material needed for multi-view dense retrieval, BM25 documents, and typed edge proposals.
- Existing contract: YAML frontmatter is authoritative for `name`, `description`, and `tags`; body bullets become subunits.
- Important unknown: extraction prompt wording should stay provider-agnostic and should not make local extraction brittle.

## Inherited Changes — V1.7 batch 1-2 triage (todos 228-244)

These landed on `feat/v-1-7` during the 228-243 triage swarm (2026-06-09) and bind this ticket:

- **Migration floor is now 008.** Any new migration this ticket adds is `009_*` and MUST bump BOTH the ordering test `migration_set_is_ordered_001_through_008` AND the live-count test `live_run_migrations_applies_then_skips_on_second_boot` (now asserts all 8 IDs — a hardcoded count gate, #238). Add any new persisted table to the `TRUNCATE_ALL_TABLES_SQL` const (one source of truth for runtime truncate + its test, #228/#238) or e2e isolation breaks.
- **Migration 007 (`generality`, `generality_rationale` on `skills`) is now ACTIVE but WRITE-AHEAD** (ratified #233): populated today via `.pending` SKILL.md frontmatter (`session-extractor/src/writer.rs`) + the LLM verifier, NOT the `skills` row; no production reader. If this ticket makes any multi-view field canonical in the `skills` row, it OWNS wiring the reader/writer and updating the rebuild INSERT — see the WRITE-AHEAD comment in `007_skill_generality.sql`.
- **`embedding_model_metadata` is now written** (UPSERT `key='active'` per rebuild, #228) — available if this ticket's reports/roundtrip tests want honest model attribution.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`

## Deeper-Dive Refs

- `docs/reference/skill-md-format.md`
- `docs/assessments/2026-06-08-local-vs-cloud-extraction-gap-214.md`
- `~/.claude/projects/-home-rabak-projects-dynamic-agent-skill-layer/memory/extraction-caps-align-to-window-not-footguns.md`

## Coupling Notes

T04 consumes these fields for lexical documents; T05 consumes them for typed edge proposal signals. Keep schema changes explicit and approval-sensitive.
