---
ticket_id: T05-addendum
title: "Retro-authorization — Claude Code CLI extraction provider (host-only)"
kind: hardening # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: completed # ready | in_progress | blocked | completed
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/adr-0002-claude-code-cli-extraction-provider-v1-5.md
constitution_ref: docs/constitution.md # v2.1.0, 2026-06-02
governance_finding: todos/115-pending-p1-undocumented-claude-code-cli-provider-scope-drift.md
feature_home: crates/infrastructure/src/extraction
depends_on: [T05]
dependency_type: none # retro-authorization only; code already merged
serves:
  - SC-V1.5-C (real extraction is reliable)
  - SC-V1.5-F (no production stub paths remain)
files:
  - crates/infrastructure/src/extraction/claude_code.rs
  - crates/session-extractor/src/providers/claude_code.rs
  - crates/infrastructure/src/extraction/mod.rs
  - crates/session-extractor/src/lib.rs
implementing_commits:
  - bcfa9de
  - d8e45f3
  - 295bfef
---

# Retro-authorization — Claude Code CLI extraction provider (host-only)

## Purpose

This ticket provides the governance paper trail for the `claude-code` CLI subprocess extraction
provider introduced in commits `bcfa9de`, `d8e45f3`, and `295bfef` after T05 completed. Those
commits added a fully functional third extraction provider but did so without a ticket, plan
amendment, or ADR — creating a scope-drift violation against plan RATIFIED Decision 3 and the T05
scope fence. This retro-authorization resolves that gap.

The implementation itself is structurally sound (see todo #115 for the code-quality assessment).
This ticket exists solely to establish the governance trail.

## Background

Plan RATIFIED Decision 3 rejected a `claude -p` CLI subprocess extraction provider because it
"needs Node + the CLI + credential mounting inside the container — more moving parts, not fewer."
That reasoning was correct for the `docker compose` container context.

The post-ticket commits implemented the CLI provider for a different deployment context: the
developer's host machine, where the Claude Code CLI and `~/.claude` session are already present.
The container objection does not apply on the host. ADR-0002 records the full rationale for why
Decision 3's rejection is overridden for the host-only path.

## Decision

**Owner decision (2026-06-02): keep the provider and create the governance trail.**

Reference: `todos/115-pending-p1-undocumented-claude-code-cli-provider-scope-drift.md` — Option 1
(keep + document), selected by repository owner (rabak).

## Scope

Docs-only retro-authorization. No code changes are made by this ticket; the implementing code
already exists. The routing realignment (`=claude` → Anthropic API, `=claude-code` → CLI) is owned
by todos #116 and #119.

## Acceptance Criteria

- [x] ADR-0002 created: `docs/architecture/adr-0002-claude-code-cli-extraction-provider-v1-5.md`
- [x] Constitution amended to v2.1.0 with third provider listed and amendment-log entry added
- [x] This retro-ticket created and linked from index.md
- [x] T05 scope-fence-superseded note appended to `05-reliable-extraction-worker-pool-provider.md`
- [x] index.md overlap note added: `claude_code.rs` recorded in T05's file set for T07–T10 safety analysis

## Affected Files (implementing commits only — no code changes here)

- `crates/infrastructure/src/extraction/claude_code.rs` — new; 687-line CLI subprocess adapter
- `crates/session-extractor/src/providers/claude_code.rs` — new; 45-line provider registration
- `crates/infrastructure/src/extraction/mod.rs` — modified; `ExtractionProvider::ClaudeCode` variant
- `crates/session-extractor/src/lib.rs` — modified; dispatch routing updated

## Governance Trail

| Artifact | Location | Purpose |
|---|---|---|
| ADR-0002 | `docs/architecture/adr-0002-claude-code-cli-extraction-provider-v1-5.md` | Formal decision record — overrides Decision 3 for host-only context |
| Constitution v2.1.0 | `docs/constitution.md` (amendment log, 2026-06-02) | Sanctions the CLI as a third extraction provider |
| This ticket | `docs/tickets/2026-05-31-skill-layer-v1-5/T05-addendum-claude-code-cli-provider.md` | Retro-authorization in the ticket system |
| T05 scope-fence note | `docs/tickets/2026-05-31-skill-layer-v1-5/05-reliable-extraction-worker-pool-provider.md` | Records that the "No CLI subprocess" fence is superseded for the host path |
| Governance finding | `todos/115-pending-p1-undocumented-claude-code-cli-provider-scope-drift.md` | Original finding + owner decision |

## Provider Routing (post-realignment)

After the routing realignment (todos #116, #119) the mapping is:

```
EXTRACT_SESSION_PROVIDER=ollama        → Ollama (default, local)
EXTRACT_SESSION_PROVIDER=claude        → Anthropic Messages API (key-checked, loud-fail without ANTHROPIC_API_KEY)
EXTRACT_SESSION_PROVIDER=claude-code   → Claude Code CLI subprocess (host-only, credential-free)
EXTRACT_SESSION_PROVIDER=claude-cli    → alias for claude-code
```

## Work Log

### 2026-06-02 — Retro-authorization (todo #115)
**By:** Claude Code (todo #115 docs unit)
**Actions:** Created ADR-0002, amended constitution to v2.1.0, created this retro-ticket, appended
scope-fence-superseded note to T05, updated index.md overlap note.
**Owner decision:** keep the provider + create governance trail (Option 1).
