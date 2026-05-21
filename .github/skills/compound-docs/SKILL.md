---
name: compound-docs
description: Capture solved problems as categorized documentation with YAML frontmatter for fast lookup
model: gpt-5.3-codex
---

# compound-docs Skill

Capture non-trivial solved problems as searchable documentation in `docs/solutions/`. Keep the file short, structured, and schema-valid so future sessions can actually find and trust it.

## When to use
- A bug, migration issue, incident, or tricky setup problem is confirmed fixed.
- The solution required real investigation, multiple attempts, or a non-obvious insight.
- The team would benefit from preserving the failure mode, root cause, and prevention guidance.
- Skip trivial typos, obvious syntax mistakes, and fixes that needed no meaningful investigation.

## Workflow
1. Detect a confirmed fix. Auto-invoke on clear success phrases such as "that worked", "it's fixed", or "problem solved", or run manually.
2. Gather the required context: module, exact symptom/error, failed attempts, root cause, fix, prevention guidance, file references, and relevant environment details.
3. If critical context is missing, stop and ask for it before writing anything.
4. Search `docs/solutions/` for similar documents. Reuse or cross-link only when the root cause genuinely matches.
5. Build a filename as `[sanitized-symptom]-[module]-[YYYYMMDD].md`.
6. Validate YAML frontmatter against `schema.yaml` using the enum guidance in `references/yaml-schema.md`.
7. Create the solution doc with `assets/resolution-template.md`, then offer the post-capture menu.

## Required context
- Module or subsystem name.
- Exact symptom or error message.
- Stage or execution context.
- What failed during investigation.
- Root cause and what changed to fix it.
- Prevention guidance and any follow-up actions.

## Validation rules
- `problem_type`, `component`, `root_cause`, and `severity` must match the schema exactly.
- Use category mapping from `references/yaml-schema.md` to place the file under `docs/solutions/<category>/`.
- Block on invalid YAML, missing required fields, or vague symptoms.
- Include concrete examples or commands whenever the fix would be hard to repeat without them.

## Post-capture menu
After writing the document, present a terse next-step menu:
1. Continue workflow.
2. Add to Required Reading using `assets/critical-pattern-template.md`.
3. Link related issues.
4. Add the learning to an existing skill.
5. Create a new skill seeded by this solution.
6. View the captured document.
7. Other.

## Output
- A validated file in `docs/solutions/<category>/[filename].md`.
- Cross-references when similar issues exist.
- A short summary of what was captured and the next-step menu.

## Guardrails
- Do not proceed when module, symptom, stage, or resolution steps are missing.
- Do not skip schema validation.
- Do not write vague titles, vague symptoms, or root causes like "fixed code".
- Do not auto-promote to critical patterns; offer the option and let the user choose.
- Prefer one high-signal document over scattered notes across multiple files.
