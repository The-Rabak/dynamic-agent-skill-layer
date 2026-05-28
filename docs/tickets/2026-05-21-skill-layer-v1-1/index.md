# Skill Layer V1.1 Ticket Set

- **Plan:** `docs/plans/2026-05-21-feat-skill-layer-v1-1-plan.md`
- **Architecture artifact:** `docs/architecture/2026-05-21-skill-layer-v1-1-architecture.md`
- **Execution shape:** `vertical-slices`
- **Ticket set path:** `docs/tickets/2026-05-21-skill-layer-v1-1/`

## Contract Inputs

| Asset | File used | First non-empty line |
|---|---|---|
| Ticketization contract | `.github/skills/workflows-to-issues/references/ticketization-contract.md` | `# Ticketization Workflow Contract` |
| Ticket execution contract | `.github/skills/workflows-to-issues/references/ticket-execution-contract.md` | `# Ticket Execution Contract` |
| Execution shape contract | `.github/skills/workflows-to-issues/references/execution-shape.md` | `# Execution Shape Contract` |
| Vertical-slice architecture contract | `.github/skills/workflows-to-issues/references/vertical-slice-architecture.md` | `# Vertical Slice Architecture Contract` |
| Orchestration protocol | `.github/skills/workflows-to-issues/references/orchestration-protocol.md` | `---` |
| Focused ticket priming skill | `.github/skills/focused-ticket-priming/SKILL.md` | `---` |

## Architecture Handoff

- Keep the feature homes frozen to `domain`, `infrastructure`, `mcp-server`, `retrieval`, `compiler`, `graph-builder`, `maintenance`, `admin`, and `session-extractor`.
- Preserve the canonical contracts already frozen by the plan and architecture artifact: result semantics, transcript ingress, event catalog, lifecycle state machine, graph invalidation ordering, and scalar `scope` plus `merged_from_scopes`.
- Keep global guardrails global: constitution rules, local-first Docker Compose deployment, human-gated filesystem mutations, stable domain vocabulary, PG schema contracts, and Redis event envelope rules.
- Keep deeper architecture on demand: the architecture artifact, vertical-slice contract, and TDD/evidence contract stay out of ticket bodies unless a ticket needs a direct citation.

## Ticket Order

| Order | Ticket | Kind | Outcome | Depends on |
|---|---|---|---|---|
| 01 | `01-compose-and-domain-foundation.md` | tracer-bullet | Local Docker topology and pure domain contracts exist | none |
| 02 | `02-infrastructure-adapters-and-schema.md` | tracer-bullet | Shared adapters, PG schema, outbox, and shared infra utilities exist | T01 |
| 03 | `03-single-scope-compile-context.md` | tracer-bullet | `compile_context` works end to end against seeded skills | T02 |
| 04 | `04-dual-scope-retrieval-and-hooking.md` | expansion | Project and global scopes fuse correctly and Claude hook config is documented | T03 |
| 05 | `05-watcher-driven-graph-rebuild.md` | expansion | Filesystem changes rebuild the graph and publish invalidation events | T04 |
| 06 | `06-session-end-extraction-and-approval.md` | expansion | `extract_session` produces human-gated `.pending` skills | T05 |
| 07 | `07-outbox-relay-and-reconciliation.md` | hardening | PG to Qdrant consistency is replayable and audited | T05 |
| 08 | `08-maintenance-merge-retire-and-cron.md` | expansion | Merge, retire, and scheduled maintenance proposals exist without auto-approval | T05, T07 |
| 09 | `09-admin-inspection-and-rebuild-tools.md` | expansion | Read-only inspection and manual rebuild tools exist | T05 |
| 10 | `10-pending-lifecycle-state-machine.md` | hardening | `.pending` lifecycle metadata, warnings, and tombstones are consistent | T06, T08 |
| 11 | `11-graceful-degrade-and-health-checks.md` | hardening | Resilience and health semantics are explicit across services | T08 |
| 12 | `12-session-persistence-and-context-cache.md` | hardening | Session suppression and compiled-context caching survive restart and invalidation | T11 |
| 13 | `13-logging-benchmarks-and-docs.md` | hardening | Structured logs, latency evidence, and operator docs are complete | T11 |
| 14 | `14-live-data-plane-e2e-and-stress-suite.md` | hardening | Full live data-plane flow is validated under realistic dependency and load conditions | T07, T11, T13 |
| 15 | `15-extraction-prompt-review-and-unification.md` | hardening | Extraction prompt strategy is reviewed, unified where possible, and provider-specific divergence is explicitly justified | T06 |

## Dependency View

| Ticket | Hard blockers | Notes |
|---|---|---|
| T01 | none | Tracer-bullet foundation |
| T02 | T01 | Shared infra can only bind to frozen domain types and config |
| T03 | T02 | First user-visible flow depends on real adapters and schema |
| T04 | T03 | Dual-scope builds on working single-scope semantics |
| T05 | T04 | Watcher-driven rebuild must invalidate the same live retrieval path |
| T06 | T05 | Approval flow needs watcher/rebuild to activate approved skills |
| T07 | T05 | Outbox relay hardens the graph write path introduced in T05 |
| T08 | T05, T07 | Maintenance proposals rely on graph data and durable vector sync |
| T09 | T05 | Admin tools read and trigger the graph-builder path; they stay out of maintenance policy |
| T10 | T06, T08 | Lifecycle metadata must cover both extraction and merge proposal flows |
| T11 | T08 | Resilience pass comes after the main offline policy loop exists |
| T12 | T11 | Cache and suppression semantics rely on final degraded/healthy rules |
| T13 | T11 | Logging and docs should describe the hardened runtime, not a moving target |
| T14 | T07, T11, T13 | Final hardening gate validates live data-plane correctness, degraded recovery, and bounded stress behavior end to end |
| T15 | T06 | Prompt strategy hardening depends on the established extraction contract and must preserve provider-parity outputs |

## Blocker Summary

- No missing plan inputs blocked ticket generation: WHY artifacts, `execution_shape`, architecture handoff, and TDD contract were all explicit.
- The only execution blockers are the ticket dependencies above; no extra scope or architecture clarifications were required during ticketization.

## Review Summary

### Blocking gaps

- None. `ticket-flow-auditor` final sweep found the ticket set execution-safe against the plan, architecture, and ticket contracts.

### Post-Execution Artifacts

- **T11 realignment (2026-05-28):** Implementation notes appended to `11-graceful-degrade-and-health-checks.md` to reconcile ticket env-var semantics with actual compose/code defaults. See the `## Implementation Notes` section in the ticket file for detailed diffs.

### Recommendations

- Clarify the canonical-scope policy for approved merge proposals in T08 before execution starts. The ticket already surfaces this as an open question, but the policy itself is still intentionally unresolved.
