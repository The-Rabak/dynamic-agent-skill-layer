# Vertical Slice Architecture Contract

Use this reference alongside `references/execution-shape.md` when the work is organized as `vertical-slices`.

`execution-shape.md` answers **how to decompose work**. This contract answers **where the feature lives**, **what stays local**, **what stays shared**, and **how to split context after planning** so downstream agents can work with smaller packets.

## Core rule

Every real feature should have a single named **feature home**: one namespace, directory, or module boundary that acts as the default home for that feature's business behavior.

Inside that feature home, keep together the code needed to reason about the behavior end to end:
- feature-local backend logic
- feature-local frontend/UI logic
- feature-local tests
- feature-local mappers, policies, presenters, or helper code that only serves that feature

If the work spans multiple existing feature homes, say so explicitly instead of pretending one home owns everything.

## Shared / global rule

Vertical slices do **not** suspend DRY or SOLID.

Keep code in shared or global namespaces when it is genuinely cross-feature infrastructure or a stable reusable building block, such as:
- primitives and shared utilities
- reusable base classes and exceptions
- design-system components
- framework or vendor integration adapters
- cross-feature contracts and translation layers

Do **not** move core business behavior into shared directories just because more than one layer touches it.

## Extraction rule

Extract behavior out of a feature home only when at least one of these is true:
1. Multiple feature homes already rely on the same stable behavior.
2. The code is an adapter or contract boundary whose reason to change is cross-feature, not feature-specific.
3. The architecture artifact's deletion test shows the local version is now the wrong boundary.

If none of those are true, keep the code in the feature home even if that means a little local duplication for now.

## Drift checks

Treat these as architectural drift unless explicitly justified:
- Core feature behavior scattered across horizontal directories with no clear feature home.
- Copy-pasted shared abstractions across multiple feature homes.
- Shared/global directories that mainly contain one feature's business rules.
- Tickets or execution slices that cannot name the feature home they are changing.

## Context tiers

After planning, split context into these tiers:

| Tier | What belongs here | Why |
|---|---|---|
| Global context | constitution rules, local config defaults, repo-wide architectural guardrails, stable shared/global boundaries | every downstream phase needs these rules |
| On-demand context | architecture artifact, this vertical-slice contract, deep design references, related docs | useful for deeper dives without bloating every ticket |
| Ticket-local context | feature home, exact files, scope fence, success criteria, evidence command, immediate blockers, concise WHY linkage | the smallest packet an execution agent should need by default |

## Workflow obligations

- **`/workflows-brainstorm`** should identify the likely feature home, neighboring boundaries, and any obvious shared/global constraints.
- **`/workflows-plan`** should keep execution packets honest and, for `vertical-slices`, require every slice to name its feature home.
- **`/workflows-architecture`** should confirm feature homes, shared/global decisions, context tiers, and drift checks in a durable artifact.
- **`/deepen-plan`**, **`/workflows-work`**, and **`/workflows-review`** must preserve the same feature-home and shared/global boundary decisions instead of re-inventing them later.

## Packet add-on for `vertical-slices`

When the selected execution shape is `vertical-slices`, every execution packet must name:
- `Feature home`
- any shared/global dependency it relies on when that dependency matters to scope or sequencing

The packet stays execution-sized. Deeper architectural reasoning belongs in the architecture artifact and other on-demand context, not inside every slice body.
