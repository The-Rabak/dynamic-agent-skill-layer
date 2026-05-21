---
{}
---

# Architecture Improvement Artifact Contract

This template is consumed by the `workflows:architecture` phase. It is a **reference document**, not an invocable command.

The purpose of the artifact is to make architectural choices explicit before `/deepen-plan` and `/workflows-work` continue. Use the vocabulary below consistently so downstream agents inherit a shared contract instead of guessing from oral tradition.

## Mandatory inputs

A valid architecture improvement pass requires:

- `plan_ref` -- the plan being hardened
- Problem Narrative
- User Story
- Success Criteria
- Architectural Context
- Current plan phases/tasks
- The vertical-slice module contract from `vertical-slice-architecture.md`
- `brainstorm_ref`, constitution context, and source docs when available

If any of the first five are missing, stop and report the missing input instead of improvising.

## Mandatory output location

Write the artifact to:

```text
docs/architecture/YYYY-MM-DD-<topic>-architecture.md
```

Then record that path back into the plan as `architecture_ref: <artifact path>` or under a `## Related Artifacts` section.

## Required frontmatter

```yaml
---
date: YYYY-MM-DD
topic: <kebab-case-topic>
status: complete
plan_ref: docs/plans/...
brainstorm_ref: docs/brainstorms/... # optional
handoff:
  deepen_plan: true
  work: true
  review: true
---
```

## Required sections

```markdown
# <Topic Title> Architecture Improvement

## Purpose Linkage
- Problem Narrative: <copied or summarized from plan>
- User Story: <copied or summarized from plan>
- Success Criteria: <list the criteria this artifact protects>
- Architectural Context: <where this lives and what it touches>

## Feature Homes and Ownership
- **Feature home:** `<path/or/namespace>`
  - Owns: <business behavior that should stay local to this feature>
  - Crosses into: <neighboring boundaries touched by this work>
  - Notes: <why this is the right home>

## Shared / Global Decisions
| Candidate | Keep in feature home / Move to shared | Why |
|-----------|----------------------------------------|-----|
| <thing>   | <decision>                             | <reason grounded in DRY, SOLID, and reason-to-change> |

## Deepening Candidates
- <candidate>: Why this area needs deeper architectural treatment before execution hardening

## Deletion Test
| Candidate | Keep/Delete/Delay | Why |
|-----------|-------------------|-----|
| <thing>   | <decision>        | <reason grounded in user story, scope, and complexity> |

## Interfaces as Test Surfaces
- **Interface:** <named behavior boundary>
  - Callers/tests rely on: <stable behavior>
  - Must not leak: <implementation details>
  - Evidence needed later: <unit/e2e/test surface>

## Seams, Adapters, and Contracts
- **Seam:** <where behavior can change or be substituted>
  - **Adapter:** <translation layer at that seam, or `None`>
  - **Contract:** <explicit promise that must stay stable>

## Design-It-Twice Options
- **Option A:** <simpler structural option>
- **Option B:** <alternative if leverage or risk justifies it>
- **Chosen for now:** <decision and why>

## Context Tiers
- **Global context:** <repo-wide rules, defaults, and stable shared/global boundaries>
- **On-demand context:** <artifact refs and deeper docs later phases can load as needed>
- **Ticket-local context:** <the minimum feature-home, scope, and evidence packet each execution ticket should carry>

## Recommendations for `/deepen-plan`
- <how to harden tasks, dependencies, tests, or research prompts>

## Recommendations for `/workflows-work`
- <what implementation must preserve>

## Recommendations for `/workflows-review`
- <what reviewers should verify>

## Drift Checks
- <what would count as feature-home drift, bad shared extraction, or horizontal scattering>

## Open Questions
- <unresolved architectural question, or `None`>
```

## Language rules

Use these terms exactly and consistently:

- **Deepening candidates** -- structural areas that need more treatment before execution hardening
- **Feature home** -- the primary namespace or directory where one feature's business behavior lives
- **Shared / global** -- code whose reason to change is truly cross-feature or infrastructural
- **Deletion test** -- the test that asks what can be removed, avoided, or delayed before adding abstraction
- **Interface as test surface** -- the stable behavior callers and tests should target
- **Seam** -- a boundary where implementation can vary
- **Adapter** -- the translation layer at a seam, usually for external systems or incompatible models
- **Contract** -- the explicit promise a seam or interface must honor
- **Design-it-twice** -- a lightweight comparison of two structural options when a boundary is high leverage
- **Context tiers** -- the split between global, on-demand, and ticket-local context after planning

Avoid fuzzy substitutes like "clean it up later," "probably abstract here," or "future-proofing" unless you immediately restate them in deletion-test, interface, seam, or adapter terms.

## Completion standard

The artifact is complete only when:

- Every proposed abstraction survives the deletion test or is explicitly deferred
- Feature homes and shared/global decisions are named explicitly
- Interfaces are described as test surfaces, not just nouns
- Seams and adapters are mapped to real boundaries in the plan
- Context tiers are explicit enough that ticket creation and execution can stay smaller than the full plan
- `/deepen-plan`, `/workflows-work`, and `/workflows-review` each have explicit handoff guidance
- The artifact path is recorded back into the plan
