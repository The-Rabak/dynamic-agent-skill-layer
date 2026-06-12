# T14 CL Acquisition Band — task selection + full protocol (2026-06-12)

**What this is.** The owner-directed plan for the T14 acquisition band: 10 CL-bench tasks (2 smoke +
8 full), each run through the **teach-session protocol** (real session → real extraction pipeline →
fidelity gate → held-out paired solve). It also doubles as the dress rehearsal for running the full
CL-bench with the skill layer later (§8) and for DS-030's compounding curve.

**Why CL-bench tasks.** The T14 smoke (2026-06-12) proved our self-authored invented rules were
within Sonnet's default competence — OFF won everything. CL-bench (arXiv:2602.03587, Tencent
Hunyuan + Fudan NLP, published 2026-02 — past every deployed model's training cutoff) is 1,899
expert-authored tasks over 500 contexts whose knowledge is absent from pretraining; ten frontier
models average **17.2% solve WITH the context provided** (best: GPT-5.1, 23.7%). Our OFF arm
removes the context entirely, so OFF-failure is near-guaranteed for invented-knowledge contexts —
which flips the binding constraint: we select for tasks that are **ON-winnable** (mechanically
solvable from a distilled rule summary) on top of OFF-hard.

**Provenance fence (unchanged).** No rule is ever hand-planted into the corpus. The knowledge
travels: context document → genuine claude-code working session → real extraction → `.pending` →
human gate → corpus. That is the loop T14 measures; CL-bench only supplies the teaching material.

---

## 1. Source, pinning, reproduction

- Dataset: `tencent/CL-bench` (HF), **pinned sha `b28a5832a09b0d96c0cf4c22e90d7c60ede25b80`**
  (2026-02-06). Paper: arXiv:2602.03587. License: Tencent Hunyuan & Fudan NLP; local evaluation use;
  **contexts are NOT committed to the repo** — `scripts/fetch_clband_contexts.py` re-materializes
  them on demand (verifies the pin, verifies every sentinel, fails loud; tested 2026-06-12, 13/13
  contexts green).
- Manifest: `tests/e2e/efficacy/clband/manifest.json` (selection + verified sentinels + eval scores).
- **Expiry rule:** the bench's non-pretrained property holds for current model checkpoints and
  decays with future ones. The solver checkpoint + bench sha are recorded in every run report; any
  solver change re-runs the OFF pre-gate (§4 step 0) before results are comparable.

## 2. How the 10 were selected (method, so it can be re-run)

Funnel: 1,899 tasks → group by `context_id` (500 contexts) → keep invented-knowledge subcategories
(Rule System: Game Mechanics / Programming Syntax / Math Formalism / Tech Standards / Legal & Reg;
Procedural: Operational / Instructional / Workflow Orchestration — Domain-Knowledge subcats excluded
because their knowledge is real-world and potentially memorized) → ≥3 sibling tasks + context
3k–70k chars → **136 candidates** → ranked by a rubric-determinism heuristic → top 48 scored by
**6 parallel evaluators** on six axes: `invented`, `off_fails`, `on_winnable`, `extractable`,
`det_verifiable`, `sibling_quality` (1–5 each), plus real-world-artifact and context-referential
flags and candidate sentinels.

Selection from the scores, with two human-override exclusions the evaluators got wrong or we
exclude by design:
- **Molly House** (scored 26, flagged invented) — actually a REAL published board game (Wehrlegig
  2024). Excluded: OFF-arm purity.
- **Emoji base-13** (scored 28) — sentinels are emoji; extraction/embedding garbling would confound
  the fidelity gate. Excluded by design.
- Every surviving sentinel was **mechanically verified** to appear in the shared context text (two
  evaluator sentinels were hallucinated from rubrics — `M-WARN-01`, `LMI-2025` — and replaced with
  verified strings). The fetch script re-verifies on every run.

## 3. The band

| # | role | name | sub-category | tasks (held-out-capable) | knowledge | size | score |
|---|---|---|---|---|---|---|---|
| 1 | smoke | flywheel-assembly-agent | Operational Proc. | 12 (12) | system | 4.2k | 29 |
| 2 | smoke | aether-language | Programming Syntax | 3 (2) | user | 33.5k | 29 |
| 3 | full | material-handler-sops | Operational Proc. | 7 (7) | system | 13.3k | 30 |
| 4 | full | source-integrity-agent | Workflow Orch. | 7 (7) | system | 10.1k | 29 |
| 5 | full | quartermaster-hold-inventory | Math Formalism | 10 (10) | system | 3.6k | 29 |
| 6 | full | dpms-agent-m | Operational Proc. | 4 (4) | system | 6.0k | 29 |
| 7 | full | dartman-game | Game Mechanics | 4 (3) | user | 39.0k | 27 |
| 8 | full | ezlang-language | Programming Syntax | 3 (2) | user | 33.0k | 27 |
| 9 | full | drywave-3000-manual | Technical Standards | 3 (2) | user | 47.2k | 27 |
| 10 | full | 123corp-hr-policy | Legal & Regulatory | 3 (2) | user | 16.0k | 26 |
| A1 | alternate | shelbys-recipe-assistant | Instructional Proc. | 3 (2) | user | 35.6k | 26 |
| A2 | alternate | agent04-podcast-orchestrator | Workflow Orch. | 6 (6) | system | 4.0k | 26 |
| A3 | alternate | micro-moonshine-game | Game Mechanics | 3 (2) | user | 19.0k | 26 |

Deliberate composition: context sizes span **3.6k → 47.2k chars** (the extraction-fidelity
envelope); both knowledge placements (system-prompt SOPs vs user-document specs); six flat-sibling
contexts (clean independent held-outs) + four nested multi-turn (deepest: dartman at depth 8);
DS-band coverage — one-shot rule (all), procedural fidelity (flywheel, material-handler, dpms),
compositional pipeline (source-integrity ≈ DS-028), negative-transfer pressure (ezlang/dartman,
where strong priors actively contradict the invented rules ≈ DS-029). The two smoke picks
intentionally bracket the size range: smallest context with most siblings (flywheel) + a large
invented language (aether), so the smoke proves the pipeline at both extremes.

**Smoke = #1–2. Full = #3–10. Alternates substitute, in order, for any context whose siblings all
fail the OFF pre-gate** (A3 is reality-unverified — use only if the pre-gate confirms
discrimination).

## 4. Per-task protocol (the full band mechanics)

Each context runs this lifecycle. One **skill-layer project scope per context**
(`clband-<name>`) — isolating corpora per rule-system, enabling the cross-scope placebo, the
per-scope fidelity gate, and later DS-030 accumulation.

**Step 0 — OFF pre-gate (before anything is taught).** Every candidate Session B sibling is solved
by the bare agent (no layer, no context) against the compiled verifier. Any sibling OFF passes is
**rejected as non-discriminating**; a context losing all siblings is swapped for the next alternate.
This empirically re-proves non-pretraining per task (we never rely on the cutoff assumption) and is
the pre-committed task-selection rule from the T14 pre-registration amendment.

**Step 1 — Session A (teach).** A genuine claude-code working session in a fresh workspace
containing the knowledge document (`tests/e2e/efficacy/clband/contexts/<name>/{system,context}.md`,
merged per `knowledge_home`). The session prompt = the **teach task** (the shallowest sibling; for
nested contexts whose depth-2 turn fuses context+question, that fused turn verbatim — the fetch
script marks these `teach_only`). The agent actually works the task — reads the document, reasons,
answers. Not a paste-and-quit; the transcript must show the rules being used. Hooks capture the
session under the context's scope.

**Step 2 — Pipeline.** Real extraction on the captured session (`EXTRACT_SESSION_PROVIDER=
claude-code`, the proven richer extractor; an optional local-provider arm on the same transcripts
is a free measurement of the 2.6× density gap) → `.pending` drafts → **gate** → corpus +
rebuild for the scope. The drafts also feed T14's ≥10-real-drafts acceptance criterion.

> **Gate mode (T23 automated band run, amended 2026-06-12 BEFORE any band datum):** for the
> unattended overnight band the human gate is replaced by `gate_mode=auto-accept-all`, **`clband-*`
> benchmark scopes ONLY** — every `.pending` draft in the context's `clband-<name>` scope is accepted
> via the REAL acceptance action (rename `SKILL.md.pending` → `SKILL.md`) + real scope rebuild, behind
> a hard `clband-*` scope-guard assertion (fail loud on any non-clband path). The production human
> gate and the 262 dogfood corpus are UNCHANGED. See the LOCKED "CL Acquisition-Band AUTO-GATE
> Amendment" in `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/14-efficacy-task-outcome-ab-harness.md`
> for the full pre-registration (gate policy, accept-all rationale, unattended continue/stop policy,
> `gate_mode` recorded verbatim). The smoke (plan §5) used the human gate; the band uses auto-accept-all.

**Step 3 — Fidelity gate (deterministic, before any measured solve).** All manifest sentinels for
the context must appear across the scope's accepted skill texts (`grep`-level check, committed
script). Optionally per-sibling operative constants (e.g. quartermaster's `500/100/150%`) recorded
at verifier-authoring time. **Failure = INSTRUMENT-FAILURE(extraction)** — a P0 extraction finding,
reported as such; that context produces no efficacy data point and no "layer doesn't help" reading.
This is the stage DS-025–030 skip (they seed directly); we are the first design to gate on it.

**Step 4 — Session B (measure), paired arms.** For each surviving held-out sibling:
- **Question de-referencing (pre-registered rewrite rule):** replace references to "the attached
  document / the paper above" with the named system ("the Aether language", "the DPMS reporting
  rules"). Never add content; rewrites are committed alongside the verifier before any run.
- **Nested siblings:** prior reference answers CONTAIN rule content (leakage). Rule: measure
  without `prior_turns` when the de-referenced question is self-contained; if it intrinsically
  depends on prior turns, include them **identically in all arms** and let the OFF pre-gate (which
  runs in the same configuration) adjudicate whether discrimination survives. Flat siblings have no
  such issue and are preferred.
- **Arms:** `ON` = real mcp-server injection from the context's scope (focused inject-query mode
  initially, per the smoke's Finding 2; injection mode is recorded in the report; switch to the
  production `compile_context` path once T18/T12 fix verbose-prompt priming and re-label) / `OFF` =
  bare agent / `PLACEBO` = injection from a **different** context's scope at matched token mass
  (e.g. dartman skills for an EZLang question) — the explicitly-labeled control, on the real stack.
- **Attribution:** every pull logged (skill ids, scores, injected mass); ON failing a sibling whose
  skills were verifiably injected = **INSTRUMENT-FAILURE(injection/obedience)**, distinct from
  Step 3's extraction failure. The three-way ambiguity (extraction / retrieval / obedience) is
  resolved by construction: Step 3 gates extraction, attribution logs retrieval, what remains is
  obedience — a real efficacy signal.

**Step 5 — Verifier.** Per sibling, CL-bench rubrics are compiled BEFORE any measured run into:
(a) a **deterministic core** — exact strings, numbers, orderings, structural musts (the band was
selected for rubric determinism; a sibling needs ≥5 deterministic checks to be measurement-eligible)
— this decides task pass/fail; (b) the **full-rubric judge score** — committed claude-CLI judge
prompt with the verbatim rubrics, CL-bench's native all-rubrics-must-pass scoring — reported
secondary, and the bridge to full-benchmark comparability (§8). Reference assistant answers embedded
in nested contexts are verifier-authoring material, never shown to any arm.

**Scoring.** One pass/fail per (context, sibling) per arm; the context's measured sibling
contributes one paired data point to T14's pre-registered **"ON wins ≥7/10, sign test, no
catastrophic regression"** criterion alongside the organic band. Pre-registered secondaries:
paired turns-to-solve, token cost, judge-rubric score, and ON-vs-PLACEBO stated explicitly.

## 5. Smoke segment (2 contexts — run first, gates the full band)

`flywheel-assembly-agent` + `aether-language`, full lifecycle (Steps 0–5), 1–2 siblings each.
**Must prove:** (1) a teach session captures and the pipeline extracts genuinely novel rules —
fidelity gate green at BOTH size extremes (4.2k system-prompt SOP and 33.5k user-document language
spec); (2) the OFF pre-gate craters where the smoke's earlier battery didn't; (3) scope-isolated
injection + cross-scope placebo work end-to-end. **Explicitly not efficacy data** — outcomes are
pipeline-validation findings; the pre-registration's verdict machinery stays untouched until the
full band runs. If the fidelity gate fails at 33.5k but passes at 4.2k, that is a P0 *extraction*
finding with a size threshold attached — exactly the kind of result worth knowing before 8 more.

## 6. Pre-registration deltas (fold into T14 BEFORE the full run)

1. The band roster (§3) and the alternate-substitution order are fixed; substitution happens only
   via the OFF pre-gate, never after paired data exists.
2. Verifiers, question rewrites, and judge prompts committed per sibling before its measured run.
3. The INSTRUMENT-FAILURE taxonomy (extraction vs injection/obedience) and its no-verdict rules.
4. Solver checkpoint + dataset sha recorded; solver change voids OFF pre-gate results.
5. Injection mode (focused-query vs production compile_context) labeled per run.

## 7. Cost envelope

10 teach sessions + 10 extraction/gate cycles + OFF pre-gate over ~20 candidate siblings + (8
contexts × ~1–2 siblings × 3 arms) ≈ **60–80 claude-code solves + 10 extractions**, comparable to
the original ≥10-task × 3-arm plan, sequenced so the cheap gates (Steps 0, 3) kill doomed work
before the expensive paired runs. All runs serialized by the orchestrator (standing rule).

## 8. Scaling to the full benchmark (the later prize)

The same machinery generalizes with three changes, none architectural: (1) iterate the manifest
over all 500 contexts (the fetch script + selection pipeline already handle both knowledge
placements and nested turns; Domain-Knowledge contexts rejoin since full-bench mode doesn't need
the OFF arm's purity — CL-bench's own WITH-context framing applies); (2) scoring flips to CL-bench
native (judge, all-rubrics) as primary with our deterministic cores as the audit subset; (3) the
run becomes **layer-mediated CL-bench**: Session A on context k, then siblings answered from
extracted skills — reported next to the published WITH-context frontier numbers as "context
distilled to skills" vs their "context in window". DS-030's compounding curve falls out of
accumulating scopes and re-measuring earlier contexts' siblings as the corpus grows. That run is
gated on the full band landing first; this plan's §4 protocol is its unit of execution.

## 9. Risks

| risk | mitigation |
|---|---|
| Extraction garbles large contexts (2.6× density gap) | Fidelity gate (Step 3) converts it into a measured P0 finding, not a confound; smoke brackets the size envelope first |
| Verbose Session B prompts hit the no_match bug (Finding 2) | Focused inject-query mode, labeled; production path re-test after T18/T12 |
| A "fictional" context is actually real (Molly House class) | OFF pre-gate is the empirical backstop for every task; A3 explicitly flagged |
| Nested-sibling prior turns leak rules | Pre-registered handling (§4 Step 4); flat siblings preferred — 6 of 10 contexts are flat |
| Judge-rubric subjectivity | Deterministic core decides pass/fail; judge score is secondary |
| Dataset drift upstream | Pinned sha; fetch script fails loud on drift |
| Persona/format rubrics measure prompt-following, not knowledge | Verifier authoring drops persona-only rubrics from the deterministic core; knowledge rubrics (values, codes, orderings) dominate by selection |
