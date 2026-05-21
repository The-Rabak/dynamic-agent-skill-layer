---
name: agent-native-architecture
description: Build applications where agents are first-class citizens. Use this skill when designing autonomous agents, creating MCP tools, implementing self-modifying systems, or building apps where features are outcomes achieved by agents operating in a loop.
model: gpt-5.3-codex
---

# agent-native-architecture Skill

Design software where agents are first-class operators, not an afterthought. Start with parity, primitive tools, and explicit contracts so new features can be delivered as prompts, not hard-coded workflows.

## When to use
- Designing or refactoring an agent-native application.
- Creating MCP tools, tool contracts, or system prompts.
- Reviewing action parity between UI and agent capabilities.
- Producing the architecture artifact consumed by the compound workflow chain.

## Workflow
1. Identify the user's goal: design, files/workspace, tool design, domain tools, execution patterns, system prompts, context injection, action parity, self-modification, product design, mobile patterns, testing, or refactoring.
2. Load only the reference files needed for that topic, then map the advice onto the current product, data model, and deployment constraints.
3. Run the architecture checklist below before recommending new abstractions.
4. If this is part of the compound workflow chain, turn the guidance into a concrete architecture artifact under `docs/architecture/`.

## Core principles
- **Parity**: if the user can do it, the agent can achieve the same outcome.
- **Granularity**: tools are primitives; features are prompt-defined outcomes.
- **Composability**: new capabilities should come from new prompts before new code.
- **Emergent capability**: open-ended requests should reveal gaps and product demand.
- **Improvement over time**: context, prompt refinement, and safe self-modification should make the system better with use.

## Reference router
- Design from scratch -> `references/architecture-patterns.md`
- Files and workspace design -> `references/files-universal-interface.md`, `references/shared-workspace-architecture.md`
- MCP and primitive tool design -> `references/mcp-tool-design.md`
- Domain-tool thresholds -> `references/from-primitives-to-domain-tools.md`
- Execution and completion loops -> `references/agent-execution-patterns.md`
- System prompt design -> `references/system-prompt-design.md`
- Dynamic context injection -> `references/dynamic-context-injection.md`
- Action parity reviews -> `references/action-parity-discipline.md`
- Self-modification -> `references/self-modification.md`
- Product implications -> `references/product-implications.md`
- Mobile patterns -> `references/mobile-patterns.md`
- Testing -> `references/agent-native-testing.md`
- Refactoring legacy systems -> `references/refactoring-to-prompt-native.md`

## Architecture checklist
- Every important UI action has an agent path.
- Tools expose primitives, not hidden business decisions.
- CRUD completeness exists where the domain needs it.
- Agents and users share the same workspace or data plane.
- Runtime context explains available resources, capabilities, and vocabulary.
- Long-running work has explicit completion, partial progress, and resume rules.
- The design still works under bounded context windows.

## Compound Architecture Artifact Contract
When this skill is used inside the compound workflow chain, produce an artifact that downstream phases can consume directly.

Use this vocabulary exactly:
- **Deepening candidates** -- areas that need more design work before `/deepen-plan`.
- **Feature home** -- the primary namespace or directory where one feature's business behavior should live.
- **Shared / global** -- code whose reason to change is truly cross-feature or infrastructural.
- **Deletion test** -- what can be removed, avoided, or delayed before adding structure.
- **Interface as test surface** -- the behavior callers and tests rely on.
- **Seam** -- where behavior can be swapped, isolated, or tested.
- **Adapter** -- the boundary component that translates external detail into domain-safe behavior.

Required sections for the artifact:
- Problem framing and constraints.
- Feature homes and ownership.
- Shared / global decisions.
- Capability map and parity gaps.
- Deepening candidates.
- Context tiers.
- Deletion test outcomes.
- Interfaces as test surfaces.
- Seams, adapters, and explicit contracts.
- Design-it-twice options when two credible architectures remain.
- Recommended next step for `/deepen-plan` or implementation.

## Output
- A concise architecture recommendation grounded in the chosen references.
- When part of the workflow chain, an artifact under `docs/architecture/` that uses the contract vocabulary above.
- Clear parity gaps, tool gaps, and follow-up actions.

## Guardrails
- Do not recommend agent features that lack an execution path.
- Do not hide business logic inside tools when a prompt or primitive would suffice.
- Do not treat architecture as commentary; emit an artifact when the workflow requires it.
- Prefer deletion and simpler seams before adding abstractions.
- Ask for missing domain constraints only when they materially change the architecture recommendation.
