---
name: grill-with-docs
description: Grilling session that challenges your plan against the existing domain model, sharpens terminology, and updates documentation (CONTEXT.md, brainstorm docs, plan docs, ADRs) inline as decisions crystallise. Use when user wants to stress-test a plan against their project's language and documented decisions.
---

<what-to-do>

Interview me relentlessly about every aspect of this plan until we reach a shared understanding. Walk down each branch of the design tree, resolving dependencies between decisions one-by-one. For each question, provide your recommended answer.

Ask the questions one at a time, waiting for feedback on each question before continuing.

If a question can be answered by exploring the codebase, explore the codebase instead.

</what-to-do>

<supporting-info>

## Domain awareness

During codebase exploration, also look for existing documentation, especially the active feature artifact for the current discussion.

### File structure

Most repos have a repo-wide constitution, a glossary-oriented `CONTEXT.md`, and feature documents under `docs/`:

```
/
├── CONSTITUTION.md
├── CONTEXT.md
├── docs/
│   ├── brainstorms/
│   │   └── 2026-04-30-checkout-race-brainstorm.md
│   ├── plans/
│   │   └── 2026-05-01-fix-checkout-race-plan.md
│   └── architecture/
│       ├── 2026-04-30-nucleus-stage-1-architecture.md
└── src/
```

Create files lazily -- only when you have something to write. If no `CONTEXT.md` exists, create one when the first term is resolved. If no `CONSTITUTION.md` exists, advise the user to create one using the workflows-constitution command using the context from this session.

## During the session

### Choose the right documentation sink

Before grilling, decide where concrete decisions belong:

1. If a plan file exists for the current feature, or the session is clearly continuing plan work, the plan file is the implementation-decision sink.
2. Otherwise, if a brainstorm document exists for the current feature, or the session is clearly continuing brainstorm work, the brainstorm document is the implementation-decision sink.
3. `CONTEXT.md` is only for canonical domain language. ADRs remain for cross-feature decisions that deserve a durable architectural record.
4. If neither a plan nor a brainstorm artifact exists, do not invent one just for this skill unless the user explicitly asks for it.

### Challenge against the glossary

When the user uses a term that conflicts with the existing language in `CONTEXT.md`, call it out immediately. "Your glossary defines 'cancellation' as X, but you seem to mean Y — which is it?"

### Sharpen fuzzy language

When the user uses vague or overloaded terms, propose a precise canonical term. "You're saying 'account' — do you mean the Customer or the User? Those are different things."

### Discuss concrete scenarios

When domain relationships are being discussed, stress-test them with specific scenarios. Invent scenarios that probe edge cases and force the user to be precise about the boundaries between concepts.

### Cross-reference with code

When the user states how something works, check whether the code agrees. If you find a contradiction, surface it: "Your code cancels entire Orders, but you just said partial cancellation is possible — which is right?"

### Update CONTEXT.md inline

When a term is resolved, update `CONTEXT.md` right there. Don't batch these up — capture them as they happen. Use the format in [CONTEXT-FORMAT.md](./context-format.md).

### Update the active feature doc inline

After each question is answered with concrete implementation, architecture, data-shape, API, dependency, boundary, rollout, or operational detail, immediately write it into the active feature doc. Do not wait until the end of the session, and do not leave the decision only in chat history.

Prefer updating the most specific existing section over inventing a catch-all notes bucket:

- **Brainstorm doc:** update `## Chosen Approach`, `## Key Decisions`, `## Architectural Context`, and move answered items into `## Resolved Questions`.
- **Plan doc:** update `## Implementation` or `## Overview`, `## Technical Considerations`, `## Architectural Context`, `## Success Criteria`, and the relevant execution slice, acceptance criteria, or file list when the answer changes execution shape.
- If a new answer supersedes earlier wording, edit the earlier section in place so the document stays coherent.

`CONTEXT.md` should be totally devoid of implementation details. Do not treat `CONTEXT.md` as a spec, a scratch pad, or a repository for implementation decisions. It is a glossary and nothing else.


</supporting-info>
