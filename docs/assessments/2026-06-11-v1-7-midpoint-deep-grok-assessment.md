# V1.7 Midpoint Deep-Grok Assessment — 2026-06-11

**Assessor:** Claude (Fable 5), full-repo fan-out (plans v1/v1.1/v1.5/v1.7, all architecture docs + ADRs, all 44 tickets, 9 execution sessions, all 6 prior assessments, dream-state contract suite, codebase map)
**Assessed at:** feat/v-1-7, Phase A closed (T01–T09; T07 skipped), Phase B queued
**Assessment basis (per owner instruction):** the *final product* as defined by the plans, the architecture, and the 23 dream-state endgame tests — not the current version's own scope.

---

## Verdict (one line)

> **The creature is alive, honest, and raised on its own cooking — and it has still never stepped on a scale.**

Continuing the lineage: *iron skeleton* (72%) → *spine forged* (78%) → *loop closes on bench* (82%) → *heartbeat* (84%) → *plumbing excellent, brain unproven* (06-07 re-base). Today the organism demonstrably feeds itself — the 262-skill corpus was grown by the real pipeline from 24 genuine dev sessions, which means the self-growth loop is not a claim anymore, it's a dataset. What remains unproven is the only thing that ever mattered: whether carrying this organ makes the host agent stronger.

## Two scores, because there are two books

The historical 58→84% trajectory measured **trust infrastructure** — does it run, does it degrade honestly, does the loop close. On that scale this repo has kept climbing:

- **Trust-infrastructure score (old 12-dim basis): ~87%.** Loop closed in the deployed body, live suite green, corpus self-grown, agent graph surface live-proven, governance airtight.
- **Endgame score (efficacy-weighted, dream-state denominator): ~66%.** The biggest single term — "the layer makes agents measurably better" — is still 0 measured evidence, and 16 of 23 dream contracts are still `pending_contract()` stubs.

The 06-07 brutal assessment split the project into plumbing (excellent) and brain (decorative). The honest midpoint update: **the plumbing got even better, the brain got honest, and the scale got built but not yet stood on.**

## Scorecard

| Dimension | Score | Δ trend | One-line basis |
|---|---:|---|---|
| Architectural integrity | 9.5 | = | Survived three major versions with zero boundary violations; CQRS/ADR discipline intact; qdrant_hybrid CQRS-break correctly quarantined as experimental |
| Loop closure / deployment truth | 9.0 | ▲ | The v1.1 headline risk is fully dead: live graph boot, hot-swap refresh, and now a corpus literally produced by the loop (T10 dogfood) |
| Process honesty / compounding | 9.5 | ▲ | Assessments become tickets become measured closeouts; T07 skipped on evidence and *recorded*; "0.80 unvalidated, not falsified" is exemplary scientific writing |
| Lifecycle governance / human gate | 9.0 | = | `.pending`/`.retired`/tombstones unchanged and airtight; no auto-approval path anywhere |
| Extraction & corpus | 8.0 | ▲ | 262 genuine skills, 71% multi-view, 60 real communities; remaining gap is local-vs-frontier density (2.6×), a known quantity not a mystery |
| Agent-native surface | 8.0 | ▲ NEW | search_skill_graph / find_skill rationale / inspect_skill live-proven 3/3 vs the real 262 server; relevance finally human-readable (0.65–0.84) after the RRF-artifact fix |
| Resilience / graceful degrade | 8.0 | = | DS-003/004/005 chaos, replay, drift-convergence run live; residual soft-assertion debt flagged 06-04 (hardcoded `Passed` in reports) partially addressed, same class actively being fixed (2f8550b) |
| Retrieval quality (measured) | 7.0 | = | MRR 0.767 / hit@3 0.867 / no-match precision 1.0 — solid, plateaued, and *stale*: the only labeled fixture is 0/30 aligned with the live corpus |
| Measurement integrity (the ruler itself) | 6.0 | ▼ NEW | Real-server-over-HTTP standing rule is exemplary; but a fixture that can't see the corpus, and five arms returning *identical* 0.767, says the instrument currently can't discriminate — T11 is the keystone ticket of the entire repo |
| Skill-graph intelligence (SkillRAE→SkillDAG) | 5.5 | ▲ | Communities honestly cut from ranking; typed edges + acyclicity guards shipped; but zero measured contribution to retrieval quality yet — today it's a *surface*, not a *brain* |
| Observability / causal trace | 6.0 | = | Health/arm/backend surfacing good; DS-011/DS-022 trace-graph band untouched |
| Multi-tenant / security band (DS-008/010) | 4.0 | = | Still panic-stubs; correctly deferred but it is the soft underbelly of every "team scope" ambition |
| Self-healing / platform band (DS-014–023) | 2.5 | = | 10 dream contracts with zero code; honestly labeled V2/V3 |
| **Efficacy (the thesis)** | **1.5** | ▲ from 0 | Still unmeasured — but for the first time every prerequisite exists: real corpus ✅, attribution fields ✅, A/B harness designed ✅, hygiene gate (T13) queued. The 1.5 is for building the scale; nobody has stood on it |

## The arc (why this project's process is its real moat)

Each version is the previous assessment's top finding turned into a plan, executed, and closed with measurements:

- **V1/V1.1** built correctness and was told "the deployed binary reads an empty graph" → **V1.5** existed to close exactly that loop, and did (18/18 live, CI-gated).
- **V1.5** was told "plumbing excellent, efficacy zero, corpus is a toy" (06-07 brutal) → **V1.7** is *literally that battle plan*: real corpus (T10 ✅), retrieval ceiling probes (T01–T04 ✅), honest contract docs (T08 ✅), then efficacy (T14/T15).

This is compounding engineering actually compounding. 170 of 171 todos closed with frontmatter status. Negative results (qwen3 neutral-or-worse, hybrid zero uplift, communities inert) are *recorded and acted on* — defaults demoted, tickets skipped — instead of being quietly shipped. Very few codebases kill their own darlings this consistently.

## The most important datapoint in the repo: 0.767, five times

Nomic dense = 0.767. Qwen3 (3.3× dims, 3× latency) = 0.767. Snapshot-hybrid BM25+RRF = 0.767. Qdrant sparse-idf hybrid = 0.767. Weight sweeps = no movement.

Three readings, in descending order of likelihood:

1. **The eval set can't discriminate.** ~30 queries, lexically distinctive, all arms converge on the same easy top-1s; MRR quantization on small N makes *identical* scores across five genuinely different systems almost diagnostic of a saturated/underpowered fixture. The subsequent discovery that the fixture is 0/30 aligned with the live corpus strengthens this.
2. **A genuine ceiling** of embedding+eq.3 scoring at this corpus size — possible, and consistent with the "ceiling = model + scoring, not candidate gen" closeout.
3. **0.767 with hit@3 0.867 and zero false matches is simply… good**, and 0.80 was a frozen aspiration, not a derived requirement. Worth saying out loud: no user-facing failure has ever been traced to the 0.033 gap.

**Consequence:** T11's fixture build is the single highest-leverage piece of work in the repository — more important than any retrieval feature shipped in Phase A, because every Phase A verdict ("hybrid is a tie") is provisional until re-measured on an instrument that can see. **One warning for T11:** the leading fixture method (derive queries from each skill's `use_when`) has a circularity smell — querying the corpus with text generated from the corpus measures self-recall, not retrieval. Mix in genuinely held-out task descriptions from session transcripts the skills were *not* extracted from, and keep the negative set adversarial.

## The unweighed thesis

The 06-07 line still governs everything: *"The risk isn't that the system doesn't work; it's that it works perfectly and doesn't matter."* Phase B is correctly aimed at exactly this (T14 ON/OFF, T15 SWE-bench compounding with difference-of-differences and attribution). Two pre-registration cautions before the scale is stepped on:

- **Small-N truth:** ≥10 tasks (T14) and SWE-bench Lite subsets give wide confidence intervals. Decide *now* what passes: e.g. "ON ≥ OFF on ≥7/10 with no catastrophic regression," not a post-hoc reading of noisy means. A null result on 10 tasks must be allowed to say "underpowered," not be spun either way.
- **Attribution is the real prize:** if uplift appears, knowing whether it came from SessionStart priming, mid-session `find_skill`, or sheer context mass determines whether T12 (priming mode) is the next version or a dead end. The instrumentation fields exist — make them non-optional in the harness.

## Distance to the dream state

The 23 endgame contracts split into three honest bands:

- **Trust band (DS-001–013):** 7 have live bodies; chaos/replay/drift/load run against real containers today. Residual debt: DS-006's NoMatch-counted-as-success class (flagged 06-04, same class actively being eliminated), report-level hardcoded `Passed` outcomes, and DS-008/009/010/011/012/013 still stubs. This band is finishable within V1.x.
- **Intelligence band (DS-014, 016, 018, 020, 024):** zero code; gated entirely on the efficacy chapter proving there is intelligence worth governing.
- **Platform band (DS-015, 017, 021–023):** time-travel, cross-repo collective intelligence, shadow deploys, bit-perfect replay — two major versions away minimum, and correctly not being pretended at.

The realistic near-endgame — and the right one — is narrower than the full dream: **a single-repo, local-first, self-growing skill memory with measured task uplift.** That product is one good Phase B away. The platform dream stays parked behind it.

## Risks and watch-items

1. **Eval circularity in T11** (above) — the one way the measurement era could quietly lie.
2. **Workspace gates RED pre-existing** (clippy dead-code in e2e harness, fmt debt from T04/T05) — known, not a regression, but it must clear before the V1.7 final gate or it erodes the "honest tree" property everything else stands on.
3. **Qwen3 operational debt:** ~7-min boot re-embed with `/health` flipping healthy early; precomputed-vector boot is filed but until then every restart is a small lie to the health endpoint.
4. **Floor 0.48 calibrated on the dead corpus/model pair** — must be re-swept on the T11 fixture; live no_match behavior currently looks correct but is unverified at the threshold edges.
5. **Single-developer bus factor on WSL2** — the environment has already eaten one working tree (unflushed-write truncation). Commit cadence is the mitigation and is being observed.
6. **Engineering mass vs. proof mass:** ~56k prod LOC + 23k test LOC, 9 crates, 10 migrations, 5 containers — a serious distributed system — resting on 0 bits of efficacy evidence. Phase B doesn't just decide V1.7; it decides whether the mass was load-bearing.

## Where it's going

Critical path is correct and tightly sequenced: **T11 (build a ruler that can see) → T12 (priming/intent split, data-driven) → T13 (no-fakes hygiene) → T14 (ON/OFF) → T15 (compounding transfer)**, T16 parallel.

- **If T14/T15 show even modest, attributable uplift:** this becomes a genuinely differentiated product — local-first, human-gated, self-compounding agent memory with an evidence trail no comparable system has. The trust infrastructure that looked over-built becomes the moat.
- **If null:** the fallback levers are already identified in-repo — extraction density (local 2.6× gap), injection format/UX of compiled context, and the intent split (priming vs task retrieval) — and the system is honest enough to find out *which*. A null result here would be reported as a null result; six assessments of precedent say so.

Either way, this is the rare repo where the next 8 tickets will produce an *answer* rather than more capability. That is exactly where a project like this should be at its midpoint.

---

## Addendum — 2026-06-11 (same day): endgame re-armed against CL-bench + ticket tightening

Two pieces of work landed after the body above was written. Both change *what the endgame is*, not the midpoint score — so the scorecard stands, but the denominator it's measured against just got harder and more honest.

### 1. Phase-B tickets tightened (T11/T12/T14/T15 amended, T17 added)

The retrieval-evaluation critique in this assessment was converted into ticket law:

- **T11 is now instrument-first.** Before any arm verdict it must pass an **α=0 negative-control gate** (semantic scoring off must crater MRR, or the fixture is rejected), report **candidate-recall@limit** as a first-class per-arm metric (the only thing candidate-gen can move at 262 skills), use **paired per-query rank diagnostics + sign tests** instead of 3-decimal mean equality, add an **MRR@10 resolution arm**, and build a **conditional env-gated lexical-ranking arm** (δ·BM25 in eq.3) so the hybrid bet gets tested as a *ranking* signal at least once. The anti-circularity rule is now an owner decision in-ticket: headline queries come from held-out transcript *problem statements*, `use_when`-derived queries demoted to a labeled secondary stratum.
- **T12** gains a hard T11 dependency + a session-start query stratum + pre-registered per-signal ROI thresholds.
- **T14** gains pre-registration, paired design, a **placebo arm** (matched-token-mass irrelevant context), per-pull attribution, and a third honest outcome: **PASS / FAIL / UNDERPOWERED**.
- **T15** gains a committed **minimum-detectable-effect** so a null on a small SWE-bench subset reports UNDERPOWERED, not "no effect."
- **New T17** (`mcp-server-boot-readiness-honesty`): `/health` must not report ready during the ~7-min qwen3 boot re-embed; load precomputed vectors at boot. Sequenced after the in-flight T13 session (shared `crates/mcp-server` files). This is the same health-marker-honesty class the project has fixed twice before, and it directly protects T11's measurement validity.

**Why this matters to the score:** the "measurement integrity 6.0" line was the binding constraint on everything downstream. These amendments don't raise it yet — they make it *raisable*, by forcing the instrument to prove it can discriminate before any verdict rides on it. The next assessment can move that number only if T11 actually runs the negative-control gate and it craters.

### 2. The dream-state suite was fully realized and re-aimed at CL-bench (arXiv:2602.03587)

The endgame is no longer 24 mostly-stubbed contracts with 7 live bodies. `tests/e2e/test_dream_state_contract.rs` went 2,987 → 4,933 lines; **every `pending_contract` panic is gone**. The suite now has three honest bands:

- **Trust band (DS-001–013): all live, all hard-asserted.** DS-001/002/008/009/010/011/012/013 were promoted from panics to real bodies that drive the containerized stack — closed-loop *extraction* determinism, stdio↔HTTP transport parity, multi-repo canary isolation + repo-scoped suppression, restart/Redis-restart suppression durability, a hostile-input barrage, reason-coded observability, provider parity, and a lifecycle SLA with a hard "rejected never activates" no-auto-approval gate.
- **Platform band (DS-014–024): live RED capability probes.** Each drives the real server and asserts the dream surface is advertised in `tools/list`, red-lining the exact missing tool name as the machine-checkable definition of done. No more silent placeholders; add the tool, the contract greens.
- **NEW — Context-Learning Mastery band (DS-025–030):** the CL-bench counter-move encoded as executable contracts. One-shot acquisition of a *non-pretrained* invented rule; procedural fidelity (complete + in-order); **supersession** (a corrected rule must outrank the one it contradicts); compositional retrieval across typed `requires↔produces` edges; **zero negative transfer**; and the north star, **DS-030's compounding mastery curve** — coverage of a fixed task must be monotone non-decreasing and net-positive as skills accumulate. That is "the system gets better the more it learns" as a monotonicity assertion — precisely the property CL-bench shows static models lack.

**New learning that updates the thesis framing:** CL-bench reframes the efficacy question from "does injected context help a task" to "can the system durably acquire what a model *cannot* learn in-context." That is a stronger, more defensible differentiation than generic RAG uplift — and DS-025/DS-030 are the cleanest expression of it in the repo. It also sharpens the efficacy gate: T14/T15 should adopt at least one CL-bench-shaped task (a novel rule/procedure absent from pretraining) as a headline arm, because that is where the layer's advantage is largest and least confoundable by "any extra context helps."

**Process note (honesty cost):** a prior session had left a truncated, brace-imbalanced DS-010 block (ended mid-token in `ass`) that would never compile — found and excised surgically. Worth a standing watch-item: large hand-authored test files are a corruption surface; the compile-gate in `run-e2e-tests.sh` (`--skip ignored`) is the thing that catches it and must stay green.

**Status of the new suite:** compiles clean and fmt-clean; all 30 DS contracts register. The bodies are unrun against live containers in this session (they're `#[ignore]` by contract). That is the single biggest caveat below.

### What the follow-up assessment (a few days out) should focus on

1. **Did T11 build a ruler that can see?** This is the keystone. Specifically: did the α=0 negative-control arm actually crater MRR on the new fixture (proving discrimination), and did candidate-recall@limit and paired sign-tests replace mean-equality verdicts? If T11 shipped a verdict *without* the negative-control gate passing first, treat every retrieval conclusion as unproven and say so. Grade **measurement integrity** primarily on this.
2. **Do the promoted dream contracts actually pass live?** DS-001/008/009/010/011/012/013 are written but unrun here. Run them against the real stack (or read the persisted `tests/e2e/reports/*.json`) and report a real pass/fail/RED matrix. Watch for the old DS-006 failure mode (NoMatch-counted-as-success) creeping back in — the new bodies have non-vacuity guards, so verify those guards fired. Promote the dream-state readiness number only on observed green, never on "written."
3. **Is the CL-bench framing reflected in the efficacy harness?** Check whether T14/T15 adopted a CL-bench-shaped headline task (novel non-pretrained rule/procedure) and whether DS-025/DS-030 ran. The compounding-curve (DS-030) and one-shot-acquisition (DS-025) results are the highest-signal evidence the project can produce; if they're green, the efficacy score moves off 1.5 for the first time on real grounds.
4. **Pre-registration discipline held?** For any T14/T15 run, confirm the pass criterion / minimum-detectable-effect was written *before* the data existed and the report classifies PASS/FAIL/UNDERPOWERED against it verbatim. A post-hoc reading is a regression in the project's strongest asset (its honesty), and should be scored as one.
5. **Did the workspace gates go green?** The pre-existing clippy dead-code + T04/T05 fmt blockers, plus the new T17 boot-readiness item, all gate the V1.7 final. Track whether they cleared; an honest tree is the foundation every other score rests on.
6. **Watch-item, not a focus:** confirm no new truncation/corruption in the now-4,933-line dream file, and that T13's landing didn't collide with the untouched-this-session mcp-server surface T17 depends on.

The shape of the next assessment is therefore: *less about capability, entirely about whether the measurement apparatus is real and whether the CL-bench-shaped contracts run green.* The project has, this session, defined a harder and more honest finish line. The score moves when the stack is driven across it — not before.

---

*Prior scores for continuity: 58% (05-21 adversarial) → 72% (05-26) → 78% (05-28) → 82% (05-31) → 84% (06-02) → re-based by 06-07 brutal (efficacy 0/10). This assessment: trust-basis ~87%, endgame-basis ~66%, efficacy 1.5/10 (scale built, never stood on). Addendum 06-11: endgame re-armed against CL-bench (DS-025–030) and Phase-B tickets tightened to instrument-first + pre-registered; scores unchanged — the finish line moved, the runner has not yet crossed it.*
