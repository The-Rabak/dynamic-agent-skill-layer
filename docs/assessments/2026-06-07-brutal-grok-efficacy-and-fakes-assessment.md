---
date: 2026-06-07
topic: brutal-grok-efficacy-fakes-and-retrieval
assessor: Claude (Opus 4.8)
status: complete
constitution_ref: docs/constitution.md
related_assessments:
  - docs/assessments/2026-06-02-skill-layer-v1-5-current-state-assessment.md
related_memory:
  - brutal-eval-2026-06-07-efficacy-and-fakes
tickets_filed: ["205","206","207","208","209","210","211","212","213","214","215","216","217","218"]
scope:
  branch: feat/v-1-5-1
  method: full crate trace (10 crates, ~44k LoC), live e2e log review, doc/code reconciliation
handoff:
  purpose: true
  assessment: true
  battle_plan: true
---

# Brutal Grok Assessment — Efficacy, Fakes, and Retrieval (2026-06-07)

## Verdict (one line)

**The most disciplined, honestly-engineered pre-release infrastructure I've reviewed in a while — with
almost no evidence that the thing it exists to do actually works.** The plumbing is excellent. The
"brain" (retrieval quality, extraction quality, the graph's effect on ranking) is mediocre-to-decorative
and unproven. We have built a beautiful nervous system and have not shown it makes the agent smarter.

The evidence budget has been spent proving **correctness**; almost none on proving **efficacy**. The next
chapter must shift to measurement.

---

## What is genuinely excellent (verified, not asserted)

- **No-stub discipline holds.** Real Ollama HTTP embeddings (`infrastructure/src/embeddings/ollama.rs:142`),
  durable PG queue with `FOR UPDATE SKIP LOCKED` + retry/dead-letter (`transcript_queue.rs:236,287`),
  Redis self-heal on `NOGROUP` (`streaming/redis.rs:249`), real HDBSCAN (`communities.rs:181`). The stub
  embedder that started the machine-wide rule did not recur.
- **CQRS hot-swap is correct.** `ArcSwap::rcu` swap with a torn-read concurrency test
  (`retrieval/orchestrator.rs:851`), ACK-after-reload so events replay on failure
  (`graph_refresh_subscriber.rs:135`), coalesced N→1 reloads.
- **The human gate is airtight.** Every mutation path writes only `.pending`/`.retired` with
  `create_new(true)`; nothing auto-applies.
- **Scoring formula matches spec** (`scoring.rs:32`) with real arithmetic tests.
- **Process honesty.** 113/114 todos `done`/`resolved` by frontmatter (only #197 truly open). Unbuilt
  dream-state tests are honestly `#[ignore]`'d "not fully implemented" rather than faked green.

---

## The brutal findings (each filed as a ticket)

| # | P | Finding | Evidence |
|---|---|---------|----------|
| 205 | P0 | **Efficacy unproven.** Everything proven is correctness; nothing is "the layer makes the agent better." No ON/OFF, no task-outcome metric, no draft-acceptance rate. | whole-repo absence |
| 206 | P0 | **Fakes survive in non-unit tests + soft silent-fallbacks in prod.** Needs a systematic sweep + CI guard, not a one-off. | see 207/211/212 |
| 207 | P1 | **Merge "e2e" drives two fakes** (`DeterministicEmbeddingService` + `AlwaysEquivalentVerifier`); chronically drifts vs threshold (#196/#202). The lone red test. | `test_maintenance_e2e.rs:128,146` |
| 208 | P1 | **SkillRAE graph is ranking-inert.** `community_boost` is binary 0.2 and every skill belongs to ≥1 community → uniform 1.05× → cancels out of every comparison. HDBSCAN is decorative in retrieval. | `mcp-server/src/lib.rs:1092`, `communities.rs` |
| 209 | P1 | **Floor calibrated on a toy corpus.** 0.450 from 8 skills / 20 queries, bimodal gap 0.0179. A coin on its edge; will collapse at real scale. | `retrieval/orchestrator.rs:156` |
| 210 | P1 | **Retrieval brain is mediocre.** Measured MRR/nDCG ≈ 0.625. No committed target, no closed tuning loop. | first measured baseline |
| 211 | P1 | **`#[serde(default)]` admits empty-procedure skills** into `.pending` — soft silent degradation. | `session-extractor/src/seams.rs:494-517` |
| 212 | P1 | **Writer goes permissive (writes anywhere) when `SKILL_GLOBAL_PATHS` unset.** Safety boundary disarms on missing config. | `session-extractor/src/writer.rs:103` |
| 213 | P2 | **Orchestration seam invariants are `.expect()`/panic, not type-encoded.** First-job panic where a compile-time guarantee is cheap. | `session-extractor/src/lib.rs:745-770` |
| 214 | P1 | **Default local extraction is the weakest path.** Good extraction needs cloud; the private default's quality is unproven (#176). Unstated tension. | #176 lineage |
| 215 | P2 | **Doc overclaim / drift.** README implies dream-state contracts are "proven" (18/25 ignored); stale `rescue.rs` 0.20-vs-0.450 comment; todo filenames lie vs frontmatter. | README, `compiler/src/rescue.rs` |

### The retrieval architecture finding (filed as 217)

A trace of the real entry points shows the system **conflates two distinct retrieval intents into one
similarity-to-prompt cosine path**, and carries a self-growth-killing cold-start bug:

- **Every lifecycle event is identical and query-driven.** SessionStart sends `{{initial_prompt}}` and runs
  the same cosine ranking as everything else (`hooks.example.json`, `tools/compile_context.rs`). There is
  no priming mode. At session start the prompt is thin, so cosine has nothing to bite on.
- **Recency is a ~0.03 tiebreaker** (`usage_prior` cap 0.15 × γ 0.20), and **`usage_count==0 → prior=0`**:
  a freshly-approved skill must win on cosine alone but cannot accrue usage until retrieved. **The newest
  knowledge is the hardest to surface** — the opposite of what a self-growing system needs.
- **Global "appropriateness" = cosine × 0.7** — weak and unprincipled.
- **Mid-session `find_skill(prompt, limit)` is real** (`protocol.rs:63`) and is the sharp, high-signal path.

**The reframe (217):** session-start is a **priming** problem (centrality + recency + a freshness slot),
task-time is a **similarity** problem (`find_skill` / prompt events), and global appropriateness is a
**recurrence** problem (cross-project, ties to #180). One cosine path serves none of them well.

### The flagship efficacy idea (filed as 218)

Run **SWE-bench Lite** as a compounding self-improvement experiment: baseline (layer off) → learn corpus →
re-run (layer on) → re-run with full analysis. This demonstrates the actual thesis (compounding) against a
credible external yardstick AND organically generates the #216 corpus. **Validity guards are the ticket:**
train/test split (prove transfer, not memorization), a control arm (difference-of-differences, not raw
climb), retrieval-source attribution (priming vs `find_skill`), commit metrics before running, run through
Claude Code with our hooks. Without the split + control arm it is a great demo and a worthless proof.

---

## The strategic doubt

The thesis — "auto-extracted procedural skills, retrieved and injected, make a coding agent measurably
better" — has zero supporting evidence. The risk isn't that the system doesn't work; it's that it works
perfectly and doesn't matter. Until a closed-loop efficacy measurement exists, "it works" means only
"it runs."

---

# FIX BATTLE PLAN

Ordered by dependency, not by ticket number. Each batch has a **goal**, an **inspection** (how to verify
it landed), an **expectation** (what good looks like), and a **fallback** (what to do if reality disagrees).

**Standing rule for every batch:** no fix is "done" until the full live suite is green with no new
`#[ignore]`, and no fake was introduced to get there. A red or skipped test is an honest result; a faked
green is a violation.

---

### Phase 0 — Green the tree & close the fail-loud holes  *(no corpus dependency; do first, in parallel)*

**Tickets:** 207 (real-embedder merge test), 211 (serde fail-loud), 212 (writer fail-loud), 213 (type-encode
seams), 215 (doc accuracy), 206 (audit + CI guard portion).

**Why first:** zero external dependencies, restores a green tree, and erects the no-fakes guard *before* the
big measurement work can smuggle a fake in.

**Inspect after the batch:**
- `cargo test --workspace` + the live e2e suite → **0 failures, 0 new ignores.**
- Plant a banned fake symbol in a `tests/e2e/` file → the **CI guard rejects it** (prove the guard works,
  don't just write it).
- The #206 audit table is committed: every fake/silent-fallback found, classified OK-unit / VIOLATION.
- Unit tests exist for: empty-procedure LLM response → loud failure (211); unset `SKILL_GLOBAL_PATHS` →
  boot fails loud (212); orchestrated path can't be constructed without seams (213).

**Expect:** the only previously-red test (merge cross-scope) goes green *via real embeddings*, not a
recalibrated fake. The audit surfaces a handful of additional soft fallbacks, not a swamp.

**If expectations aren't met:**
- If 207 still drifts with real embeddings → the merge threshold or body-inclusive vector is mis-set;
  that's a real retrieval bug, escalate to 208/210, do not lower the threshold to force green.
- If the audit surfaces many violations → stop, triage into child tickets with file:line, fix the P0/P1
  ones before proceeding; the no-fakes mandate gates everything downstream.

---

### Phase 1 — Cold-start fix + intent split  *(architecture; prerequisite for 218's measured runs)*

**Ticket:** 217 (build + unit-prove the cold-start freshness boost and the priming/task-retrieval split;
defer *quality validation* of the new signals to Phase 3).

**Why here:** on 218's layer-on run, freshly-created skills have `usage_count=0 → prior=0` and are
suppressed. Running the efficacy experiment before this fix would falsely show "no improvement."

**Inspect after the batch:**
- Unit test: a brand-new approved skill (usage_count 0) is retrieved for a clearly-relevant query, ranked
  above irrelevant skills. **This is the gate for Phase 4.**
- Unit test: a thin/empty SessionStart prompt surfaces project-baseline skills via the priming ranker.
- The retrieval contract doc is committed and matches the code.

**Expect:** the cold-start test flips from fail→pass once the freshness boost lands. Intent split is wired
but its *tuning* is intentionally not finalized yet (waits for the corpus).

**If expectations aren't met:**
- If the freshness boost over-surfaces noise (new bad skills crowd out good ones) → bound N harder and make
  the boost decay faster; re-validate in Phase 3 against the corpus, do not ship an unbounded boost.
- If priming can't be made cheap → fall back to "inject top-N most-central project skills, prompt-agnostic"
  as a v1 and revisit; do not let priming become a per-prompt full scan.

---

### Phase 2 — Build the real corpus  *(foundation for ALL measurement)*

**Tickets:** 216 corpus, generated by **218 run 1** (SWE-bench Lite, layer **off**, `claude-code` extraction).

**Why here:** 209/208/210/214/205/218-phase-2 all measure against a corpus that does not yet exist. This is
the critical-path unlock.

**Inspect after the batch:**
- ≥ **200 real, curated, actionable** skills in filesystem + PG + Qdrant with real HDBSCAN + tag
  communities, all produced through the real pipeline (no hand-authored shortcuts).
- The ingestion log: per-source yield, **draft-acceptance rate**, and a list of ingestion weaknesses found.
- The SWE-bench Lite **baseline score** (layer off) recorded.
- A named, reproducible corpus snapshot the downstream tickets reference.

**Expect:** real benchmark sessions yield a usable corpus, but with friction — expect a meaningful fraction
of low-value drafts rejected at the human gate. That rejection rate is itself a finding for 214.

**If expectations aren't met:**
- If yield is too low to reach 200, or acceptance rate is poor → that is a **critical extraction finding**,
  not a corpus problem. Route to 214 (and possibly 211): the ingestion pipeline isn't producing value from
  real sessions. Fix extraction quality before forcing a corpus; a corpus of junk poisons every downstream
  measurement.
- If `claude-code` extraction is impractical to run at benchmark scale → fall back to a curated transcript
  set (216 standalone path) with the same no-fakes invariant, and note the provider in the snapshot.

---

### Phase 3 — Calibrate, tune, and measure on the corpus  *(the "brain" work)*

**Tickets:** 209 (floor recalibration + labeled queries), 208 (graph-boost keep/cut decision), 210 (quality
target + tuning sweep), 214 (local-vs-cloud extraction gap), 217-validation (new-signal quality impact).

**Why here:** these all need the corpus and a held-out query set; they share the #210 measurement rig.

**Inspect after the batch:**
- 209: positive/negative score distributions on ≥200 skills, recomputed floor, **no_match precision/recall**
  (or ROC-AUC) on a held-out query set, with the real separation gap.
- 208: committed MRR/nDCG comparison of (binary boost) vs (new affinity/centrality boost) vs (λ=0). A
  **keep-or-cut decision** with numbers attached.
- 210: a committed quality target, a recorded tuning sweep showing each lever's delta on a held-out set, and
  measured quality meeting target.
- 214: measured local-vs-cloud extraction quality on real transcripts.

**Expect:** the 0.018 floor gap widens or the scalar floor proves inadequate (→ per-scope/normalized floor).
The graph boost likely shows little; be ready to cut HDBSCAN. Tuning moves MRR off 0.625 but maybe not to a
great number on the first sweep.

**If expectations aren't met:**
- If the scalar floor can't separate at scale → implement the justified alternative (per-scope floor /
  normalization / calibrated gate); do not keep a global scalar that emits confident-wrong answers.
- If the graph boost shows no measured lift over λ=0 → **cut HDBSCAN** from the build and correct the README
  (215). "Compute it and ignore it" is not an acceptable end state.
- If tuning can't reach the target → document the gap and the next architectural bet in this assessments
  dir; do not quietly lower the target to claim success.

---

### Phase 4 — Prove efficacy  *(the headline)*

**Tickets:** 205 (optional cheap preliminary synthetic A/B — may run anytime as an early smoke signal),
then **218 phases 1–2**: layer-on/off iterations on TRAIN + the **held-out TEST transfer measurement**.

**Why last:** depends on a green tree (Phase 0), the cold-start fix (Phase 1), the corpus (Phase 2), and a
tuned retriever (Phase 3).

**Inspect after the batch:**
- 205 (if run): early ON-vs-OFF signal on synthetic tasks — a directional sanity check, **not the proof.**
- 218 headline: layer-ON vs layer-OFF on the **disjoint held-out TEST set**, as a **difference-of-differences
  with variance/CI**. Plus the same-set 3-run trajectory (narrative only, flagged for memorization risk).
- Retrieval-source attribution: per passing instance, did the helpful skill come from **priming** or
  **`find_skill`**? Any causal "skill X caused pass" claim must be ablation-backed.
- Result written to `docs/assessments/` with raw data.

**Expect:** if the system has real value, the held-out ON arm beats OFF by a statistically honest margin,
and source attribution tells us which intent earns it. A small-but-real, well-attributed transfer gain is a
genuine win and the strongest possible evidence.

**If expectations aren't met:**
- If 205 (cheap) shows nothing → **debug before spending on a full 218 run.** Check: was anything retrieved?
  Were the tasks ones the corpus could help? Is injection actually reaching the agent?
- If 218 held-out shows null/negative → **publish it honestly.** Then diagnose with the attribution data:
  was nothing relevant retrieved (retrieval problem → 210/217), or was it retrieved but unused/unhelpful
  (the value thesis itself is weaker than hoped)? That distinction decides whether the project iterates or
  pivots. A buried null result is the one outcome the constitution forbids.

---

## Dependency graph (quick reference)

```
Phase 0:  207, 211, 212, 213, 215, 206         (independent — start now)
Phase 1:  217  (cold-start fix + intent split) ── prerequisite for 218 measured runs
Phase 2:  216 ← 218 run 1                       (builds the corpus)
Phase 3:  209, 208, 210, 214, 217-validation    (need corpus + held-out queries)
Phase 4:  205 (cheap, anytime) → 218 phases 1–2 (held-out transfer = headline proof)
```

## The single most important sentence

We have proven the system is **correct**. Phases 2–4 exist to find out whether it is **useful** — and the
guards in 216/218 exist so that when we answer that question, we believe the answer.
