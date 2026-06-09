---
adr: "0002"
date: 2026-06-02
status: accepted
deciders:
  - repository-owner (rabak)
supersedes:
  - plan Decision 3 (2026-05-31) — "not a `claude -p` CLI subprocess"
---

# ADR-0002: Claude Code CLI Extraction Provider — Host-Only Third Provider (V1.5)

## Context

Plan RATIFIED Decision 3 (`docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md`, line 202)
explicitly rejected a `claude -p` CLI subprocess as an extraction provider for the following reason:

> "not a `claude -p` CLI subprocess (needs Node + the CLI + credential mounting **inside the
> container** — more moving parts, not fewer)"

That reasoning was correct for the `docker compose` deployment target, where the container has no
Node runtime, no Claude Code CLI binary, and no host credential store mounted in.

Three post-ticket commits (`bcfa9de`, `d8e45f3`, `295bfef`) introduced a new extraction provider —
`crates/infrastructure/src/extraction/claude_code.rs` — that spawns a `claude -p --output-format
json` subprocess. This was undocumented relative to Decision 3 and the T05 scope fence ("No CLI
subprocess, no sidecar"). The implementation is structurally sound, but the decision had no paper
trail. Todo #115 records the governance finding.

## Decision

**The `claude-code` CLI subprocess provider is adopted as a sanctioned, host-only third extraction
provider for V1.5.**

The key insight that overrides Decision 3's objection is that the Decision 3 reasoning applied
exclusively to the container context. The `claude-code` provider is NOT intended to run inside the
stock `docker compose` container; it runs directly on the developer's host machine. On the host:

- Node and the Claude Code CLI are already present (the developer uses Claude Code).
- The host `~/.claude` session provides credentials — no API key is required and no credential
  mounting is needed.
- The "more moving parts inside the container" objection does not apply — there are no additional
  container moving parts at all.

This gives the system three sanctioned extraction providers:

| Provider | Env value | Deployment context | Credentials |
|---|---|---|---|
| Ollama | `ollama` (default) | `docker compose` or host | None — fully local |
| Claude / Anthropic API | `claude` | `docker compose` or host | `ANTHROPIC_API_KEY` (loud-fail at construction if absent) |
| Claude Code CLI | `claude-code` (alias: `claude-cli`) | Host only | Host `~/.claude` session — credential-free from the user's perspective |

The routing assignment (implemented by todo #116 / #119) is:
- `EXTRACT_SESSION_PROVIDER=claude` → Anthropic Messages API (key-checked, loud-fail without key)
- `EXTRACT_SESSION_PROVIDER=claude-code` (or `=claude-cli`) → CLI subprocess provider (host-only)

The CLI provider must NOT be used in the compose container. Its compose service entry (if present)
must carry an explicit comment making the host-only constraint visible.

## Rationale for Overriding Decision 3

Decision 3's "more moving parts" objection was a container-context argument, not a universal one.
The full reasoning chain was:

1. The compose container lacks Node + CLI → would require installing them → more container moving
   parts → rejected.
2. Credential mounting inside the container adds operational surface → rejected.

Neither point applies when the provider runs on the host. The host developer already has Claude
Code running — the CLI binary and credentials are a given, not additional overhead. Running
`claude -p` on the host is a zero-new-dependency operation from the user's perspective.

The host/subscription path is a distinct deployment mode from the compose container path. Decision 3
did not contemplate this mode because the V1.5 plan was container-first. The post-ticket commits
recognized this gap and filled it. The decision is correct for the use case it targets.

Constitution Principle 1's local-first guarantee and loud-fail clause remain intact: the CLI
provider does not reach the Anthropic cloud; it uses the local Claude Code session. The provider
selection is still explicit and opt-in.

## Consequences

### Accepted

- **Host-only constraint.** The `claude-code` provider works only where the Claude Code CLI is
  installed and authenticated. Selecting it in an environment without the CLI binary defers to a
  `ProviderUnavailable` error at use-time (construction succeeds; binary absence is checked on first
  use). This is by design — the binary check is a runtime gate, not a startup penalty.
- **No compose default.** The compose `EXTRACT_SESSION_PROVIDER` value remains `ollama`. The
  `claude-code` provider is not usable inside the stock container and must not be set as the default
  in `docker-compose.yml`.
- **Three-provider surface.** The extraction subsystem now has three provider branches instead of
  two. The `TranscriptSkillExtractionService` seam keeps all three behind a single trait, so callers
  are unaffected.

### Positive

- Subscription users (developers with an active Claude Code / claude.ai subscription) get a
  high-quality extraction provider with zero API key management.
- The existing `~/.claude` authentication is reused — no new credential surface.
- The `--disallowed-tools` and `--system-prompt` JSON enforcer flags in the subprocess invocation
  constrain the CLI's behavior at the call site.
- 20 unit tests including fake-CLI subprocess paths cover the adapter.

### Not Changed

- Constitution Principle 1 loud-fail clause: selecting `=claude` without `ANTHROPIC_API_KEY` still
  fails loudly at construction. This clause targets the Anthropic API provider (`=claude`) and is
  unchanged. The CLI provider (`=claude-code`) has no API key requirement.
- Ollama remains the default (`EXTRACT_SESSION_PROVIDER` unset or `=ollama`).
- The compose data plane (Qdrant, PostgreSQL, Ollama embeddings) remains local-only.

## Supersedes

This ADR supersedes plan Decision 3 specifically on the question of whether a `claude -p` CLI
subprocess is an acceptable extraction provider. Decision 3's rejection held for the container
deployment context. This ADR sanctions the CLI provider for the host-only deployment context.

The plan's Decision 3 text is retained as historical record; this ADR is the authoritative
governance document for the CLI provider decision. See also: constitution v2.1.0 amendment log
(2026-06-02) and retro-ticket
`docs/tickets/2026-05-31-skill-layer-v1-5/T05-addendum-claude-code-cli-provider.md`.

## References

- Plan Decision 3 (overridden for host context): `docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md` line 202
- T05 scope fence (superseded for host path): `docs/tickets/2026-05-31-skill-layer-v1-5/05-reliable-extraction-worker-pool-provider.md`
- Governance finding: `todos/115-pending-p1-undocumented-claude-code-cli-provider-scope-drift.md`
- Routing realignment: todos #116, #119
- Constitution: `docs/constitution.md` (v2.1.0, 2026-06-02)
- Retro-ticket: `docs/tickets/2026-05-31-skill-layer-v1-5/T05-addendum-claude-code-cli-provider.md`
- Implementing commits: `bcfa9de`, `d8e45f3`, `295bfef`
- New provider file: `crates/infrastructure/src/extraction/claude_code.rs`
- Provider registration: `crates/session-extractor/src/providers/claude_code.rs`
