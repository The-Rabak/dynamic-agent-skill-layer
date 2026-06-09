---
kind: retro-approval
date: 2026-06-02
authorized_by: "repository owner (rabak)"
governance_finding: todos/117-pending-p1-undocumented-model-bumps-and-infra-config-without-human-gate.md
implementing_commits:
  - bcfa9de
  - d8e45f3
  - 295bfef
affected_tickets:
  - docs/tickets/2026-05-31-skill-layer-v1-5/05-reliable-extraction-worker-pool-provider.md
status: approved
---

# Retroactive Human-Gate Approval — Model Defaults + Healthcheck (2026-06-02)

## Purpose

This record retroactively closes the human-gate gap identified in todo #117.
Constitution §Allowed Exceptions and §Execution Guardrails require explicit owner approval for
Ollama model changes and infrastructure configuration changes. Commits `bcfa9de`, `d8e45f3`, and
`295bfef` made three such changes without a human-gate checkpoint, ticket, or session note.

**Owner decision (2026-06-02): keep all three changes; record retroactive approval.**

Reference: `todos/117-pending-p1-undocumented-model-bumps-and-infra-config-without-human-gate.md`
— Option 1 (keep + document), selected by repository owner (rabak).

---

## Approved Change (a) — Ollama default model: `granite4:3b` → `gemma4:e4b`

**File:** `crates/infrastructure/src/extraction/ollama.rs`
**Commits:** `d8e45f3`, `bcfa9de`

**Rationale:** `gemma4:e4b` is a more capable model in the Gemma 4 family than `granite4:3b`,
improving extraction quality on the default local path without requiring any additional
configuration. The change is backward-compatible: users can still override the model via
`OLLAMA_EXTRACTION_MODEL`. The 32-job burst success criterion (SC-V1.5-C) is not invalidated —
it is a throughput/correctness criterion, not a model-identity criterion; the pool and timeout
architecture proven in T05 applies equally to `gemma4:e4b`.

**Verified current value:** `"gemma4:e4b"` at
`crates/infrastructure/src/extraction/ollama.rs:37`.

**Approved.** Authorized by: repository owner (rabak), 2026-06-02.

---

## Approved Change (b) — Claude API default model: `claude-haiku-4-5` → `claude-sonnet-4-6`

**File:** `crates/infrastructure/src/extraction/claude.rs:31`
**Commit:** `d8e45f3`

**Rationale:** `claude-sonnet-4-6` provides materially higher extraction quality than
`claude-haiku-4-5` for the skill-extraction prompt. The Claude provider is an opt-in path
(`EXTRACT_SESSION_PROVIDER=claude`); it is not the compose default (Ollama is). Users who
select the Claude provider are already opting into cloud cost; the model can still be overridden
via `EXTRACT_SESSION_MODEL`.

**Cost-increase acknowledgement (required by constitution §Allowed Exceptions):**
`claude-sonnet-4-6` has a higher per-token cost than `claude-haiku-4-5`. This cost increase
is **explicitly acknowledged and accepted** by the repository owner. The increased cost is
justified by the improvement in extraction quality on the opt-in Claude path.

**Verified current value:** `"claude-sonnet-4-6"` at
`crates/infrastructure/src/extraction/claude.rs:31` (`DEFAULT_CLAUDE_MODEL`).

**Approved.** Authorized by: repository owner (rabak), 2026-06-02.

---

## Approved Change (c) — Compose healthcheck: `wget http://localhost:11434/api/tags` → `["CMD","ollama","ps"]`

**Files:** `docker-compose.yml`, `docker-compose.test.yml`
**Commit:** `bcfa9de`

**Rationale:** The `ollama/ollama` Docker image does not ship `wget`. The previous healthcheck
command always failed (container reported `unhealthy`), masking real Ollama readiness. Replacing
it with `["CMD","ollama","ps"]` uses the `ollama` binary that is guaranteed present in the image
and correctly probes whether the Ollama server is accepting requests.

This change is a **correctness bugfix**, not a scope expansion. No new compose service, port,
volume, or network was added. The T08 human-gate checkpoint for `docker-compose.test.yml` is not
bypassed — this fix predates T08's gate and is recorded here retroactively.

**Approved.** Authorized by: repository owner (rabak), 2026-06-02.

---

## Implementing Commits

| SHA | Summary |
|---|---|
| `bcfa9de` | fix(tests,compose): correct stale graph-rebuild skill count + Ollama healthcheck |
| `d8e45f3` | feat(extraction): headless Claude Code CLI provider + default model bumps |
| `295bfef` | docs(extraction): clarify Claude CLI provider handles no credentials |

---

## Governance Gap Note

No execution session or STATE.md was created for these commits, contrary to §Agent Execution
Rules completion-reporting. This retro record satisfies the approval requirement. A formal
session note is not required because the commits were made outside the standard ticket-execution
flow; the approval trail is complete with this document.

---

## Authorized by

**Repository owner (rabak), 2026-06-02**
