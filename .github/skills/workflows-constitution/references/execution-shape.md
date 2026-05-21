# Execution Shape Contract

Use this reference when planning, deepening, or executing work so the workflow can preserve judgment without re-explaining the same rules in every prompt.

For `vertical-slices`, also apply `references/vertical-slice-architecture.md` so the plan names the feature home and preserves the shared/global boundary instead of treating slices as pure task buckets.

## Default

Choose `vertical-slices` unless that would create fake end-to-end work just to satisfy the template.

## Allowed modes

### `vertical-slices` (default)

Use when a thin, testable, end-to-end behavior exists.

Required packet:
- `Slice type`
- `Serves`
- `Demo scenario`
- `Feature home`
- `Scope`
- `Scope fence`
- `Files`
- `Depends on`
- `Dependency type`
- `Success criteria`
- `Test command`

### `infra-track`

Use for enabling or foundational work where no honest user-visible tracer bullet exists yet.

Required packet:
- `Capability enabled`
- `Consumers / downstream work unlocked`
- `Scope`
- `Files`
- `Depends on`
- `Risk / rollback`
- `Validation command`
- `Success criteria`

### `fix-batch`

Use for a series of small mostly independent fixes where forcing one vertical slice would blur the real work.

Required packet:
- `Problem`
- `Repro / expected outcome`
- `Files`
- `Depends on`
- `Validation command`
- `Success criteria`

## Selection rules

1. Default to `vertical-slices`.
2. Switch to `infra-track` only when the honest near-term value is enabling capability, not a user-visible behavior.
3. Switch to `fix-batch` only when the work is truly a batch of small fixes, not a feature being split too late.
4. Never force `vertical-slices` if that would create fake verticality.
5. If the mode is not the default, record why in `execution_shape.rationale` and in `## Execution Shape`.

## Plan shape

Add this frontmatter block to every plan:

```yaml
execution_shape:
  mode: vertical-slices # vertical-slices | infra-track | fix-batch
  rationale: "" # required when mode is not vertical-slices
```

Add this body section:

```markdown
## Execution Shape
- **Mode:** [vertical-slices | infra-track | fix-batch]
- **Why:** [1-2 sentences]
```

Then use the packet section that matches the chosen mode:
- `## Execution Slices`
- `## Infrastructure Work Packets`
- `## Fix Batch Items`

When the mode is `vertical-slices`, each packet must also name the feature home defined by `references/vertical-slice-architecture.md`.

## Deepening rules

- Validate the plan against the chosen mode, not against `vertical-slices` unconditionally.
- You may recommend switching modes if the selected one is clearly wrong, but record that as a `WHY Reassessment` note instead of silently rewriting intent.

## Execution rules

- `/workflows-work` must execute the units defined by the chosen mode.
- Do not coerce `infra-track` or `fix-batch` plans into slices unless the user explicitly approves a mode change.
- Session tracking may stay generic (`unit`, `work status`) even when the selected mode is `vertical-slices`.
