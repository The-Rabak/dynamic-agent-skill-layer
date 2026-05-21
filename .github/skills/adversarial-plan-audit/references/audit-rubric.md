# Adversarial Plan Audit Rubric

Use this rubric to score architecture and plan quality consistently across projects.

## Scoring System

- Use `X/Y` and percentage per track.
- Use status bands:
  - `✅ Excellent` = 80-100%
  - `⚠️ Partial` = 50-79%
  - `❌ Needs Work` = 0-49%

## Weighted Composite

Apply weighted averaging for final score:

| Track | Weight |
|---|---:|
| Action parity | 10% |
| Tools as primitives | 10% |
| Context injection | 10% |
| Shared workspace | 10% |
| CRUD completeness | 10% |
| UX/feedback integration | 10% |
| Capability discovery | 10% |
| Prompt-native extensibility | 10% |
| Adversarial risk profile | 10% |
| Market differentiation | 10% |

Adjust weights only if the user requests alternative priorities.

## Track-Level Evaluation Criteria

## 1) Action parity

- Enumerate user actions across all surfaces.
- Verify agent-equivalent path for each action.
- Penalize implicit/manual-only operations.

## 2) Tools as primitives

- Classify callable surfaces as primitive vs workflow-heavy.
- Penalize business logic hidden in transport/tool adapters.

## 3) Context injection

- Verify dynamic context categories: resources, readiness, scope, history, preferences, capabilities.
- Penalize stale or non-deterministic context assembly.

## 4) Shared workspace

- Verify agent and humans operate on same files/data plane.
- Penalize shadow state and split-brain diagnostics.

## 5) CRUD completeness

- Evaluate CRUD coverage for critical entities, not just one object type.
- Penalize missing operational lifecycle controls.

## 6) UX/feedback integration

- Verify user-visible feedback for async work and state transitions.
- Penalize silent background actions and weak progress observability.

## 7) Capability discovery

- Verify onboarding, docs, command listing, examples, empty-state guidance.
- Penalize undocumented or hard-to-discover capabilities.

## 8) Prompt-native extensibility

- Evaluate whether behavior evolves via prompts/policy vs code surgery.
- Penalize rigid orchestrations where safe prompt-level configuration is possible.

## 9) Adversarial risk profile

- Build impact x likelihood matrix.
- Penalize unresolved contradictions and trust-eroding failure modes.

## 10) Market differentiation

- Compare against credible competitors.
- Penalize commodity-only value propositions.

## Recommendation Prioritization Rules

Rank recommendations by:

1. Adoption/trust impact
2. Risk reduction
3. Implementation effort
4. Dependency ordering

Prefer small, high-leverage changes first.

