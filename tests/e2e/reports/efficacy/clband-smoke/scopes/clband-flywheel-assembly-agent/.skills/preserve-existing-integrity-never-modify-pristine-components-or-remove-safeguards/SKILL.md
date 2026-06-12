---
name: Preserve existing integrity — never modify pristine components or remove safeguards
description: Components in a pristine state and existing mandatory safeguards must be left untouched. The burden of proof for any modification is extremely high; when in doubt, preserve rather than alter.
tags:
- integrity
- safety
- non-destructive
- safeguards
type: principle
origin: session_extraction
source_session_id: clband-teach-flywheel-assembly-agent
source_provider: claude-code
created_at: 2026-06-12T17:07:02.341778980+00:00
warning_at: 2026-07-12T17:07:02.341778980+00:00
expires_at: 2026-09-10T17:07:02.341778980+00:00
generality: general
generality_rationale: Appears in two structurally identical but domain-separate contexts (physical component integrity and procedural check integrity), indicating a general 'preserve over modify' discipline.
use_when:
- Working near or on a component flagged as pristine
- Refactoring or simplifying a procedure that contains mandatory checks
- Any operation where an existing safeguard could be silently bypassed or removed
avoid_when:
- The component has been explicitly decommissioned and replaced (remove protection only with explicit sign-off)
- A mandatory check has been demonstrably superseded by a stronger replacement check
invariants:
- A pristine component exits a work session in the same condition it entered.
- Mandatory checks remain present and active unless explicitly retired with documented justification.
produces:
- Unchanged pristine component state
- Intact mandatory-check set
evidence:
- 'Skill 2: The rotor is pristine; do not modify it'
- 'Skill 8: Do not remove any mandatory checks'
---

# Preserve existing integrity — never modify pristine components or remove safeguards

Components in a pristine state and existing mandatory safeguards must be left untouched. The burden of proof for any modification is extremely high; when in doubt, preserve rather than alter.

## Procedures
- Before any operation, identify whether the target component is in a pristine or protected state.
- If pristine or explicitly guarded, mark it off-limits for modification and route work around it.
- Treat mandatory checks as load-bearing: removing one requires explicit justification and re-approval, not convenience-driven deletion.

## Conventions
- Document pristine/protected status at the start of a work session so it cannot be forgotten mid-procedure.
- Mandatory checks are assumed load-bearing until proven otherwise.

