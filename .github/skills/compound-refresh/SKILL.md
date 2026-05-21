---
name: compound-refresh
description: Refresh stale or drifting learnings and pattern docs in docs/solutions/ against the current codebase
model: gpt-5.3-codex
---

# Compound Refresh

Maintain the quality of `docs/solutions/` over time. Review existing learnings against the current codebase, then refresh any derived pattern docs that depend on them.

## Mode Detection

Check if `$ARGUMENTS` contains `mode:autonomous`. If present, strip it from arguments and run in **autonomous mode**.

| Mode | When | Behavior |
|------|------|----------|
| **Interactive** (default) | User is present and can answer questions | Ask for decisions on ambiguous cases and confirm disruptive actions |
| **Autonomous** | `mode:autonomous` in arguments | No user interaction. Apply unambiguous actions, mark ambiguous cases stale, and generate a full report |

### Autonomous mode rules

- Skip all user questions.
- Process all docs in scope. If no scope hint was provided, process everything.
- Attempt all safe actions: Keep, Update, auto-Archive, and Replace when evidence is sufficient.
- If a write succeeds, record it as **applied**.
- If a write fails, record it as **recommended** in the report and continue.
- If classification is genuinely ambiguous, mark the doc stale with `status: stale`, `stale_reason`, and `stale_date`.
- Use conservative confidence. Borderline cases become stale in autonomous mode.
- Always generate a report with **Applied** and **Recommended** sections.

## Local Grounding

Before editing anything under `docs/solutions/`, read the local guidance files in this order:
1. `AGENTS.md`
2. `CLAUDE.md` (if present)
3. `README.md`

Use them to preserve the repository's docs conventions, OpenViking guidance, and workflow naming.

## Interaction Principles

These principles apply to interactive mode only. In autonomous mode, skip all user questions and apply the autonomous rules above.

Follow the same interaction style as `/workflows-brainstorm`:
- ask questions one at a time
- prefer multiple choice when natural options exist
- start with scope and intent, then narrow only when needed
- do not ask the user to decide before you have evidence
- lead with a recommendation and explain it briefly

The goal is to help the user make a good maintenance decision with minimal friction.

## Refresh Order

Refresh in this order:
1. review the relevant individual learning docs first
2. note which learnings stayed valid, were updated, were replaced, or were archived
3. then review any pattern docs that depend on those learnings

Why this order:
- learning docs are the primary evidence
- pattern docs are derived from one or more learnings
- stale learnings can make a pattern look more valid than it really is

If the user starts by naming a pattern doc, you may begin there to understand the concern, but inspect the supporting learning docs before changing the pattern.

## Maintenance Model

For each candidate artifact, classify it into one of four outcomes:

| Outcome | Meaning | Default action |
|---------|---------|----------------|
| **Keep** | Still accurate and still useful | No file edit by default; report that it remains trustworthy |
| **Update** | Core solution is still correct, but references drifted | Apply evidence-backed in-place edits |
| **Replace** | The old artifact is now misleading, but there is a known better replacement | Create a trustworthy successor, then archive or supersede the old artifact |
| **Archive** | No longer useful or applicable | Move the obsolete artifact to `docs/solutions/_archived/` with archive metadata |

## Core Rules

1. **Evidence informs judgment.** Use engineering judgment; do not treat this as a mechanical scorecard.
2. **Prefer no-write Keep.** Do not edit a doc just to leave a review breadcrumb.
3. **Match docs to reality, not the reverse.** If current code differs from a learning, update the learning to reflect the codebase as it exists today.
4. **Be decisive, minimize questions.** When evidence is clear, apply the update. Only ask when the action is genuinely ambiguous.
5. **Avoid low-value churn.** Do not edit a doc for cosmetic wording or style-only cleanup.
6. **Use Update only for meaningful, evidence-backed drift.** Paths, module names, links, code snippets, and metadata are fair game when accuracy improves materially.
7. **Use Replace only when there is a real replacement.** The current codebase, related docs, or recent verified fixes must provide enough evidence to document the successor honestly.
8. **Archive when the code is gone and the problem domain is gone.** Missing referenced files with no matching modern equivalent is strong archive evidence.

## Scope Selection

Start by discovering learnings and pattern docs under `docs/solutions/`.

Exclude:
- `README.md`
- `docs/solutions/_archived/`

Find all `.md` files under `docs/solutions/`, excluding `README.md` files and anything under `_archived/`.

If `$ARGUMENTS` is provided, use it to narrow scope in this order:
1. **Directory match** - a subdirectory under `docs/solutions/`
2. **Frontmatter match** - `module`, `component`, or `tags`
3. **Filename match** - partial filename match is fine
4. **Content search** - keyword search inside files

If no matches are found, report that and ask the user to clarify. In autonomous mode, report the miss and stop.

If no candidate docs are found, report:

```text
No candidate docs found in docs/solutions/.
Run `/workflows-compound` after solving problems to start building your knowledge base.
```

## Phase 0: Assess and Route

Before asking the user to classify anything:
1. discover candidate artifacts
2. estimate scope
3. choose the lightest interaction path that fits

### Route by Scope

| Scope | When to use it | Interaction style |
|-------|----------------|-------------------|
| **Focused** | 1-2 likely files or a specific named doc | Investigate directly, then present a recommendation |
| **Batch** | Up to about 8 mostly independent docs | Investigate first, then present grouped recommendations |
| **Broad** | 9+ docs, ambiguous, or repo-wide stale-doc sweep | Triage first, then investigate in batches |

### Broad Scope Triage

When scope is broad:
1. inventory frontmatter and group by module, component, or category
2. identify dense clusters of learnings plus pattern docs
3. spot-check whether the primary referenced files still exist
4. recommend a starting area; in autonomous mode, process all clusters in impact order

Do not ask action-selection questions yet. First gather evidence.

## Phase 1: Investigate Candidate Learnings

For each learning in scope, read it, cross-reference its claims against the current codebase, and form a recommendation.

A learning can go stale in several dimensions:
- **References** - do the file paths, class names, and modules still exist or have they moved?
- **Recommended solution** - does the fix still match how the code works today?
- **Code examples** - if snippets exist, do they still reflect the current implementation?
- **Related docs** - are cross-referenced learnings and patterns still present and consistent?
- **Local guidance alignment** - does the learning still match current repository conventions documented in `AGENTS.md`, `CLAUDE.md`, and `README.md`?

Match investigation depth to the learning's specificity. A learning with exact file paths and code snippets needs more verification than one describing a general principle.

### Drift Classification: Update vs Replace

The critical distinction is whether the drift is **cosmetic** or **substantive**:

- **Update territory** - file paths moved, classes renamed, links broke, metadata drifted, but the core recommended approach is still correct.
- **Replace territory** - the recommended solution conflicts with current code, the architecture changed, or the preferred pattern is different.

The boundary: if you find yourself rewriting the solution section or changing what the learning recommends, stop. That is Replace, not Update.

### Judgment Guidelines

Three guidelines that are easy to get wrong:
1. **Contradiction = strong Replace signal.** If the learning conflicts with current code or a recently verified fix, it is actively misleading.
2. **Age alone is not a stale signal.** Use age only as a prompt to inspect more carefully.
3. **Check for successors before archiving.** Before recommending Replace or Archive, look for newer learnings, pattern docs, PRs, or issues covering the same problem space.

## Phase 1.5: Investigate Pattern Docs

After reviewing the underlying learning docs, investigate any relevant pattern docs under `docs/solutions/patterns/`.

Pattern docs are high-leverage. A stale pattern is more dangerous than a stale individual learning because future work may treat it as broadly applicable guidance.

A pattern doc with no clear supporting learnings is itself a stale signal.

## Subagent Strategy

Use subagents for context isolation when investigating multiple artifacts. Choose the lightest approach that fits:

| Approach | When to use |
|----------|-------------|
| **Main thread only** | Small scope, short docs |
| **Sequential subagents** | 1-2 artifacts with many supporting files |
| **Parallel subagents** | 3+ truly independent artifacts with low overlap |
| **Batched subagents** | Broad sweeps; narrow scope first, then investigate in batches |

When spawning any subagent, include this instruction in its task prompt:

> Use dedicated file search and read tools for all investigation. Do not use shell commands for file operations. Report: file path, evidence, recommended action, confidence, and open questions.

There are two subagent roles:
1. **Investigation subagents** - read-only. They must not edit files or archive anything.
2. **Replacement subagents** - write a single new learning to replace a stale one. Run these one at a time.

The orchestrator merges investigation results, detects contradictions, coordinates replacement subagents, and performs archival or metadata edits centrally.

## Phase 2: Classify the Right Maintenance Action

After gathering evidence, assign one recommended action.

### Keep

The learning is still accurate and useful. Do not edit the file. Report that it remains trustworthy.

### Update

The core solution is still valid but references have drifted. Apply the fixes directly.

### Replace

Choose **Replace** when the learning's core guidance is now misleading.

Assess whether evidence is sufficient to write a trustworthy replacement:
- **Sufficient evidence** - you understand both what the old learning recommended and what the current approach is. Proceed to write the replacement.
- **Insufficient evidence** - you cannot confidently document the current approach. Mark the file stale in place with:
  - `status: stale`
  - `stale_reason`
  - `stale_date: YYYY-MM-DD`

### Archive

Choose **Archive** when:
- the code or workflow no longer exists
- the learning is obsolete and has no modern replacement worth documenting
- the learning is redundant and no longer useful on its own
- there is no meaningful successor evidence suggesting it should be replaced instead

Action:
- move the file to `docs/solutions/_archived/`
- add `archived_date: YYYY-MM-DD`
- add `archive_reason`

### Before archiving: check whether the problem domain still exists

If the referenced implementation is gone but the broader problem domain is still active, that is usually **Replace**, not **Archive**.

Auto-archive only when both the implementation and the problem domain are gone, or when the doc is fully superseded by a clearly better successor.

## Pattern Guidance

Apply the same four outcomes to pattern docs, but evaluate them as derived guidance:
- **Keep** - underlying learnings still support the generalized rule
- **Update** - the rule holds but examples, links, or supporting references drifted
- **Replace** - the rule is now misleading or the refreshed learnings support a different synthesis
- **Archive** - the pattern is no longer valid, recurring, or distinct

## Phase 3: Ask for Decisions

### Autonomous mode

Skip this entire phase. Do not ask any questions. Proceed directly to Phase 4:
- unambiguous Keep, Update, auto-Archive, and Replace with sufficient evidence -> execute directly
- ambiguous cases -> mark as stale
- then generate the report

### Interactive mode

Most Updates should be applied directly without asking. Ask only when:
- the right action is genuinely ambiguous
- you are about to Archive a document and the evidence is not unambiguous
- you are about to create a successor and want confirmation for that larger write

Do not ask whether code changes were intentional or whether the user wants to fix the code. Stay focused on doc accuracy.

#### Question Style

Always ask one question at a time. Prefer multiple choice. Lead with the recommended option and explain the rationale briefly.

#### Focused Scope

For a single artifact, present:
- file path
- 2-4 bullets of evidence
- recommended action

Then ask:

```text
This [learning/pattern] looks like a [Update/Keep/Replace/Archive].

Why: [one-sentence rationale based on the evidence]

What would you like to do?

1. [Recommended action]
2. [Second plausible action]
3. Skip for now
```

#### Batch Scope

For several learnings:
1. group obvious Keep cases together
2. group obvious Update cases together when fixes are straightforward
3. present Replace cases individually or in very small groups
4. present Archive cases individually unless the archive is unambiguous and safe to auto-apply

#### Broad Scope

If the user asked for a sweeping refresh:
1. narrow scope first
2. investigate a manageable batch
3. present recommendations
4. ask whether to continue to the next batch

## Phase 4: Execute the Chosen Action

### Keep Flow

No file edit by default. Summarize why the learning remains trustworthy.

### Update Flow

Apply in-place edits only when the solution is still substantively correct.

Valid in-place updates include:
- file or class reference renames
- outdated links to related docs
- refreshed implementation notes after a directory move
- stale frontmatter or metadata that materially affects lookup accuracy

Do not use Update for:
- cosmetic wording tweaks
- style-only cleanup
- anti-pattern guidance that must be replaced
- architecture changes that make the old guidance misleading

### Replace Flow

Process Replace candidates one at a time.

When evidence is sufficient:
1. spawn a single subagent to write the replacement learning
2. pass it:
   - the old learning's full content
   - investigation evidence
   - the target path and category
3. have it write the new learning following the `/workflows-compound` or `compound-docs` document structure: frontmatter, problem, root cause, current solution, code examples, and prevention tips
4. after it completes, the orchestrator:
   - adds `superseded_by` to the old learning
   - moves the old learning to `docs/solutions/_archived/`

When evidence is insufficient:
1. mark the learning stale in place
2. report what evidence was found and what is missing
3. recommend running `/workflows-compound` after the next real encounter with that area

### Archive Flow

Archive only when a learning is clearly obsolete or redundant. Do not archive a document just because it is old.

## Output Format

The full report must be printed as markdown output. The report is the deliverable.

After processing the selected scope, output:

```text
Compound Refresh Summary
========================
Scanned: N learnings

Kept: X
Updated: Y
Replaced: Z
Archived: W
Skipped: V
Marked stale: S
```

Then for every file processed, list:
- file path
- classification (Keep/Update/Replace/Archive/Stale)
- evidence found
- action taken or recommended

For **Keep** outcomes, list them under a reviewed-without-edits section so the result is visible without creating git churn.

### Autonomous mode output

In autonomous mode, the report is the sole deliverable. Print every section in full.

Split actions into two sections:

**Applied** (writes that succeeded):
- Updated files
- Replaced files and their successors
- Archived files
- Marked stale files

**Recommended** (actions that could not be written):
- same level of detail, framed as recommendations for a human to apply

If all writes succeed, the Recommended section is empty.

## Phase 5: Commit Changes

Skip this phase if no files were modified.

### Detect git context

Before offering options, check:
1. which branch is currently checked out
2. whether the working tree has other uncommitted changes beyond what compound-refresh modified
3. recent commit messages to match repo style

Stage only the files that `compound-refresh` modified.

### Autonomous mode

Use sensible defaults:
- on main or master, create a branch named for what was refreshed, commit, and attempt to open a PR
- on a feature branch, commit as a separate commit on the current branch
- if git operations fail, include the recommended git commands in the report

### Interactive mode

If on the default branch, offer:
1. create a branch, commit, and open a PR (recommended)
2. commit directly to the current branch
3. do not commit

If on a feature branch with a clean working tree, offer:
1. commit to the current branch as a separate commit (recommended)
2. create a separate branch and commit
3. do not commit

If on a feature branch with other dirty changes, offer:
1. commit only the compound-refresh changes to the current branch
2. do not commit

### Commit message

Write a descriptive commit message that:
- summarizes what was refreshed
- follows the repo's existing commit conventions
- stays succinct

## Relationship to `/workflows-compound`

- `/workflows-compound` captures a newly solved, verified problem
- `/workflows-compound-refresh` maintains older learnings as the codebase evolves

Use **Replace** only when the refresh process has enough real evidence to write a trustworthy successor. When evidence is insufficient, mark as stale and recommend `/workflows-compound` for when the user next encounters that problem area.
