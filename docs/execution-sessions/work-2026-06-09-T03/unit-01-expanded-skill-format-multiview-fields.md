---
unit: "T03 — Expanded skill format and multi-view extraction fields (+ skills-row persistence)"
unit_number: 1
unit_kind: expansion
serves: "Source data for T04 hybrid dense/BM25 retrieval and T05 typed-edge proposals (plan Design Decision 5)."
status: completed
attempt_count: 1
domains: [extraction, persistence, graph-builder, docs]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/03-expanded-skill-format-multiview-fields.md
session_id: work-2026-06-09-T03
---

## What Was Implemented

Seven OPTIONAL multi-view fields — `use_when`, `avoid_when`, `artifacts`, `tools`, `invariants`, `requires`, `produces` — wired end-to-end through the real producer→consumer chain, plus persisted to the Postgres `skills` row via migration 009 (owner-approved persistence scope):

- **LLM prompt contract** (`crates/infrastructure/src/extraction/prompt_contract.rs`): fields added to text prompt + system prompt + JSON schema, all OPTIONAL (not in `required`) so local models that omit them still produce valid candidates.
- **Domain** (`crates/domain/src/types.rs`): `ExtractedSkillCandidate` gains `#[derive(Default)]` + 7 `#[serde(default)] Vec<String>` fields.
- **Writer** (`crates/session-extractor/src/writer.rs`): `PendingDraftFrontmatter` gains the 7 fields with `skip_serializing_if = is_empty` so empty fields emit no YAML key (backward compat). Also `seams.rs` `SynthesisCandidate` carries them across the synthesis wire boundary.
- **Reader** (`crates/graph-builder/src/extraction/rules.rs`): `SkillFrontmatter` + `StructuralExtraction` gain the 7 fields; frontmatter authoritative; fields never leak into subunits. Carried through `mod.rs` `SkillExtraction`, `graph/build.rs` `BuiltSkill`, `graph/rebuild.rs` mapping.
- **Persistence** (`crates/infrastructure/src/persistence/rebuild.rs`): `LiveGraphSkillRecord` gains the 7 fields; `INSERT INTO skills` binds them `$7..$13` (null-when-empty). `crates/admin/src/tools.rs` rebuild trigger carries them through too.
- **Migration 009** (`crates/infrastructure/migrations/009_skill_multiview_fields.sql`): 7 nullable `TEXT[]` columns, idempotent per-column `pg_attribute` guard, WRITE-AHEAD header mirroring 007 (no production reader yet — T04/T05 will SELECT). Registered in `postgres.rs` `MIGRATIONS`; ordering test renamed `_001_through_009`; live apply/skip test asserts all 9 IDs.
- **Docs** (`docs/reference/skill-md-format.md`): canonical example with new fields, WRITE-AHEAD note, classification table (retrieval-destined vs advisory-only), reader precedence, contract-test section.
- **Tests** (`tests/integration/test_skill_md_roundtrip.rs`): 2 new tests — populated fields survive real-writer→real-reader and never leak into subunits; absent fields emit no YAML key and parse back as empty.

## Files Changed

See execution reports. Production: domain/types.rs, prompt_contract.rs, writer.rs, seams.rs, skeleton.rs, orchestrator.rs, rules.rs, mod.rs, build.rs, graph/rebuild.rs, persistence/rebuild.rs, postgres.rs, admin/tools.rs, migration 009. Docs: skill-md-format.md. Tests: test_skill_md_roundtrip.rs + ~13 e2e/integration fixture literals updated for the new required `LiveGraphSkillRecord`/`ExtractedSkillCandidate` fields.

## Problems Encountered

### Problem 1: Missed `LiveGraphSkillRecord` construction sites (regression)
- **Error:** `E0063: missing fields ... in initializer of LiveGraphSkillRecord` at `crates/admin/src/tools.rs:284` and ~13 e2e test fixtures.
- **Root cause:** First execution agent validated with crate-scoped tests, not `cargo test --workspace --all-targets`, so it missed every other construction site of the shared struct.
- **Fix:** Orchestrator caught it via full-workspace `--no-run`. Fixed admin (carry-through from `BuiltSkill`) directly; delegated the ~13 test-fixture repairs to a second sonnet execution-agent (empty vecs for fixtures, carry-through for the one `BuiltSkill`-sourced closure). `Default` deliberately NOT added to `LiveGraphSkillRecord`/`ScopeType` (silent-default footgun vs fail-loud rule).

### Problem 2: `clippy::large_enum_variant` on `MapOutcome::Skeleton`
- **Fix:** Boxed the variant; dereferenced at the orchestrator match arm.

## Patterns Discovered

- Adding required fields to a shared struct breaks ALL construction sites — always validate with `cargo test --workspace --all-targets --no-run`, not crate-scoped.
- WRITE-AHEAD schema (007 precedent) is the honest pattern for columns written before a reader exists: nullable, idempotent `pg_attribute` guard, documented future consumer. Real persisted data, not a stub.
- A `SynthesisCandidate`-style wire type at an LLM boundary must carry new fields or they're silently dropped post-synthesis.

## Test Results

- `cargo fmt --check`: PASS
- `cargo clippy -p domain -p infrastructure -p session-extractor -p graph-builder -p admin --all-targets --features test-utils -- -D warnings`: PASS (T03-owned crates clean)
- `cargo test --workspace --all-targets --features test-utils --no-run`: PASS (compiles)
- `cargo test -p domain`: PASS
- `cargo test -p infrastructure --lib persistence::postgres`: PASS (10 passed, 3 ignored-live)
- `cargo test -p graph-builder --lib`: PASS (19 passed, incl. 2 new multi-view parser tests)
- `cargo test -p session-extractor`: PASS (166 passed)
- `cargo test -p maintenance --test test_skill_md_roundtrip --features test-utils`: PASS (3 passed, incl. 2 new)
- **LIVE e2e:** `cargo test -p infrastructure ... live_run_migrations_applies_then_skips_on_second_boot -- --ignored` against real Postgres (host :15432): PASS — migration 009 applies on first boot, skips on second, all 9 IDs asserted.

## Known Gaps / Caveats (honest)

- **Pre-existing merge blocker (NOT T03):** `cargo clippy --workspace --all-targets -D warnings` is RED due to dead-code lints in `tests/e2e/harness/` + `tests/e2e/support/` (`ScopeEnvGuard`, `QdrantObserver`, `RedisObserver`, `run_docker`, etc.). PROVEN pre-existing by stashing T03's edits and re-running clippy on clean HEAD (0cf77a6) — same/more errors. T03-owned crates are clippy-clean.
- **Live-rebuild column persistence not asserted end-to-end:** migration applies live and the INSERT binds the columns (compiles), but no live reader asserts the persisted values yet — by design, T04/T05 add the reader (WRITE-AHEAD). The writer→reader roundtrip of the SKILL.md fields IS proven live via the real-writer/real-reader integration test.
