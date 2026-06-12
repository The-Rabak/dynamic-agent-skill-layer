# clband instrument-authoring spec (T23 Unit A — per full context)

You author the **measured instruments** for ONE CL-bench context of the T14 acquisition band, the
same way `author_smoke_instruments.py` + `verifiers/flywheel-assembly.sh` did for the two smoke
contexts. Every instrument is committed BEFORE that context's measured run (pre-registration fence).
Hallucinated sentinels VOID a context (the `M-WARN-01`/`LMI-2025` lesson) — so every operative
sentinel is mechanically verified against the context text, and every verifier is self-tested on a
good/bad fixture pair (Ralph RED/GREEN) before you finish.

## Inputs (read these for YOUR context `<name>` only)
- `tests/e2e/efficacy/clband/contexts/<name>/system.md` — the system-prompt knowledge (if any)
- `tests/e2e/efficacy/clband/contexts/<name>/context.md` — the user-document knowledge (if any)
- `tests/e2e/efficacy/clband/contexts/<name>/tasks.json` — the sibling tasks: each has `task_id`,
  `question`, `rubrics` (the CL-bench grading rubrics, verbatim), and depth.
- The manifest entry: `tests/e2e/efficacy/clband/manifest.json` → `.contexts[] | select(.short==<short>)`
  (gist, sub_category, n_tasks, task_depths, sentinels). READ it; do NOT edit it.

## Sibling selection (apply the plan's rules)
- **`knowledge_home`** is given to you (`system` → the rules live in `system.md`; `user` → in
  `context.md`). The **doc_file** is `system.md` when `knowledge_home=system`, else `context.md`.
- **Teach sibling** = the SHALLOWEST sibling (lowest depth). For a nested context whose depth-2 turn
  fuses the document with the task, that fused turn is the teach prompt (like aether).
- **Measured siblings** = ONE or TWO held-out siblings that (a) are NOT the teach sibling, (b) become
  self-contained after de-referencing (inline any snippet they reference so no `prior_turns` are
  needed — flat siblings are ideal; for nested, pick the one(s) that self-contain cleanly), and
  (c) admit **≥5 deterministic checks** from their rubrics. Prefer flat siblings. If only one sibling
  qualifies, author one; never invent a sibling.

## Deliverables (create ONLY these files; do NOT touch shared files)

1. **Verifier** `tests/e2e/efficacy/clband/verifiers/<name>.sh` (one verifier; the siblings of a
   context exercise the SAME invented rules, like flywheel). Model it on
   `verifiers/flywheel-assembly.sh`:
   - `set -uo pipefail`; `ws="${1:?usage}"`; read `$ws/solution.md` (fallback: all `*.md`/`*.txt`
     not under `.git`); fail loud "LOSS: no answer text" if empty.
   - **≥5 deterministic checks**, each compiled VERBATIM from a knowledge rubric in `tasks.json`.
     Knowledge rubrics only — values, codes, orderings, named procedures, structural musts. DROP
     persona/boldface/tone/format-only rubrics from the deterministic core (they go to the judge).
   - Use `grep -iF` for literal strings, `grep -iE` for value/phrasing variants (be tolerant of
     paraphrase, like flywheel's `extra torque|more torque|...`). Each failed check: `fail "LOSS:
     <reason naming the invented rule>"`. End: `echo "WIN: ..."; exit 0`.
   - Exit 0 == invented rules OBEYED (task win); non-zero == loss. Pure deterministic inspection;
     NO network, NO LLM, NO model calls.
   - `chmod +x` it (or note it; the orchestrator will ensure +x).

2. **Fixtures** `tests/e2e/efficacy/clband/fixtures/<name>-good/solution.md` and `.../<name>-bad/solution.md`:
   - **good** = a solution that OBEYS every invented rule → your verifier MUST exit 0 on it.
   - **bad** = a plausible OFF-style answer (model defaults, the invented rule absent/violated) →
     your verifier MUST exit non-zero on it.
   - RUN your verifier on both (`bash verifiers/<name>.sh fixtures/<name>-good` and `.../<name>-bad`);
     confirm good→0, bad→non-0. This is your Ralph RED/GREEN evidence. If either is wrong, fix the
     verifier or the fixture until both behave. (Light bash only — this is allowed.)

3. **De-referenced task spec(s)** `tests/e2e/efficacy/clband/tasks/clband-<name>-<short8>.json`, one per
   measured sibling, CONTRACT-shaped (copy the schema from `author_smoke_instruments.py` `write_spec`):
   keys `task_id` (= the slug `clband-<name>-<short8>`), `title`, `_clband` (context, sibling_task_id,
   `measured_without_prior_turns: true`, note), `invented_rule` (`summary` [ALSO the focused ON
   inject-query], `corpus_skill_slug: "PENDING-UNTIL-EXTRACTION"`, `corpus_skill_id:
   "PENDING-UNTIL-EXTRACTION"`, `absent_from_pretraining_rationale`), `prompt`, `workspace`
   (`{"kind":"scratch","base_ref":null,"setup":[]}`), `verifier` (`command`:
   `tests/e2e/efficacy/clband/verifiers/<name>.sh`, `contract`), `expected`
   (`{"on":"pass","off":"fail","placebo":"fail","sensitivity_note": "...INSTRUMENT-FAILURE(injection/obedience) if ON fails with the rule-bearing skill injected"}`).
   - **prompt** = a system-naming de-reference frame (names the invented system so ON retrieval can
     find the skill; adds NO rule content) + the sibling `question` VERBATIM + the workspace instr:
     `"\n\nWrite your complete response to a file named \`solution.md\` in the current working
     directory. Output only that file."`. For nested siblings, inline the referenced snippet so the
     task is self-contained, exactly like the aether translate sibling.

4. **Judge prompt(s)** `tests/e2e/efficacy/clband/judge/clband-<name>-<short8>.md` — VERBATIM rubrics,
   `JUDGE: WIN` only if all PASS else `LOSS`, secondary score. Copy `write_judge`'s format.

5. **Teach workspace** `tests/e2e/efficacy/clband/teach/<name>/`:
   - copy the doc_file into it (e.g. `<name>-doc.md`),
   - `prompt.txt` = a teach prompt that makes the agent WORK the teach sibling's task by APPLYING the
     document's rules (read the doc, reason, write `solution.md`) — NOT paste-and-quit. Follow
     `setup_teach_workspaces.py`'s flywheel/aether pattern.

6. **Operative + document sentinels + metadata** `tests/e2e/efficacy/clband/instruments/<name>.json`:
   ```json
   {
     "context": "<name>", "short": "<short8>", "knowledge_home": "<system|user>",
     "doc_file": "<system.md|context.md>",
     "teach_sibling_id": "<full task_id of the teach sibling>",
     "teach_prompt_file": "teach/<name>/prompt.txt",
     "measured_siblings": [
       {"slug": "clband-<name>-<short8>", "sibling_task_id": "<full>", "verifier": "<name>.sh",
        "summary": "<the invented_rule.summary, = ON inject-query>"}
     ],
     "sentinels_operative": ["<verbatim strings/numbers the verifier checks; the GATING tier>"],
     "sentinels_document": ["<system names/personas; reported, not gating>"],
     "self_test": {"good_exit": 0, "bad_exit": <non-zero>, "sentinels_verified": true}
   }
   ```
   - **sentinels_operative** = the literal constants/strings/codes your verifier greps for (the rule
     Session B needs). For EACH, run `grep -iF -- "<sentinel>" contexts/<name>/<doc_file>` (and the
     other context file) and CONFIRM it is present. If a sentinel is NOT in the context text, it is
     HALLUCINATED — change the verifier check + sentinel to a string that IS present verbatim. Set
     `"sentinels_verified": true` ONLY after every operative sentinel matches the context text.
   - **sentinels_document** = invented system names / personas (e.g. the system title) — reported
     tier, not gating.

## Hard fences (violations are unacceptable)
- Do NOT run cargo/clippy/build/test, Docker, the mcp-server, or any HTTP/model call. Do NOT spawn
  subagents. The ONLY commands you run are file reads/writes and light bash (`grep`, `chmod`, running
  YOUR verifier on YOUR fixtures). This protects the shared machine (a prior parallel-heavy run
  crashed it).
- Do NOT edit shared files: `manifest.json`, `teach_delivery.py`, `run_band.py`, `efficacy_ab.py`,
  `setup_teach_workspaces.py`, `author_smoke_instruments.py`, or any other context's files. The
  orchestrator merges your `instruments/<name>.json` into the manifest and re-verifies sentinels.
- NO rule content is ever added to a prompt frame — only the system NAME + the verbatim question.
- Provenance: you author measurement instruments, never plant rules into the corpus.

## Report back (structured)
- The teach sibling + measured sibling(s) you selected and WHY (flat/self-contained/≥5 checks).
- The ≥5 verifier checks, each mapped to its source rubric.
- Fixture self-test: good exit, bad exit (your Ralph RED/GREEN).
- The operative sentinels + the exact `grep` confirmation each is present verbatim in the context text.
- Files created (paths).
- Any sibling you rejected and why (e.g. couldn't reach 5 deterministic checks, intrinsically needs
  prior_turns).
