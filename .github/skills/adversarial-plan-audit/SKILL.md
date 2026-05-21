---
name: adversarial-plan-audit
description: Run a deep adversarial review of plans and architecture before implementation. Use when validating strategy docs, contracts, roadmaps, and competitive positioning with scored findings and prioritized recommendations.
model: gpt-5.3-codex
---

# Adversarial Plan Audit

Run a rigorous pre-implementation audit for architecture and plan quality, not a superficial review. Produce a scored, evidence-cited report that pressure-tests viability, operational readiness, and differentiation.

**Announce at start:** "I'm using the adversarial-plan-audit skill to run a scored adversarial architecture and plan review."

## When to Use

- Evaluate a major architecture or implementation plan before building
- Stress-test a memory/plugin/agent system intended for daily use
- Decide whether a design is ready for implementation or needs redesign
- Generate investor-grade or leadership-grade recommendation artifacts

## When Not to Use

- Fix isolated code bugs with clear root causes
- Perform style-only PR review
- Produce implementation plans from scratch without existing artifacts

## Required Inputs

- Primary architecture/plan/contracts documents
- Any benchmark/quality gates and constraints
- Target output location for final assessment artifact

If scope is ambiguous, ask for the target decision this audit should support (go/no-go, re-scope, or rank options).

## Core Workflow

### Step 1: Build the Audit Matrix

Load [references/audit-rubric.md](references/audit-rubric.md) and convert scope into explicit tracks:

1. Action parity
2. Tools as primitives
3. Context injection
4. Shared workspace
5. CRUD completeness
6. UX/feedback integration (including CLI and artifact visibility)
7. Capability discovery and onboarding
8. Prompt-native extensibility
9. Adversarial risk and contradiction analysis
10. Market positioning and differentiation

Apply weighting from the rubric and predefine scoring scale before collecting evidence.

### Step 2: Run Parallel Adversarial Tracks

Launch track investigations in parallel with subagents.

Dispatch rules:

1. Run up to platform concurrency limits (typically 4 concurrent). Use waves if needed.
2. Use specialized reviewers where available for architecture, security, performance, and data integrity.
3. Load local agent templates before named specialist dispatch:
   - Include `AGENT_TEMPLATE` directly in dispatched prompt.
4. Require each track to return:
   - numeric score in `X/Y` and percentage
   - concrete evidence with file/section citations
   - top gaps
   - high-impact recommendations first

### Step 3: Add Dedicated Red-Team and Market Passes

After core principle tracks complete:

1. Run red-team pass:
   - identify likely adoption failures
   - identify hidden coupling and contradiction risks
   - rank by impact x likelihood
2. Run market pass:
   - benchmark against credible competitors
   - classify differentiators vs commodity areas
   - identify strategic opportunities and traps

### Step 4: Synthesize, Normalize, and Prioritize

1. Normalize scores into a single weighted composite.
2. Resolve contradictions across tracks; do not average conflicting claims blindly.
3. Produce a prioritized recommendation list using:
   - impact on trust/adoption
   - effort
   - risk reduction
   - dependency order
4. Classify strengths separately from gaps.

### Step 5: Produce a Reusable Assessment Artifact

Load [references/report-template.md](references/report-template.md). Write the full report to a persistent docs path:

- Prefer `docs/assessements/` if that path exists in the repository.
- Otherwise use `docs/assessments/`.

Name format:

`YYYY-MM-DD-<project-or-topic>-adversarial-assessment.md`

The report must include:

1. Executive verdict and readiness rating
2. Per-principle score table with status badges
3. Adversarial risk matrix
4. Market positioning analysis
5. Top prioritized recommendations
6. Clear next-step decision framing (go / re-scope / redesign)

### Step 6: Return Tight Summary to User

Return concise summary in chat:

- overall score and confidence
- biggest 3 strengths
- biggest 3 blockers
- immediate next action

Do not paste full report in chat when an artifact file already exists.

## Output Quality Bar

- Cite all critical claims to source docs or credible external references.
- Avoid generic advice; bind recommendations to observed evidence.
- Distinguish design defects from planned non-goals.
- Flag uncertainty explicitly when evidence is weak.
- Prefer smallest high-leverage interventions before large redesigns.

## Failure Handling

- If subagent limits block full parallelism, run in waves and continue.
- If external research hits rate limits, proceed with available internal evidence and clearly mark confidence.
- If required docs are missing, stop and request exact missing artifacts.

## Success Criteria

- [ ] Core and adversarial tracks completed
- [ ] Numeric scored output generated per track
- [ ] Final artifact written to repository docs path
- [ ] Recommendations prioritized by impact
- [ ] Summary delivered with explicit decision framing
