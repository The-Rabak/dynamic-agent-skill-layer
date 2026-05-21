---
artifact: project-constitution
status: active
version: 1.0.0
ratified: 2026-05-21
last_amended: 2026-05-21
owners:
  - rabak
review_cycle: monthly
applies_to:
  - ideate
  - brainstorm
  - plan
  - work
  - review
handoff:
  purpose: true
  principles: true
  phase_guardrails: true
  agent_rules: true
  amendment_process: true
---

# Project Constitution

## Purpose

This repository builds **Dynamic Agent Skill Layer** — a local-first, self-growing skill context layer for agentic coding harnesses (Claude Code V1, multi-harness V2). It provides zero-touch, automatically compiled task-relevant skill context at session start, auto-extracts new skills from session activity, and enables cross-project and cross-harness skill portability. Architecture follows the SkillRAE paper (arXiv:2605.10114): multi-level skill graph with offline construction and online retrieval + context compilation.

## Scope Boundaries

- **In scope:** Skill graph construction (offline), dual-scope semantic retrieval + context compilation (online), session-end skill extraction with human approval, skill deduplication/merge/retirement, MCP protocol integration with agent harnesses, Docker Compose local deployment.
- **Out of scope:** Agent memory systems, chat/transcript management, agent orchestration, team collaboration UI, cloud hosting, auth/access control, monitoring dashboards. This is infrastructure, not an agent.

## Core Principles

### 1. Local-First Execution

- All services MUST run entirely on the developer's machine via Docker Compose.
- Zero cloud dependencies in V1. Embeddings MUST use local Ollama. Vector search MUST use local Qdrant. Relational storage MUST use local PostgreSQL.
- V2 team scope MAY add remote PG + Qdrant, but local-first path MUST remain the default.

### 2. Zero-Touch Session Start

- Context injection MUST require zero manual skill selection from the developer.
- Session-start compilation MUST complete in under 500ms (template-based V1; LLM guidance deferred to V2).
- First-prompt `compile_context` MUST fire automatically via Claude Code `UserPromptSubmit` MCP tool hook.
- Cold start (no matching skills) MUST return empty context silently — no error, no forced filler.

### 3. Human Gate for Mutations

- Skill creation (session extraction) MUST produce `.pending` draft files requiring human rename-to-approve.
- Skill retirement MUST produce `.retired` file markers. Automated retirement evaluation MAY run offline, but the retirement decision MUST be human-approved.
- New tag creation MUST be proposed in `.pending` files alongside skill content. Human MUST approve tags when approving skills.
- All other filesystem mutations (import, merge, deduplication) MUST produce audit records.

### 4. Portable Scope

- Skills MUST be portable across projects (project scope → global scope → different project scope) without format conversion.
- Skills MUST be portable across harnesses (Claude Code, OpenCode, Copilot, Codex) without modification. SKILL.md format is the universal interchange.
- Scope is a retrieval boundary, not a content restriction. Same skill format applies to project-local and global-machine-wide scopes.

### 5. Filesystem-Observable State

- All graph mutations MUST be visible as filesystem changes.
- Skill state transitions: `SKILL.md` (active), `SKILL.md.pending` (proposed), `SKILL.md.retired` (retired).
- The filesystem is the UI for approval workflows. No web dashboard, no CLI admin tool required for core operations.
- Graph builder MUST watch the filesystem and react to changes via `skill.file_changed` events.

## Agent Execution Rules

- **Question-asking:** Agents MUST ask before mutating the filesystem outside of `.pending` and `.retired` file patterns. Agents MUST ask before changing infrastructure configuration.
- **Portability:** All code MUST build and run in Docker Compose. Service crates MUST NOT depend on host-specific paths or environment beyond what Docker Compose env vars provide.
- **Traceability:** Every service MUST log structured events to stdout (Docker logs). Graph mutations MUST be recorded in PostgreSQL with before/after snapshots.
- **Completion reporting:** Agents MUST report: what was implemented, what tests pass, what manual verification was done, and any open issues.
- **No stubs:** Partial implementations are NOT acceptable. Every module delivered MUST have working integration. Vertical slice from input to output for every feature.

## Phase Guardrails

### Ideation Guardrails
- Ideas MUST trace to the repository purpose: local-first skill context for agentic harnesses.
- Ideas that require cloud infrastructure or external services beyond what Docker Compose provides MUST be framed as V2+ proposals with V1 local-first fallback.

### Brainstorm Guardrails
- Brainstorm MUST read this constitution before exploring approaches.
- Proposed amendments to constitution principles MUST be recorded explicitly in the brainstorm document's "Constitution Alignment" section. Silent drift is a blocking violation.

### Planning Guardrails
- Plans MUST record `constitution_version` and any `constitution_waivers` in their frontmatter.
- Plans that require waivers MUST state: which principle is waived, why, for how long, and what alternative guard applies.
- Plans MUST NOT silently override constitution baselines.

### Execution Guardrails
- Execution agents MUST stop and request approval when the constitution requires human gate (skill creation, retirement, tag creation, schema migration, model change, event contract change, infrastructure config change).
- Every execution unit's prompt MUST include relevant constitution principles as guardrails.
- Implementation MUST follow clean code principles: vertical slice modules, clear interfaces, SOLID, DRY shared utilities. No half-baked stubs.

### Review Guardrails
- Unwaived constitution violations are BLOCKING.
- Review MUST verify: local-first compliance, filesystem observability, human gate for mutations, Docker Compose deployability, test coverage.
- Named review agents dispatched through `/workflows-review` MUST include constitution compliance in their evaluation criteria.

## Allowed Exceptions

- **Waiver format:** Plans MUST record waivers as `constitution_waivers: [{principle, reason, duration, alternative_guard}]`.
- **Approval required:** Constitution amendments, skill mutations (create/retire), tag creation, graph schema migrations, Ollama model changes, Redis event contract changes, infrastructure configuration changes.
- **Recurring waivers:** If the same principle is waived more than 3 times across plans, the constitution MUST be reviewed for amendment.

## Quality Standards

- **Tests:** Integration tests for MCP server endpoints and graph builder pipeline. Unit tests for retrieval scoring, compilation, and extraction logic. Docker Compose end-to-end tests.
- **Lint:** Clippy with strict profile. MUST pass before merge.
- **Format:** rustfmt. MUST pass before merge.
- **Benchmark:** MCP server `compile_context` latency MUST be benchmarked. Target: <500ms.
- **Code standards:** Vertical slice modules, clear interfaces, SOLID, DRY (shared utilities in global scope, not duplicated). No stubs or half-baked partial implementations. Implementation MUST be complete from the start.

## Amendment Process

- **Proposer:** Repository owner (rabak) or delegated maintainer.
- **Ratification:** Owner ratifies via merge to main branch.
- **Review cadence:** Monthly minimum. Constitution is reviewed alongside the changelog.
- **Versioning:** MAJOR (principle removed or redefined), MINOR (new principle or section added), PATCH (clarification only).
- **Amendment log:** Every amendment MUST be recorded with date, version bump, and rationale.

## Amendment Log

- v1.0.0 (2026-05-21) — Initial ratification. Derived from `docs/brainstorms/2026-05-21-compiled-context-layer-skill-rae-brainstorm.md` and subsequent grill-with-docs session.
