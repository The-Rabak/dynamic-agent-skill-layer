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

*Prior scores for continuity: 58% (05-21 adversarial) → 72% (05-26) → 78% (05-28) → 82% (05-31) → 84% (06-02) → re-based by 06-07 brutal (efficacy 0/10). This assessment: trust-basis ~87%, endgame-basis ~66%, efficacy 1.5/10 (scale built, never stood on).*
