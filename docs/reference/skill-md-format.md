# SKILL.md format contract

This is the **single, authoritative** on-disk format for a skill. Every producer
(the session-extraction writer, the maintenance merge/promote proposers, test
harnesses, demo scripts) emits this format, and the single consumer (the
graph-builder reader) parses it. The contract exists because a silent divergence
between writer and reader once dropped the `description` of 229/234 corpus skills
(#224): the writer emitted YAML frontmatter, the reader scanned the body as if
there were none and captured the opening `---` fence as the description.

## Canonical format

```markdown
---
name: http-router-security-defaults
description: Apply mandatory security defaults to every HTTP router.
tags:
- security
- http
generality: general            # optional: project | general | uncertain
generality_rationale: ...       # optional
origin: session_extraction      # optional provenance
source_session_id: ...          # optional
source_provider: ...            # optional
created_at: ...                  # optional lifecycle (RFC 3339)
warning_at: ...                  # optional
expires_at: ...                  # optional
# --- multi-view optional fields (T03) ---
use_when:                        # optional: advisory conditions that call for this skill
- Configuring a new HTTP service from scratch
avoid_when:                      # optional: advisory conditions where this skill is wrong
- The service already has a security middleware layer
artifacts:                       # optional: notable outputs or deliverables the skill produces
- secure-headers.toml
tools:                           # optional: tools / CLIs / libraries the skill directly invokes
- rustls
invariants:                      # optional: conditions that must remain true when skill applies
- Security headers must be set before any route handler fires
requires:                        # optional: prerequisites before applying this skill
- An HTTP framework with middleware support
produces:                        # optional: named outputs this skill guarantees
- A hardened HTTP router with all security defaults set
---

# http-router-security-defaults

Apply mandatory security defaults to every HTTP router.

## Procedures
- Set secure response headers on every response
- Reject state-changing requests without a CSRF token

## Conventions
- Security middleware is registered before any route handler

## Assets
- (optional code snippets / references)
```

## Rules

1. **YAML frontmatter is the single source of truth** for `name`, `description`,
   and `tags`. The markdown body MUST NOT carry a `tags:` line — that duplication
   is what drifted before #224.
2. The **body** carries the human-readable `# title`, the description prose, and
   the `## Procedures` / `## Conventions` / `## Assets` / `## Evidence` /
   `## Summary` sections. Each `- ` bullet under a section becomes a retrievable
   subunit (ℓ₀). YAML list items inside the frontmatter are NOT subunits.
3. **Embedding (ℓ₁)** text = `name + description + tags` (see
   `crates/graph-builder/src/graph/build.rs`). A correct `description` is
   therefore load-bearing for retrieval — a dropped description degrades the
   largest scoring term (α).
4. **Multi-view optional fields** (`use_when`, `avoid_when`, `artifacts`, `tools`,
   `invariants`, `requires`, `produces`) are WRITE-AHEAD as of T03. They are
   populated by the extraction writer from model output, persisted in the `skills`
   table (nullable `TEXT[]` columns, migration 009), and forwarded through the
   graph-build pipeline. No production read path exists yet — T04/T05 will
   introduce retrieval views that consume these columns. See below for field
   classification and reader precedence.

## Multi-view fields: classification and reader precedence

These fields were introduced in T03 (migration 009) as a WRITE-AHEAD capability.
They carry advisory metadata emitted by the LLM during extraction. All 7 are
optional (`#[serde(default)]`): old SKILL.md files without them parse cleanly
(empty `Vec<String>`).

### Fields that WILL feed retrieval views (T04/T05 consumers)

| Field | Purpose | Planned view |
|-------|---------|--------------|
| `use_when` | Conditions that call for this skill | T04 contextual ranking |
| `avoid_when` | Conditions where this skill is wrong | T04 negative filtering |
| `requires` | Prerequisites before applying | T05 dependency graph |
| `produces` | Named outputs this skill guarantees | T05 dependency graph |

### Advisory-only fields (reference metadata, not indexed for retrieval)

| Field | Purpose |
|-------|---------|
| `artifacts` | Notable outputs or deliverables the skill produces |
| `tools` | Tools / CLIs / libraries the skill directly invokes |
| `invariants` | Conditions that must remain true when skill applies |

Advisory-only fields are stored in the `skills` table but are not currently
queried by any retrieval or ranking path. They exist as human-readable context
in the SKILL.md file and as queryable columns for exploratory use.

### Reader precedence for multi-view fields

`crates/graph-builder/src/extraction/rules.rs::extract_structural_subunits`:

- All 7 fields are **read from YAML frontmatter only**. They are never inferred
  from or present in the markdown body. Body subunit bullets cannot override them.
- If a field is absent from the frontmatter YAML, the reader defaults to an
  empty `Vec<String>` (`#[serde(default)]`). There is no body fallback.
- Empty multi-view fields are **not serialized** to the YAML frontmatter by the
  writer (`#[serde(skip_serializing_if = "<[String]>::is_empty")]`), so round-
  tripping a skill with no multi-view data produces an identical file without
  the fields.

## Reader precedence (graph-builder)

`crates/graph-builder/src/extraction/rules.rs::extract_structural_subunits`:

- Splits off the frontmatter (only when the file starts with `---\n` and a
  closing `\n---\n` follows) and scans the **body only** for subunits — so the
  fence lines and YAML list items can never leak into the description or subunits.
- Frontmatter `name` / `description` / `tags` are **authoritative** and override
  anything inferred from the body.
- `suggested_tags` is accepted as a **backward-compatibility alias** for `tags`
  (older pending drafts used it), so a graph rebuild recovers descriptions from
  the existing on-disk corpus without re-running extraction.
- **Body-only files** (no frontmatter) still parse via the body heuristic: H1 →
  name, `tags:` line → tags, first prose line → description.
- **Multi-view fields** (`use_when`, `avoid_when`, `artifacts`, `tools`,
  `invariants`, `requires`, `produces`) are read from frontmatter YAML only and
  default to empty `Vec<String>` when absent — there is no body fallback for them.

## Lifecycle filenames

`crates/domain/src/lifecycle_files.rs`:

- `SKILL.md` — active/approved skill (the only file the graph-builder ingests).
- `SKILL.md.pending` — extracted draft awaiting the human rename-to-approve gate.
- `SKILL.md.rejected` — tombstone blocking re-proposal.
- `SKILL.md.retired` — retirement marker.

## Contract tests

`tests/integration/test_skill_md_roundtrip.rs` drives the **real** writer to
produce a `SKILL.md` on disk, then feeds that exact file to the **real** reader
and asserts `description` / `tags` / procedures survive. It must use real writer
output — never a hand-authored fixture, which is exactly the substitution that
hid #224.

The suite includes three tests added in T03:

- `multiview_fields_round_trip_through_real_writer_and_reader`: drives the real
  writer with all 7 multi-view fields populated, reads back with the real reader,
  and asserts all 7 values survive intact with no leakage into subunits.
- `candidate_without_multiview_fields_round_trips_unchanged`: proves that empty
  fields are NOT serialized to YAML by the writer (no `use_when:` key on disk)
  and are parsed back as empty `Vec<String>` by the reader — preserving backward
  compatibility with old corpus files.
- The original `skill_md_roundtrip_preserves_description_and_tags` test continues
  to cover the pre-T03 contract.
