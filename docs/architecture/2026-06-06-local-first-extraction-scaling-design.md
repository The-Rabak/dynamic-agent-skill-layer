---
date: 2026-06-06
status: proposed
deciders:
  - repository-owner (rabak)
related_todos: ["183", "184", "185", "186", "187", "188", "189", "190", "191"]
supersedes: []
related_docs:
  - docs/architecture/adr-0002-claude-code-cli-extraction-provider-v1-5.md
---

# Design: Local-first extraction scaling — segment, mine, reduce

## Context

A 2026-06-06 end-to-end investigation (capturing the real `gemma4:12b` reasoning traces on the
production extraction prompt) found that skill extraction is failing for reasons that are **not** the
model's fault, and that the current single-shot "feed the whole transcript to one LLM call" strategy
will not survive real production sessions on small local models. Three distinct problems were proven:

1. **The pipeline feeds the model the wrong half of the conversation.** `sanitize_transcript_entry`
   (`crates/infrastructure/src/extraction/prompt_contract.rs`) lists `"assistant"` in
   `SUSPICIOUS_SPEAKERS` and drops every assistant-role turn before the prompt is built. The
   `<transcript>` block that reaches the model contains only user turns. The thinking trace shows the
   model correctly concluding "there is no answer in this transcript" and refusing to emit. Smaller
   models (granite4:3b, gemma4:e4b) masked the bug by **hallucinating** a candidate, so the e2e test
   was green for the wrong reason. (Fixed by **#183**.)

2. **The prompt contract is internally contradictory about what a skill is.** `extraction_targets`
   includes user guidelines and best practices, the schema has a `generality: general|project|uncertain`
   field built to *carry* general skills — yet the prompt's CRITICAL RULES say "Only extract skills
   that encode concrete, **project-specific** knowledge," and the quality weighting
   (`failure_mechanism_encoding` 0.30, `actionable_specificity` 0.25) starves pure user-preference
   skills. Skills come from **every speaker and every generality class** — a user-stated preference
   ("never add comments unless asked"), a general engineering heuristic ("run components in isolation
   to unmask a systematic failure"), or a project convention are all first-class. (Fixed by **#183**.)

3. **Single-shot extraction does not scale to real sessions on local models.** A production Claude
   Code session is hundreds of structured turns (often 100k+ tokens). A small local model
   (`gemma4:e4b`, `granite4:3b`, 8–32k context) cannot hold it, and even when it fits, long-context
   reasoning is the small model's weakest axis. `gemma4:12b` already runs ~140s/call on the reference
   RTX 2060 (Q4_K_M, partial CPU offload) — at/over the worker timeout — and degrades in quality
   (garbled tokens, hallucinated generic advice). Naive token-chunking would "fix" the size problem by
   destroying the context that makes a skill worth keeping. This document specifies the scaling
   strategy. (Implemented by **#184–#189**; runtime by **#190**; frontier path by **#191**.)

The frontier/headless path (`claude -p`, Claude API) sidesteps the size and reasoning limits (200k+
context, stronger reasoning) and is what most production installs will actually run — but it is
**completely untested** today, and local-first is a stated product value. This design makes the local
path genuinely viable and routes to frontier when the session warrants it (**#191**).

## Goals / non-goals

**Goals**
- Extract reusable skills from real, long sessions without losing the three classes of cross-turn
  context: (a) globally-stated preferences/facts, (b) problem→resolution arcs, (c) far-apart
  setup/payoff.
- Be **one provider-agnostic pipeline**, not two divergent strategies. The deterministic spine (event
  parse, preamble, skeleton mining, reduce/dedup, synthesis) runs for every provider; only the
  *segmentation granularity* varies, driven by the target model's context budget (see § "Provider-
  agnostic by default").
- Play to the small model's strengths (small, bounded transforms) and avoid its weakness
  (long-context reasoning) — while letting frontier models keep their strength (holistic cross-arc
  reasoning) rather than forcing our boundaries on them.
- Reuse the maintenance merge-verifier + dedup machinery already shipped, rather than building new
  cross-candidate reconciliation.
- Keep background cost roughly flat as session length grows.

**Non-goals**
- Replacing the human gate. Everything still lands as a `.pending` draft behind rename-to-approve.
- Changing the read/retrieval side (covered by the separate retrieval-quality tickets #192/#193).
- A frontier-only design — frontier is a routing target, not the floor. **Equally, this is NOT a
  local-only design:** segmentation is context-budget windowing, not a small-model-only tax. The
  grounding/coverage/cost wins of the spine apply to frontier too.

## Why naive chunking loses what matters

Three kinds of valuable context die under blind token-window splitting:

1. **Global facts stated once** — "we use `tokio::sync` everywhere," "never add comments" — said at
   turn 3, relevant at turn 90. A middle chunk never sees them.
2. **Problem→resolution arcs** — an error appears, is diagnosed across many turns, and resolves. Split
   the arc and no chunk holds problem + remedy together, so no chunk can encode the skill.
3. **Far-apart setup/payoff** — a convention established early, applied late.

The design defends each: arc-aware segmentation (never cut mid-arc), a persistent session preamble
(global facts ride into every chunk), and a reduce/merge step (reassemble cross-chunk skills).

## The core insight: the transcript is a structured event log, not prose

A Claude Code session is a typed event stream — user/assistant turns, `tool_call(name, input)`,
`tool_result(exit_code, stderr)`, `file_edit(path, diff)`. That structure is **free signal**. We do
not need an LLM to find "an error got fixed": a `Bash` exit≠0 → edits → the same `Bash` exit=0 *is*
the arc, deterministically. The current ingestion throws this away — `domain::TranscriptEntry` is
`{ speaker: String, content: String }`, a lossy flattening. Recovering the structure (**#184**) is the
foundation the rest stands on.

## Architecture — five stages

```
 raw transcript (.jsonl / inline)
        │
        ▼
 [#184] Structured event model ─────── typed events: turn / tool_call / tool_result / file_edit
        │
        ├──────────────► [#186] Session preamble (preferences + project facts), built once, static
        │                         carried verbatim into every episode prompt
        ▼
 [#185] Episodic segmentation ──────── arc-aware cuts (never split a problem→resolution span),
        │                              context-budget windowing w/ overlap as fallback
        │
        ├──────────────► [#189] Salience gate — skip low-skill-density episodes
        ▼
 [#188] Per-episode extraction ─────── deterministic procedure-SKELETON mining from the tool log,
        │  (map)                        LLM only NAMES / generalises / judges (bounded transform)
        ▼
 [#187] Reduce ──────────────────────  reuse merge-verifier + dedup to collapse cross-episode dupes
        │                              and stitch partial skills; then ONE synthesis pass over the
        │                              candidate LIST (small) for session-spanning patterns
        ▼
 .pending drafts (human gate, unchanged)
```

### Stage decisions (the recommendations adopted)

- **Static session preamble** (not rolling). Simpler, parallelisable map. A rolling preamble (updated
  per-episode to carry order-dependent context) is a documented upgrade path in **#186**, taken only
  if quality measurement demands it.
- **Skeleton-mine + LLM-label split** (not LLM-extracts-the-whole-episode). The procedure skeleton is
  mined deterministically from real tool output (commands, exit codes, edits); the LLM is reduced to
  "name this kebab-case, one-sentence description, judge generality, keep-or-drop." This is the single
  decision that makes small models viable AND structurally kills the hallucination failure mode — the
  steps come from the transcript, not the model's imagination. (**#188**.)
- **Salience gate on by default**, tuned for recall first. Cost stays ~flat as sessions grow.
  (**#189**.)
- **Reduce reuses existing machinery.** The maintenance LLM merge-verifier + semantic dedup already
  exist; the reduce stage calls them rather than inventing cross-candidate logic. (**#187**.)

## Provider-agnostic by default — granularity is a budget knob, not a branch

The same pipeline runs for `gemma4:e4b`, `gemma4:12b`, Claude Code, and the Claude API. What differs is
**segmentation granularity**, and it is driven by one parameter — the target model's context budget
(#185's `token_budget`) — not by a per-provider code branch.

- **The deterministic spine is universal.** Event parse (#184), preamble (#186), skeleton mining
  (#188), reduce/dedup + synthesis (#187), and the salience gate (#189) improve accuracy for *every*
  model, including frontier:
  - *Grounding* (skeleton from the real tool log) cuts hallucination for all models — frontier
    hallucinates less, not zero.
  - *Coverage* — a single giant pass satisfices (even Claude returns the 3–5 headline skills and drops
    the long tail); per-episode extraction + merge forces the long tail to surface. A memory system
    wants exhaustive recall, not a top-of-mind impression.
  - *Cost/latency* — salience-gating + sending only salient episodes is cheaper and parallelisable even
    on a 200k-context frontier model.
- **Granularity follows the window.** Set `token_budget` to the model's real context window.
  `gemma4:e4b` (8k) → many episodes; **Claude (200k) → a 100k session is a *single* episode** — the
  segmenter runs, sees it fits, emits one chunk. Holistic cross-arc reasoning is preserved with zero
  fragmentation, *and* the session still gets grounding + coverage + dedup. No special-casing.
- **Why we do NOT force fine segmentation on frontier.** Aggressive cuts sever the one thing frontier
  is best at — reasoning across distant turns (a skill emerging from turn 5 + turn 80 + turn 150).
  Boundary errors become a quality ceiling; over-fragmentation yields shallow skills; the model already
  segments semantically better than our heuristic. So fine segmentation is a small-model *necessity*,
  not a universal good — budget-parameterisation gives each model exactly as much chunking as it needs
  and no more.
- **Frontier opt-in: dual-pass.** When the budget makes a session one chunk, a frontier provider may
  ALSO run a holistic whole-session pass alongside the structured per-episode/skeleton pass, then merge
  the two via the same reduce stage. The holistic pass catches cross-arc synthesis; the structured pass
  grounds procedures and catches the long tail. This is the only genuinely provider-specific behaviour,
  and it is opt-in (worth the extra call only where reasoning justifies it). Routing (**#191**) chooses
  *granularity and whether to dual-pass* — never *whether to use the pipeline at all*.

```
#183 (prompt: all speakers + all generality)        ← prerequisite, standalone, fix first
#184 (event model)  ← foundation
   ├── #185 (segmentation)  ── #189 (salience gate)
   ├── #186 (preamble)
   └── #188 (skeleton+label split)
            #187 (map→reduce orchestration + synthesis)  ← integrates 185+186(+188)
#190 (idle-watchdog runtime)   ← independent, unblocks long episodes from being killed
#191 (frontier parity + tiered routing) ← independent; consumes the same event model
```

`#183` and `#190` deliver value immediately and independently. `#184` is the hinge for the epic.
`#191` can proceed in parallel and is what production will lean on.

## Risks & mitigations

- **Over-segmentation fragments a skill** → arc-aware boundaries + overlap + the reduce/synthesis
  stage stitch fragments back. Measured by an extraction-quality fixture with a deliberately
  multi-arc session.
- **Salience gate drops a real skill** → start recall-biased; `log()` what was gated (never silent
  truncation); fixture asserts known skills survive the gate.
- **Deterministic skeleton misses non-tool skills** (pure discussion, preferences) → the LLM-label
  path still runs on episode prose for non-tool episodes; the split is "skeleton when tools exist,
  prose otherwise," not "tools only."
- **Local quality still below frontier** → that is expected and is why **#191** exists; the bar for
  the local path is "useful and honest," not "frontier-equal."

## Acceptance (epic-level)

- A 100k-token, multi-arc synthetic session extracts ≥N grounded candidates on `gemma4:e4b` within a
  bounded, configured concurrency, with **no** turn dropped by speaker role and **no** hard-timeout
  discard.
- Candidates include at least one user-preference / general-heuristic skill (not only project
  procedures), proving the #183 contract change end-to-end.
- Extraction-quality fixtures assert on **content fidelity** to the transcript (real tokens), not just
  "≥1 candidate," closing the "green for the wrong reason" gap.
