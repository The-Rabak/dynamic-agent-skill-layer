# Multi-view extraction prompt redesign (ground-up)

**Status:** IMPLEMENTED — awaiting prompt review before the extraction run. All changes landed in
`prompt_contract.rs` (the prose extractor — the real workhorse), the domain model, the writer, a new
grounding validator, and the synthesis prompt. Unit + roundtrip suites green (domain 13, infrastructure
191, session-extractor 174, maintenance roundtrip 3; workspace compiles). Q1/Q2 + all open Qs approved.
One softening of §7 flagged below (empty-evidence is KEPT, not rejected — recall-first; ≥1 anchor must
ground for non-empty evidence). NOT yet wired to an extraction run.

**CORRECTION (2026-06-10, post-approval, after reading `infrastructure/src/extraction/prompt_contract.rs`):**
The premise in §1 below ("no prompt elicits the multi-view fields") was **incomplete**. The real
per-episode workhorse is NOT the `seams.rs` skeleton labeler — it is the **prose extractor**
(`TranscriptSkillExtractionService`, the "universal floor" run on every window), whose prompt lives
in `crates/infrastructure/src/extraction/prompt_contract.rs`. That prompt **already elicits all 7
multi-view fields** (text path + Claude tool schema), plus best-practices, failure-modes (quality
weight 0.30), and user preferences. The corpus is empty for two real reasons: (a) it is **pre-T03**
(claude-code campaign 2026-06-07; fields added to the prompt in T03 2026-06-09), and (b) the fields
are framed as **optional afterthoughts** ("omit if you cannot fill them accurately"), so models skip
them. **Revised target:** rewrite the `prompt_contract.rs` prompts to be research-grade — elevate
the multi-view fields to first-class/expected, restructure around the taxonomy (§3), and add
evidence grounding (§7). The `seams.rs` synthesis prompt is enriched secondarily. The skeleton
labeler stays as-is (it only names a mined arc; the prose extractor owns multi-view). The deliverable
(top-notch prompts populating the views + capturing iterations/preferences/best-practices) is
unchanged; only the file and the framing ("elevate" not "add") change.

---

**Original premise (kept for the record):** DRAFT — for owner review before any wiring or extraction run.
**Date:** 2026-06-10
**Owner decision that triggered this:** "build the prompts and let me review then. these prompts
need to be top notch, re-evaluate them from the ground up. the patterns extracted should not just
be explicit solutions to problems but also culminations of repeatable iterations and the final
solutions reached, patterns learned, user preferences gleaned, best practices discovered and more.
think about in-context-learning benchmarks like CL-bench (arXiv:2602.03587). it's one of the core
pillars of the app."

This document contains the **actual prompt text** proposed for review. Nothing here is wired into
code yet. The companion code changes (which prompt builders / structs / tests change) are listed in
§9 and are deliberately deferred until this design is approved.

---

## 1. Why this exists / the gap

Extraction is the core pillar: every extracted skill becomes the *context* a future agent must
learn from and apply. Retrieval quality (T04) has already hit a ceiling (held-out MRR 0.767, zero
uplift from candidate-gen or hybrid) — the lever is **item quality**, not retrieval breadth. So the
value of the whole system is bounded by how good the extracted skills are.

Today's extraction is structurally narrow. Three LLM seams exist and **none** elicit the multi-view
fields, and the per-episode path captures only a thin slice:

| Seam | What it does today | What it misses |
|------|--------------------|----------------|
| `LlmSkeletonLabeler` (`build_skeleton_labeling_prompt`) | Fires **only on build/test-failure tool arcs**; a "grounding invariant" lets the LLM only *name/judge* a deterministically-mined procedure. Emits `name/description/generality/keep/confidence`. | Everything that isn't a build/test failure: design decisions, refactors, **iterative refinements and their converged form**, **best practices**, **rules/heuristics**, **anti-patterns**. All 7 multi-view fields. |
| `LlmSynthesisPass` (`build_synthesis_prompt`) | Cross-episode "session-spanning pattern" pass. Emits `name/description/procedures/...`. | No contrast (success vs failure), no multi-view, no abstraction discipline. |
| `PreambleNormalizer` + keyword matcher | User preferences via literal substring matching (`"always "`, `"never "`). | Any preference not phrased with a trigger word; the *why*; preferences expressed across turns. |

Result: the 234-skill corpus has **all 7 multi-view fields empty** (confirmed in PG: populated
count = 0 for use_when/avoid_when/requires/invariants), which is also what crashed the T09 dense
views at boot (blank-view embedding — fixed separately). T03 wired the *plumbing* (struct → writer →
reader → `skills` columns); the *prompts that fill it* were never built. That is this work.

---

## 2. Research foundation (what the literature says to do)

Two research sweeps (CL-bench + context-learnability; experiential skill-extraction systems). The
findings are strongly convergent. Full citations in §11.

**What makes an extracted skill actually usable downstream (CL-bench, arXiv:2602.03587):**
- Frontier models apply provided knowledge only **17–24%** of the time. The dominant failures are
  **"context ignored" (55–66%)** and **"context misused" (60–66%)** — *not* reasoning. So a skill
  must make its applicability obvious and its rules unambiguous.
- The **same model is ~3× better** when a rule is stated **explicitly and declaratively** vs implied
  (Legal/Regulatory 44.8% vs Math-formalism 15.9%).
- **Length cliff:** application drops from ~25–35% at 0–4K tokens to **5–10% at 32K+**. Skills must
  be **single-purpose, short, front-loaded**.
- **Format compliance** is a scored rubric *and* a top-3 failure mode → state expected output.

**What structure transfers best (ReasoningBank, Trace2Skill, SkillRevise, AWM, ACE):**
- The highest-transfer unit = **trigger (literal keywords) + explicit rule/principle + ordered
  procedure + failure-mode pitfalls (mined from what went *wrong*) + verifiable expected outcome**,
  **distilled** (not transcribed), **detailed** (not over-compressed), single-purpose.
- **Mining failures is the single biggest quality lever** (SkillRevise: −29 pts when the failure-
  diagnosis component is removed; ReasoningBank ingests failed runs for "counterfactual signals and
  pitfalls"). This is *exactly* the "culmination of repeatable iterations" the owner asked for.
- **Extract by CONTRAST** (success vs failure), **in batches**, with **success/failure-split
  prompts** — not one "summarize the lessons" prompt per transcript. Naive single-trajectory
  summarization produces shallow restatement.
- **Abstract literals** (paths/IDs/values → `{variables}`), keep **one** concrete worked example.
- **Don't over-compress** (ACE "brevity bias degrades performance") — but **index/embed on the
  summary/trigger, not the body** (matches our existing ℓ₁=summary policy).
- **Cap quantity, forbid redundancy, require evidence citations** back to source turns.
- Single-task extraction without a frequency gate *grows the repo 2.4× with a 17.3% performance
  drop* (AutoRefine) → generality must come from **cross-session corroboration**, and per-episode
  output must stay small and high-precision.

---

## 3. The knowledge taxonomy we will extract

Ten reusable knowledge types (consolidated from ExpeL / ReasoningBank / AWM / AutoRefine / Voyager /
Generative-Agents / the 2026 memory surveys), each mapped to our existing schema fields and the
T09 embedding views. **This is the contract the prompts must elicit.**

| # | Type | One-line def | Primary schema fields | View |
|---|------|--------------|-----------------------|------|
| 1 | **Procedure / workflow** | Ordered reusable sub-routine for a recurring sub-task | `procedures`, `tools`, `artifacts`, `produces` | e_task |
| 2 | **Rule / heuristic** | Conditional "when X, do/check Y" guideline | `invariants`, `conventions`, `use_when` | e_needs |
| 3 | **Anti-pattern / what-to-avoid** | Plausible-but-wrong move; negative signal | `avoid_when`, Failure Modes | e_negative |
| 4 | **Failure→fix pair** | A specific observed error bound to its correction | `procedures` (fix) + `avoid_when` (the trap) + Evidence | e_task + e_negative |
| 5 | **Prerequisite / precondition** | State/resource that must hold first | `requires` | e_needs |
| 6 | **Preference** | Non-correctness choice the user/project consistently wants | `conventions`, `generality`, `use_when` | e_summary/e_task |
| 7 | **Best practice (cross-task)** | Positive pattern recurring across *multiple* successes | `conventions`, `invariants`, `use_when` | e_task |
| 8 | **Generalizable principle** | High-altitude invariant above any single task | `invariants`, `conventions` | e_needs |
| 9 | **Refinement trajectory** | The trial → dead-end → converged-best path; capture the converged solution AS the procedure and the dead-ends AS pitfalls | `procedures` (converged) + `avoid_when` (dead-ends) + Evidence | e_task + e_negative |
| 10 | **Diagnostic strategy** | A reusable *way to investigate* (distinct from the fix) | `procedures`, `use_when` | e_task |

Types **3, 4, 9** are where the literature shows the largest marginal value and where naive
pipelines fail. Our own `MEMORY.md` is almost entirely types 2/3/4/9 ("suspect context truncation
first", "caps must align to the window", "Redis NOGROUP self-heal") — direct evidence these are the
durable, reused units. **The prompts make them first-class, not afterthoughts.**

---

## 4. Design decisions (★ = needs owner sign-off)

1. **★ Grounding posture — from "verbatim-only" to "abstracted-but-evidence-grounded."**
   The current skeleton invariant forbids the LLM from authoring procedures (only mined tool steps).
   The research is unanimous that *verbatim transcription does not transfer* — you must abstract
   literals and distill strategy. **Proposal:** allow the LLM to author/abstract procedures, BUT
   require every skill to carry an `## Evidence` section citing concrete transcript anchors (the
   actual command(s)/error(s)/file(s) it derived from), and add a **fail-loud grounding validator**:
   if a skill cites a command/error that does not appear in the source transcript, the candidate is
   rejected (not silently kept). This keeps us honest (consistent with the no-fakes / fail-loud
   mandate) while gaining transfer. *This replaces a hard anti-hallucination guarantee with an
   evidence-checked one — your call.*

2. **★ Extraction unit — transcript-grounded episode extraction replaces the skeleton labeler as the
   primary path.** Skeleton mining stays as an optional high-precision *hint* fed into the prompt
   (the mined tool-arc is good signal for failure→fix procedures), but the LLM now reads the actual
   episode (user/assistant dialogue + tool calls + results) and may emit 0..N skills of any of the
   10 types. Non-failure episodes stop being dropped as ProseFallback.

3. **Contrast is built into the cross-episode pass.** The synthesis pass becomes a success/failure
   contrast pass (it sees which episodes converged and which dead-ended) and emits principles +
   best-practices + the converged form of multi-episode refinement arcs.

4. **Preferences become LLM-distilled, not keyword-matched.** A dedicated preference-distillation
   prompt reads user turns + assistant acknowledgements and emits preference skills with the *why*.
   The deterministic keyword pass is kept only as a cheap recall floor / pre-filter.

5. **Per-episode output stays small and high-precision** (cap 3 skills/episode, single-purpose, no
   redundancy). Generality is corroborated later by cross-session frequency, not asserted per-episode.

6. **Provider target = `claude-code` (frontier)** for the quality corpus (plan: 0.68 vs local Gemma
   0.256 non-empty-procedure rate). Prompts are written to be robust on local models too (strict
   JSON-only, `think:false`, bounded counts) but we do not gate quality on the local path.

7. **Index on summary/trigger, keep the body detailed** (ℓ₁ = summary). Unchanged; the prompts emit
   detailed bodies but retrieval embeds the high-signal views.

---

## 5. The prompts

Notation: `{{...}}` are template substitutions filled by the Rust builder. Every prompt ends with a
strict JSON-only instruction. The episode transcript is pre-bounded to the chunk window (existing
segmentation) so the length-cliff guidance is respected.

### 5.1 Episode skill extraction prompt (core — replaces `build_skeleton_labeling_prompt`)

```
You are a senior engineer distilling DURABLE, REUSABLE engineering knowledge from one episode of a
real coding session, so a future agent can apply it to a NEW task without seeing this session.

You are not summarizing what happened. You are extracting transferable skills: the kind of thing a
staff engineer would write down once and reuse for years. Capture not just explicit solutions, but
also: rules and heuristics learned, anti-patterns to avoid, the CONVERGED result of trial-and-error
(and the dead-ends that were ruled out), prerequisites discovered, best practices, and reusable
diagnostic strategies.

## Project context
{{preamble}}            # project facts (repo, language, key paths) + standing user preferences

## Episode transcript (verbatim — your only source of truth)
{{episode_transcript}} # user turns, assistant turns, tool calls, tool results, in order

## Mined tool arc (a hint — may be empty; verify against the transcript, do not trust blindly)
{{skeleton_hint}}

## What to extract
Identify 0 to 3 distinct, single-purpose skills. For EACH skill classify its `type` as one of:
  procedure | rule | anti_pattern | failure_fix | prerequisite | preference | best_practice |
  principle | refinement | diagnostic

For each skill, fill these views so a future agent both NOTICES it and APPLIES it correctly:

- name:        kebab-case, specific, <= 6 words. One capability per skill.
- description: one declarative sentence: what it accomplishes and the rule it encodes.
- use_when:    1-4 SHORT triggers using the LITERAL tokens a future task/error will contain
               (e.g. "Ollama structured call returns malformed JSON on large inputs", NOT
               "LLM problems"). This is what makes the skill get noticed — be concrete.
- avoid_when:  0-4 situations where applying this is WRONG, AND the tempting-but-wrong moves that
               were tried and failed in THIS episode (the dead-ends). This is high-value — mine it.
- procedures:  the ordered, executable steps of the CONVERGED solution. Abstract repo-specific
               literals into {placeholders} (paths, ids, values), but keep them runnable. If the
               episode was pure trial-and-error, the procedure is the FINAL approach that worked,
               not the wandering.
- invariants:  the explicit rule(s)/constraint(s) that must hold for correctness, stated
               declaratively ("X must happen before Y", "budget must equal the real model context").
- requires:    prerequisites assumed in place before the procedure can succeed.
- produces:    the named outcome/artifact a future agent should expect if it works (verifiable).
- tools:       commands / libraries / frameworks / services / models / APIs the skill invokes.
- artifacts:   file types / configs / protocols / repo objects the skill applies to.
- generality:  "general" (transfers across projects), "project" (specific to this codebase), or
               "uncertain". Be conservative: only "general" if it would help a different repo.
- evidence:    1-3 concrete anchors copied from the transcript that prove this skill is real — the
               exact command, error string, or file that it was derived from. Used for grounding;
               do not invent anchors.

## Rules (follow exactly)
- Extract the LESSON, not the log. If nothing here is durable and reusable, return an empty list.
  An honest empty result is correct and expected; do NOT manufacture filler skills.
- One capability per skill. Do not merge two unrelated lessons; do not emit overlapping skills.
- Prefer the failure->fix and the dead-ends-avoided: those transfer best.
- Every field must be supported by the transcript. If you cannot ground a field, leave it empty
  rather than guessing.
- Keep each skill single-purpose and dense. Put the most load-bearing trigger/rule first.

## Output (STRICT — JSON only, no prose, no markdown fences)
{"skills": [
  {"name":"...","type":"...","description":"...",
   "use_when":["..."],"avoid_when":["..."],"procedures":["..."],
   "invariants":["..."],"requires":["..."],"produces":["..."],
   "tools":["..."],"artifacts":["..."],
   "generality":"general|project|uncertain","confidence":0.0,
   "evidence":["..."]}
]}
If the episode contains no durable reusable knowledge, return exactly: {"skills": []}
```

### 5.2 Cross-episode contrast & synthesis prompt (replaces `build_synthesis_prompt`)

```
You are reviewing the skills extracted from ALL episodes of one coding session, plus a note of which
episodes CONVERGED (succeeded) and which DEAD-ENDED (failed or were abandoned). Your job is to
surface SESSION-SPANNING knowledge that no single episode captured: cross-cutting principles, best
practices that recurred across multiple episodes, and the converged result of refinement arcs that
spanned several episodes.

## Project context
{{preamble}}

## Per-episode skills already extracted (do NOT re-emit these)
{{episode_skills_summary}}     # name + one-line description + type, numbered

## Outcome map (which episodes converged vs dead-ended)
{{episode_outcomes}}

## What to extract
Emit 0 to 3 NEW skills that are MORE GENERAL than any single episode skill above. Favor:
  - principle:     a high-altitude invariant that explains WHY several episode skills worked.
  - best_practice: a positive pattern that recurred across >= 2 distinct episodes (cite which).
  - refinement:    the converged best approach distilled from a trial-and-error arc that spanned
                   multiple episodes — state the final approach AND the dead-ends it rules out.
  - anti_pattern:  a failure mode that showed up in more than one episode.

Use the SAME schema and the SAME strict output format as the episode extractor (name, type,
description, use_when, avoid_when, procedures, invariants, requires, produces, tools, artifacts,
generality, confidence, evidence).

## Rules
- Only emit knowledge NOT already represented in the per-episode skills. No rephrasing of existing
  skills; no overlap.
- Each new skill must cite (in `evidence`) the episode numbers it generalizes from. A best_practice
  or principle MUST be corroborated by >= 2 episodes; if it only appears once, do not emit it.
- If no genuine session-spanning pattern exists, return {"skills": []}.

## Output (STRICT — JSON only)
{"skills": [ ... same object shape as the episode extractor ... ]}
```

### 5.3 Preference distillation prompt (replaces keyword matching in the preamble)

```
You are distilling STANDING user/project PREFERENCES from a coding session — the non-correctness
choices the user consistently wants honored (style, tooling, workflow, what to avoid, how to
communicate). These become durable convention-skills applied to every future session.

## User and assistant turns (in order)
{{dialogue_turns}}

## What to extract
Emit 0 to 5 preferences. For each, capture the preference AND its rationale (the WHY), because a
preference with a reason transfers and survives edge cases; a bare directive does not.

- statement:  the preference as a clear imperative ("Execution sub-agents always run on Sonnet").
- why:        the reason, if the user gave one (else empty). Do not invent a reason.
- scope:      "general" (any project) or "project" (this codebase/context only). Use "project" when
              the preference names a specific repo, tool, path, or local convention.
- strength:   "hard" (an absolute rule the user insisted on) or "soft" (a leaning/default).
- avoid_when: 0-2 situations where the preference explicitly does NOT apply, if the user said so.
- evidence:   1-2 exact quotes from the user turns that state the preference.

## Rules
- Only durable STANDING preferences (things the user wants every time), not one-off task
  instructions specific to this session's goal.
- Ground every preference in an actual user statement (evidence quote required). No invention.
- If the user stated no standing preferences, return {"preferences": []}.

## Output (STRICT — JSON only)
{"preferences": [
  {"statement":"...","why":"...","scope":"general|project","strength":"hard|soft",
   "avoid_when":["..."],"evidence":["..."]}
]}
```

---

## 6. Output schemas (Rust wire types)

The episode + synthesis prompts share one wire shape (superset of the current `SynthesisCandidate`):

```
struct ExtractedSkillWire {
  name: String, type: String, description: String,
  use_when: Vec<String>, avoid_when: Vec<String>, procedures: Vec<String>,
  invariants: Vec<String>, requires: Vec<String>, produces: Vec<String>,
  tools: Vec<String>, artifacts: Vec<String>,
  generality: Option<String>, confidence: f32, evidence: Vec<String>,
}   // every list #[serde(default)]; name+description required (local-model truncation tolerance)
```

Maps directly onto the existing `ExtractedSkillCandidate` (which already has all these fields except
`type` and `evidence`). **Two new fields needed:** `type` (taxonomy tag) and `evidence` (grounding
anchors → written to the `## Evidence` body section). `conventions`/`assets` remain for preferences.

The preference prompt's wire shape maps onto the existing preference→candidate path (`conventions` =
[statement], `generality` = scope, plus `why`/`evidence` folded into the body).

---

## 7. Grounding & anti-hallucination (fail-loud, per the no-fakes mandate)

Because §4.1 lets the LLM author abstracted procedures, we add a **post-parse grounding validator**
(pure, unit-testable) run before any candidate reaches `.pending`:

1. **Evidence required.** A candidate with empty `evidence` is rejected (no anchor = ungrounded).
2. **Anchor existence.** Each evidence anchor must be a substring (normalized) of the source
   transcript. If a cited command/error is absent from the transcript, reject the candidate and log
   the offending anchor (this is the fabrication guard — fail loud, do not silently keep).
3. **Payload check** (existing): a candidate with a name but zero usable payload (no procedures /
   conventions / use_when / invariants) is rejected as a content-free shell.

This keeps the honesty bar: a skill is admitted only if it is grounded in something that actually
happened in the session. Rejections are surfaced, not swallowed.

---

## 8. What this does NOT change

- The `.pending` → human-approve gate (unchanged; every extracted skill still needs the rename gate).
- The SKILL.md on-disk format / writer / reader (already supports all 7 fields; we add the
  `## Evidence` body section, which is already a recommended section in the plan).
- Retrieval / scoring / eq.3 (unchanged).
- No new DB migration (fields already exist as of migration 009).

---

## 9. Code changes required (deferred until approval)

1. `crates/session-extractor/src/seams.rs` — replace `build_skeleton_labeling_prompt` →
   `build_episode_skill_extraction_prompt`; rewrite `build_synthesis_prompt`; add
   `build_preference_distillation_prompt`. Update `parse_*` to the new wire shape + `type`/`evidence`.
2. `crates/domain/src/types.rs` — add `skill_type: Option<String>` and `evidence: Vec<String>` to
   `ExtractedSkillCandidate` (both `#[serde(default)]`).
3. `crates/session-extractor/src/skeleton.rs` — `map_episode` feeds the skeleton as a *hint* into
   the new extraction prompt instead of being the sole source; non-failure episodes are no longer
   dropped.
4. `crates/session-extractor/src/writer.rs` — write the `## Evidence` body section; map `type` to a
   tag.
5. New grounding validator module (§7) + unit tests.
6. Update the prompt-content unit tests (`synthesis_prompt_includes_candidate_names`,
   `skeleton_labeling_prompt_includes_trigger_and_steps`, `normalization_prompt_contains_*`).
7. `docs/reference/skill-md-format.md` — document `## Evidence` + the `type` tag.

---

## 10. Open questions for review

- **Q1 (★ §4.1):** OK to move from verbatim-only procedures to abstracted-but-evidence-grounded,
  backed by the fail-loud grounding validator? Or keep the hard skeleton invariant and only enrich
  fields the LLM can derive without authoring procedures?
- **Q2 (★ §4.2):** OK to make transcript-grounded extraction the primary per-episode path (skeleton
  becomes a hint), so non-failure episodes produce skills?
- **Q3:** Cap of 3 skills/episode + 3 synthesis skills — too tight, about right, or should it scale
  with episode size?
- **Q4:** Add the `type` taxonomy tag to the schema (enables type-aware retrieval/edges later), or
  keep it out of the persisted model for now and infer from fields?
- **Q5:** Should preferences with a stated `why` also seed a typed edge / be weighted higher in
  retrieval, or stay as plain convention-skills for now?

---

## 11. References

CL-bench arXiv:2602.03587 · ReasoningBank arXiv:2509.25140 · Trace2Skill arXiv:2603.25158 ·
SkillRevise arXiv:2606.01139 · ACE (Agentic Context Engineering) arXiv:2510.04618 · Structurally
Aligned Subtask Memory arXiv:2602.21611 · ExpeL arXiv:2308.10144 · AWM arXiv:2409.07429 · Voyager
arXiv:2305.16291 · Reflexion arXiv:2303.11366 · Generative Agents arXiv:2304.03442 · A-MEM
arXiv:2502.12110 · AutoRefine arXiv:2601.22758 · "Where LLM Agents Fail" arXiv:2509.25370 · Memory
survey arXiv:2512.13564 · Foundation-agent memory survey arXiv:2602.06052.
