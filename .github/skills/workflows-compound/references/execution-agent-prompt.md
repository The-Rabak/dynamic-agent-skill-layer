---
model: claude-sonnet-4.6
platforms:
  copilot:
    model: gpt-5.3-codex
  opencode:
    model: openrouter/moonshotai/kimi-k2.6
---

# Execution Agent Prompt Template

This template is the **injected context packet** that `/workflows-work` passes into the named `execution-agent`.

**Canonical execution rules live in `agents/workflow/execution-agent.md`.** `/workflows-work` must load that bundled agent template, then inject the fully populated context packet below when dispatching `Task(execution-agent, prompt=scoped_prompt)`.

**This is NOT an invocable agent.** It is a reference document consumed by the orchestrator so the exact context scaffold ships with generated workflow bundles.

**Scaffold authority:** This file is the only valid source for the injected execution context packet. If you receive a shortened paraphrase, a prompt missing the sections below, or a prompt that still contains unresolved `{{PLACEHOLDER}}` tokens, stop and report that the execution template is incomplete. Do not proceed on a reconstructed or partial prompt.

---

The bundled `execution-agent` enforces clean-code, DRY, SOLID, feature-home boundary discipline, doc blocks above non-trivial functions/classes, imports at the top of files unless a real exception exists, explicit failure handling, and the structured execution report contract. Populate the scaffold below completely before dispatch.

## Your Unit

**Unit:** {{UNIT_TITLE}}

{{UNIT_DESCRIPTION}}

**Unit kind:** {{UNIT_KIND}}

**Outcome scenario:** {{OUTCOME_SCENARIO}}

**Feature home:** {{FEATURE_HOME}}

**Scope:** {{UNIT_SCOPE}}

**Scope fence:** {{UNIT_SCOPE_FENCE}}

**Files to create/modify:** {{FILE_LIST}}

**Success criteria:**
{{SUCCESS_CRITERIA}}

**Validation command:** `{{VALIDATION_COMMAND}}`

**Dependencies completed:** {{COMPLETED_DEPENDENCIES}}

**Parent refs:** {{PARENT_REFS}}

## Ticket-local context

{{TICKET_LOCAL_CONTEXT}}

## Why This Unit Exists

{{WHY_CONTEXT}}

## Architectural Context

{{ARCHITECTURAL_CONTEXT}}

## Architecture Handoff

{{ARCHITECTURE_HANDOFF}}

## Learnings from Previous Units

{{LEARNINGS_BRIEF}}

## Project Conventions

{{PROJECT_CONVENTIONS}}

## TDD Execution Contract

Use `references/tdd-evidence-contract.md` as the shared source of truth for contract resolution, Ralph evidence semantics, and report structure. Do not invent a lighter evidence format for convenience.

{{TDD_CONTRACT}}

### TDD Evidence

- Ralph is the default TDD execution path whenever the resolved contract selects Ralph-driven work.
- `Red` and `Green` prove behavior coverage.
- `Post-Refactor Green` proves cleanup safety.
- If no cleanup was needed, still rerun and say so.
