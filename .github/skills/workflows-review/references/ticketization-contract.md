# Ticketization Workflow Contract

Use this reference when turning a plan into smaller local ticket artifacts.

This contract exists to keep `/workflows-plan` and `/deepen-plan` strategic while moving execution-sized context into independently usable tickets.

## Mandatory ticket-packaging assets

Ticketization must use these shared assets instead of inventing ad hoc ticket shapes:

- `focused-ticket-priming` -- the packaging skill that turns one execution packet into one compact ticket packet
- `ticket-execution-contract.md` -- the exact frontmatter/body schema that `/workflows-work` and `/workflows-review` will consume later

## Canonical command

- **Command name:** `/workflows-to-issues`
- **Placement:** after `/workflows-plan` or `/deepen-plan`
- **Recommended timing:** after `/deepen-plan` when the plan still needs architectural or research hardening

## Inputs

A ticketization run requires:

- `plan_ref` -- the source plan
- `execution_shape` -- must already be explicit in the plan
- plan WHY artifacts: Problem Narrative, User Story, Success Criteria, Architectural Context
- the architecture artifact at `architecture_ref`, or an explicit architecture handoff contract

If the plan is missing the WHY artifacts, execution shape, or architecture guidance, stop and ask the user to repair that first instead of guessing.

## Output location

Write the local ticket set to:

```text
docs/tickets/YYYY-MM-DD-<topic>/
```

Required outputs:

- `index.md` -- ticket-set summary, dependency view, and run guidance
- `NN-<ticket-slug>.md` -- one file per ticket in execution order

Record the ticket set back into the plan as:

- `tickets_ref: docs/tickets/YYYY-MM-DD-<topic>/index.md`

If frontmatter cannot be updated safely, add the path under `## Related Artifacts`.

## v1 rollout

The first version is **local-artifact first**.

- Generate local ticket files only.
- Do not publish tracker issues automatically.
- Keep issue-tracker publishing as a later optional extension.

## Ticket sizing rules

- Start from the plan's declared execution packets instead of inventing a new backlog from scratch.
- Size tickets by **coupling and boundary clarity**, not raw line count.
- A ticket may cross backend, frontend, and tests when that is the thinnest honest slice.
- Split a packet when one ticket would deliver more than one meaningful outcome, blur ownership, or bury the feature home.
- Keep the first ticket a tracer bullet when the selected execution shape is `vertical-slices`.

## Required ticket-local context

Every generated ticket must carry a compact execution packet that can stand on its own.

Required fields:

- `Ticket`
- `Kind` -- tracer-bullet | expansion | hardening | infra-track | fix-batch
- `Serves`
- `Feature home`
- `Scope`
- `Scope fence`
- `Files`
- `Depends on`
- `Dependency type`
- `Acceptance criteria`
- `Evidence / test command`
- `Shared / global notes`
- `Parent refs` -- plan path, architecture path, and source slice/packet reference
- `Deeper-dive refs` -- optional docs to read only when the ticket-local packet is insufficient
- `Coupling notes` -- why this ticket is one unit instead of multiple tickets

Use `ticket-execution-contract.md` as the exact source of truth for the concrete frontmatter and section order.

## Context packaging rules

Split context across the existing three tiers:

- **Global context** -- constitution rules, local defaults, repo-wide guardrails
- **On-demand context** -- architecture artifact, vertical-slice architecture contract, supporting docs
- **Ticket-local context** -- only the minimum packet needed to execute one ticket safely

Ticketization should shrink execution context, not recreate the full plan inside every ticket.

## Behavior after `plan` vs after `deepen-plan`

- After **`/workflows-plan`**: allow earlier backlog shaping, but preserve visible uncertainty instead of pretending the research is settled
- After **`/deepen-plan`**: prefer this mode when the plan already has hardened boundaries, tests, and architecture guidance

In both cases, never strip WHY or architecture references out of the tickets.

## Final ticket-set review

Ticketization ends with a final review sweep over the full ticket set.

That sweep must use the dedicated `ticket-flow-auditor` reviewer so the same contract can be enforced again later during `/workflows-review`.

That review must check:

- vertical-slice honesty
- feature-home clarity
- shared/global boundary honesty
- blocker and dependency correctness
- context completeness
- ticket size and coupling quality

The review output must separate:

- **Blocking gaps** -- ticket set is not safe to execute yet
- **Recommendations** -- the set is usable, but could be improved

## Completion standard

The ticketization contract is satisfied only when:

- the command name and placement are explicit
- the output path and artifact shape are explicit
- the priming skill and ticket schema contract are explicit
- local-artifact-first behavior is explicit
- `tickets_ref` recording is explicit
- each ticket's required local context is explicit
- the final ticket-set review step and reviewer are explicit
