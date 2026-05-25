---
status: complete
priority: p1
issue_id: "025"
tags: [code-review, architecture, graph-builder, scope-drift]
dependencies: []
---

# Restrict watcher/builder to skill file contract

## Problem Statement

Watcher and graph build currently treat any `.md` file as a skill artifact, causing broad scope pollution (docs/readmes/etc.) and violating the intended skill-file boundary.

## Findings

- `crates/graph-builder/src/watcher.rs:307-309` defines skill files as any `.md` or `.pending`.
- `crates/graph-builder/src/graph/build.rs:48-54` ingests every `.md` during graph build.
- `crates/graph-builder/src/main.rs:12-29` defaults project scope to repo root and global to `docs`, amplifying accidental ingestion.
- Architecture/constitution describe explicit skill-state files (`SKILL.md`, `.pending`, `.retired`) rather than arbitrary markdown ingestion (`docs/constitution.md:65-67`).

## Proposed Solutions

### Option 1: Enforce explicit filename/state matcher

**Approach:** Accept only `SKILL.md`, `SKILL.md.pending`, and lifecycle variants that match the contract; ignore unrelated markdown.

**Pros:**
- Restores architecture fidelity.
- Prevents false-positive graph content and noisy rebuilds.

**Cons:**
- Requires fixture and test updates.

**Effort:** 3-5 hours

**Risk:** Medium

---

### Option 2: Restrict scanning roots to dedicated skill directories + keep extension filter

**Approach:** Keep broad extension logic but scope watch/build roots to `.skills/`-style directories only.

**Pros:**
- Smaller code change.
- Reduces current blast radius quickly.

**Cons:**
- Still weaker contract enforcement than explicit filename/state validation.

**Effort:** 2-3 hours

**Risk:** Medium

## Recommended Action

Implemented Option 1 with explicit contract matching in watcher/build plus integration coverage that proves non-skill markdown exclusion and `.pending -> .md` approval handling.

## Technical Details

**Affected files:**
- `crates/graph-builder/src/watcher.rs`
- `crates/graph-builder/src/graph/build.rs`
- `tests/integration/test_watcher_rebuild.rs`
- `tests/fixtures/test-skills/**`

**Database changes (if any):**
- No

## Resources

- `docs/tickets/2026-05-21-skill-layer-v1-1/05-watcher-driven-graph-rebuild.md`
- `docs/constitution.md`
- `crates/graph-builder/src/watcher.rs`

## Acceptance Criteria

- [x] Non-skill markdown files are excluded from watcher and graph build inputs
- [x] Skill-state filename contract is encoded in tests
- [x] Rebuild/audit events remain correct for `.pending -> .md` approvals
- [x] No architecture drift from feature-home file-state semantics

## Work Log

### 2026-05-25 - Review synthesis

**By:** Copilot CLI

**Actions:**
- Traced file matcher and build ingestion rules in graph-builder.
- Compared matcher behavior to constitution and ticket contracts.

**Learnings:**
- Extension-only matching risks major retrieval pollution and operational overhead.

### 2026-05-25 - Implementation completed

**By:** Copilot CLI

**Actions:**
- Updated graph-builder watcher matching to accept only `SKILL.md`, `SKILL.md.pending`, and `SKILL.md.retired`.
- Updated graph build ingestion to index only active `SKILL.md` files.
- Updated integration fixture layout to contract-style `*/SKILL.md` files and added non-skill markdown fixture files for exclusion checks.
- Extended watcher rebuild integration test to assert non-skill markdown exclusion while preserving `.pending -> .md` approval rename behavior.

**Verification:**
- `cargo test -p graph-builder && cargo test --test test_watcher_rebuild`

## Notes

- WHY classification: 🎯 PROTECTS USER STORY.
- Architecture drift: feature-home contract violation.
