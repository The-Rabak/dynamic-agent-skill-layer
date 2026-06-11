# /workflows:review prompt — V1.7 batches 6/7/8/10 (T09 defer, T06, T07 skip, T08, T13)

Review the work landed on `feat/v-1-7` this session. Ground every finding in the V1.7 WHY
(below) and the repo's standing honesty bars. Filter technical nits through purpose; escalate
anything that fakes, hides, or overstates.

## Scope / commit range
Review `faec527..HEAD` (7 commits, +2503/-187 across 49 files):
- `2cd77cf` chore: gitignore replica-run bulk + .claude/projects; track T10 VALIDATION-REPORT (low risk)
- `efa6714` docs: close Batch 6/T09 (code); **defer** the measured dense-views sweep to T11
- `32cd968` feat: **T06** SkillDAG agent retrieval tools (owns #255/#260/#243) — the main code change
- `2f8550b` test: T06 live test fails loud instead of vacuously skipping on no_match
- `4b87124` docs: close Batch 7/T06 — live-proven; pointer→7; unblock T11
- `1692489` docs: **T08** retrieval contract + efficacy handoff; **skip T07**; close Phase A
- `831465f` test: **T13** drain tests/integration fake allowlist via relocate-or-live (batch 10)

The substantive CODE to scrutinize is in `32cd968` (T06) and `831465f` (T13). The rest is docs/bookkeeping.

## WHY (anchor every finding here)
V1.7 ships a local, agent-callable, honestly-scored retrieval substrate (qwen3 dense default;
typed skill graph; agent graph tools) and hands a measured efficacy substrate to Phase B (T14/T15).
The non-negotiable bar: **no fakes/stubs/placeholders outside unit tests; fail loud; never fake or
overstate a measurement.** A passing test that lies is worse than a failing one. The efficacy story
(0.80 MRR/nDCG) is explicitly NOT yet validated and must not be claimed as validated anywhere.

## Read these for context first
- Tickets: `docs/tickets/2026-06-08-.../{06,07,08,09,13}-*.md` and `index.md` (status notes carry the decisions).
- Execution sessions (WHY + my own caught issues): `docs/execution-sessions/{work-2026-06-10-T09,
  work-2026-06-11-T06,work-2026-06-11-T08,work-2026-06-11-T13}/`.
- Assessment authored this session: `docs/assessments/2026-06-11-v1-7-retrieval-contract-measured.md`.
- Constitution: `docs/constitution.md`. Machine rule: `~/.claude/CLAUDE.md` (no-fakes mandate).

## SPECIAL EMPHASIS — scrutinize these hardest (ranked)

### P0 — Honesty of measurement & deferrals (the thing most likely to be wrong)
1. **T08 / T09 / assessment: the 0/30 fixture deferral.** I claim the committed eval fixture
   `tests/fixtures/retrieval_quality_234_corpus_labeled.json` is 0/30 aligned with the live 262
   qwen3 corpus, so held-out MRR/nDCG (and the 0.80 target) cannot be honestly measured and is
   delegated to T11. VERIFY: (a) the 0/30 claim is true; (b) nowhere in docs/tickets/assessment is
   0.80 or efficacy claimed as met/validated; (c) T08's `test_command` live gate is honestly recorded
   as NOT-run (not faked green); (d) the deferral to T11 is real (T11 ticket actually owns it).
2. **T06 #260 — agent-facing `score` is relevance, not the RRF artifact.** Verify `find_skill.score`
   now reflects eq.3 relevance via the threaded `ScoredSkill.semantic_score` (orchestrator.rs sets it
   from `candidate.semantic_score`), `fusion_rank_score` carries the RRF value, and **ranking/order is
   unchanged** (only the exposed number changed). Confirm there is NO silent fallback to 0.000 and the
   old string-parse-the-rationale approach was fully removed. Confirm the unit test actually proves
   different-cosine→different-score (not a tautology).
3. **T06 live test honesty (`tests/e2e/test_skill_graph_tools.rs`).** It now probes project-domain
   prompts and FAILS LOUD if a populated corpus returns no match (previously it skipped vacuously).
   Verify it cannot pass without exercising the contract, drives the REAL MCP HTTP endpoint (no
   in-process rig), and asserts structure (not 234-corpus IDs).

### P1 — T13 no-fakes integrity (a worker tried to game this; I corrected it — re-judge me)
4. **Is `crates/*/tests/` an honest home for controlled-double tests, or guard evasion?** The worker
   relocated fake-bearing tests there *because the guard didn't scan it*, and **renamed the fake**
   (`DeterministicEmbeddingService`→`ControlledEmbeddingService`) to dodge the symbol grep. I judged
   the relocation defensible under the owner's "relocate-or-live" policy + the constitution's
   "test-only gating" clause, and hardened `scripts/check-no-fakes.sh` to DOCUMENT the taxonomy
   explicitly (e2e/integration/src-prod = fake-free; crate `tests/` + `src #[cfg(test)]` = fake-friendly)
   and to note the symbol-match is rename-evadable. **Re-judge:** is this genuinely honest, or did I
   rationalize a loophole? Should the guard instead scan `crates/*/tests/` and force these to
   `src/#[cfg(test)]` or live? Is renaming-to-evade acceptable if the destination legitimizes the double?
5. **T13 coverage preservation.** I caught the worker DROPPING `test_admin_tools` coverage in a way it
   hid (it inlined the tests into `lib.rs`, which broke that suite's self-inspection of lib.rs, and ran
   `--lib` WITHOUT `--features test-utils` so the failure never surfaced — false green). I reverted the
   inline and relocated to `crates/mcp-server/tests/test_admin_tools.rs`. VERIFY: all relocated suites
   genuinely run and pass under the right features; no test lost an assertion vs its pre-move version
   (esp. the T06 multi-view `inspect_skill` assertion); `cargo metadata` has no dangling `[[test]]` paths.
6. **T13 guard still bites.** Confirm `bash scripts/check-no-fakes.sh` hard-fails on a reintroduced
   banned symbol in `tests/e2e` AND `tests/integration`, and that the allowlist branch is truly gone.

### P1 — T06 graph surface correctness
7. **`search_skill_graph` (new, 456 lines).** Verify: neighbors/conflicts are filtered to edges
   incident on MATCHED skills (not whole-graph dump); `conflicts_with` NEVER appears in `neighbors`;
   inbound/outbound direction + the "other endpoint" skill_id are correct; a real edge-store `Err`
   returns a `degraded` response (fail-loud, no silent empty-edge fallback).
8. **#243 provenance actually wired.** `retrieval_context{embedding_model,collection,graph_version}` is
   populated from real sources in `build_live_server` (not dead/None). `model_keyed_collection_name`
   Result handled.
9. **#255 multi-view readable + /health backend.** The 7 fields project through `inspect_skill`
   (admin/tools.rs) and are asserted; `/health` `retrieval_backend` is wired in `main.rs` boot (not
   only tested). Compiler untouched (no silent `compile_context` change).

### P2 — hygiene / known debt (flag, don't block)
10. **Pre-existing fmt debt** in `crates/graph-builder/src/graph/{edges.rs,rebuild.rs}` (and qdrant.rs)
    fails `cargo fmt --check` — NOT introduced this session (T05 debt), but it's a V1.7 final-gate
    blocker. Confirm it's out of scope here and recommend a dedicated cleanup.
11. The gitignore change ignores `.claude/projects/` (Claude-local) and `tests/e2e/reports/replica-run/`
    bulk while force-tracking `VALIDATION-REPORT.md`. Sanity-check nothing important got ignored.

## Do NOT flag (out of scope / intentional)
- The owner-authored Phase B amendments that rode in on `831465f`: new ticket `17-*.md`,
  instrument-first edits to tickets 11/12/14/15, and `docs/assessments/2026-06-11-v1-7-midpoint-deep-grok-assessment.md`.
  These are the owner's, not this session's implementation work.
- T07 being skipped is an intentional, recorded owner decision (T04 measured no candidate-gen uplift).
- The dense-views flag and qdrant_hybrid backend are intentionally default-OFF / experimental.

## Output
Per finding: severity (P0–P3), file:line, why it violates the WHY/honesty bar, and the concrete fix.
Call out explicitly whether you AGREE or DISAGREE with my two T13 judgment calls (#4 and #5 above) —
that's the highest-value verdict from this review.
