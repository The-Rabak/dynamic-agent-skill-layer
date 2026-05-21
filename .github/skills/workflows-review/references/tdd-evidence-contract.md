---
{}
---

# Shared TDD and Evidence Contract

This reference is the single source of truth for TDD contract resolution, plan output shape, Ralph evidence semantics, and review-gate classifications across the workflow chain.

## Contract Resolution

- Plan-level `tdd` values override `compound-engineering.local.md`.
- Any field set to `inherit` falls back to the local config.
- If no local config exists, default to Ralph-driven `red-green-refactor` with unit + e2e evidence required.
- Any relaxation from Ralph or the unit + e2e default must be explicit, justified in `tdd.exceptions`, and paired with `replacement_evidence`.
- Do not silently weaken the TDD contract.

## Plan Section Shape

When a plan emits `## TDD & Evidence Contract`, keep this exact bullet shape and fill it with the resolved values:

- **Precedence:** [how plan values override/fall back]
- **Effective mode:** [Ralph-driven TDD | Standard implementation]
- **Effective loop:** [Failing tests first -> minimal implementation -> refactor -> post-refactor rerun | Implementation-first]
- **Required evidence:** [Unit command/result], [E2E command/result]
- **Exceptions:** [None, or explicit justified deviation plus `replacement_evidence`]

## Ralph Evidence Block

When Ralph-driven execution is active, keep a stable `### TDD Evidence` / `## TDD Evidence` block with `Red`, `Green`, and `Post-Refactor Green` entries.

- `Red` and `Green` prove behavior coverage.
- `Red` must fail for the missing behavior, not setup, import, syntax, or environment noise.
- `Green` must show the requested behavior now passes.
- `Post-Refactor Green` proves cleanup safety; even if no cleanup was needed, rerun and say so.
- Replacement evidence is valid only when an approved exception explicitly allows it.

## Review Gate Classifications

When auditing evidence, classify findings exactly this way:

- **Missing behavior coverage** — weak or missing `Red`/`Green`, or evidence that does not prove the requested behavior.
- **Missing cleanup after refactor** — weak or missing `Post-Refactor Green`, or no rerun evidence after cleanup/refactor was claimed.

Keep the gate output terse and evidence-based: cite the task/session file, the weak or missing block, and the concrete reason.
