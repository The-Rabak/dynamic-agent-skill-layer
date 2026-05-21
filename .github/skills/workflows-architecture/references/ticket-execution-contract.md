# Ticket Execution Contract

Use this reference when writing or consuming local ticket artifacts.

This contract is shared by `/workflows-to-issues`, the `focused-ticket-priming` skill, `/workflows-work`, and `/workflows-review` so ticket generation and ticket execution stay aligned.

## Ticket set layout

Write ticket sets to:

```text
docs/tickets/YYYY-MM-DD-<topic>/
```

Required files:

- `index.md`
- `NN-<ticket-slug>.md` for each execution-ready ticket

## Ticket set index

`index.md` must include:

- plan path
- architecture artifact path or explicit handoff note
- execution shape
- ticket order
- dependency view
- blocker summary
- review summary split into `Blocking gaps` and `Recommendations`

## Ticket file naming

- Keep zero-padded numeric prefixes in execution order: `01-...`, `02-...`, `03-...`
- Use the slug to describe the outcome, not the layer or implementation detail

## Required ticket frontmatter

Every ticket file must include this frontmatter shape:

```yaml
---
ticket_id: T01
title: Tracer bullet for account onboarding
kind: tracer-bullet # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: ready # ready | in_progress | blocked | completed
plan_ref: docs/plans/2026-05-20-account-onboarding-plan.md
tickets_ref: docs/tickets/2026-05-20-account-onboarding/index.md
architecture_ref: docs/architecture/2026-05-20-account-onboarding.md
source_packet_ref: "## Execution Slices > Slice 1"
feature_home: src/features/account-onboarding
depends_on: []
dependency_type: none # none | hard | soft | parallel-safe
serves:
  - Success criterion 1
files:
  - src/features/account-onboarding/controller.ts
test_command: bun test tests/account-onboarding.test.ts
tdd_mode: inherit
---
```

## Required ticket body

Every ticket must include these sections in this order:

1. `# <Ticket title>`
2. `## Serves`
3. `## Scope`
4. `## Scope Fence`
5. `## Acceptance Criteria`
6. `## Shared / Global Notes`
7. `## Local Context`
8. `## Parent Refs`
9. `## Deeper-Dive Refs`
10. `## Coupling Notes`

## Local context rules

The `## Local Context` section is the execution packet.

It must include:

- the smallest WHY summary needed for this ticket
- the concrete files and interfaces that matter now
- architectural boundary notes that keep the feature home honest
- explicit unknowns or open questions that an execution agent must surface instead of guessing

It must NOT:

- copy the whole plan
- restate the whole architecture artifact
- duplicate repo-global guardrails that already belong in global context

## Status lifecycle

Use these ticket statuses:

- `ready` -- ticket is execution-ready
- `in_progress` -- active execution session owns it
- `blocked` -- a dependency or blocker prevents safe execution
- `completed` -- execution finished and required evidence exists

## Execution consumption rules

When `/workflows-work` receives a ticket file:

- treat that ticket as one pre-scoped execution unit
- load `plan_ref` and `architecture_ref` for WHY and architecture context
- honor the ticket's `feature_home`, `files`, and `## Scope Fence`
- stop and send the user back to `/workflows-to-issues` if required frontmatter or body sections are missing

## Review consumption rules

When `/workflows-review` or `ticket-flow-auditor` consumes ticket artifacts, verify:

- the ticket still matches the parent plan and architecture
- the implementation stayed inside the ticket scope fence unless the change was explicitly documented
- dependency order and status changes stayed honest
- evidence matches the ticket's stated acceptance criteria and test command
