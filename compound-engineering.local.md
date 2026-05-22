---
review_agents: [constitution-guardian, code-simplicity-reviewer, security-sentinel, performance-oracle, architecture-strategist]
plan_review_agents: [constitution-guardian, code-simplicity-reviewer]
tdd_enabled: true
tdd:
  precedence: plan_overrides_local
  mode: ralph
  loop: red-green-refactor
  evidence:
    unit: required
    e2e: required
  exceptions: []
review_mode: bulk
---

# Review Context

Add project-specific review instructions here.
These notes are passed to `/workflows-review` and to the template-based review steps inside `/workflows-work`. They do not authorize direct named review-agent dispatch outside `/workflows-review`.

## TDD Defaults

- `tdd` is the visible repo-local default contract for planning and execution.
- Plan-level `tdd` values override this file for that plan; `inherit` falls back to these defaults.
- `tdd_enabled` mirrors whether `tdd.mode` is `ralph` until execution templates read the full `tdd` block directly.
