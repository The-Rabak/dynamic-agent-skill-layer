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

## Lifecycle filenames

`crates/domain/src/lifecycle_files.rs`:

- `SKILL.md` — active/approved skill (the only file the graph-builder ingests).
- `SKILL.md.pending` — extracted draft awaiting the human rename-to-approve gate.
- `SKILL.md.rejected` — tombstone blocking re-proposal.
- `SKILL.md.retired` — retirement marker.

## Contract test

`tests/integration/test_skill_md_roundtrip.rs` drives the **real** writer to
produce a `SKILL.md` on disk, then feeds that exact file to the **real** reader
and asserts `description` / `tags` / procedures survive. It must use real writer
output — never a hand-authored fixture, which is exactly the substitution that
hid #224.
