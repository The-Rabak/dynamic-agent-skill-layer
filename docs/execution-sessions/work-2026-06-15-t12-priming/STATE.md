---
source_type: ticket
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/12-trigger-aware-retrieval-priming-mode.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "promoted from todo #220 (P1); restructured 2026-06-12 (instrument→T18, recurrence→T19)"
brainstorm_ref: n/a
started: 2026-06-15
status: implemented-owner-decisions-deferred
execution_shape: vertical-slices
current_unit: 4
total_units: 4
session_id: work-2026-06-15-t12-priming
---

## WHY Linkage
- Canonical WHY source: plan `## Agent usefulness targets` (todo #220)
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: make the production SessionStart prime actually useful (set-coverage@3 0.0685 → ≥0.17 target), measured through the real compile_context path; unblocks T15.
- Success-criteria focus: typed RetrievalIntent seam (Task byte-identical); verbose-priming fix; recurrence-baseline + bounded freshness ranker; measured keep/drop on the T18 instrument; SessionStart p95 <500ms.

### TDD Contract
- Effective mode: Ralph-driven TDD (ticket tdd_mode: ralph)
- Effective loop: failing unit tests first → minimal impl → refactor → post-refactor rerun; unit + e2e (real mcp-server over HTTP) evidence required.
- Required evidence: cargo test (crate units) + real-server priming sweep on the T18 session_start stratum (negative control FIRST, then paired + sign-test).
- Exceptions: none.

### Constitution Context
- SessionStart prime within 500ms budget; no LLM call on the hot path.
- No stubs/fakes in production paths — fail loud (machine-wide + project mandate).
- Measurement drives the REAL running mcp-server over HTTP; no in-process reconstruction.
- No new candidate sources / no broadened candidate generation (T11 constraint).
- Do NOT naively lower the global 0.48 floor (protects T11 no_match precision 0.92 on Task); any floor change is Priming-intent-scoped only.

### Architecture Handoff
- Artifact: explicit plan-derived handoff (plan ## Agent usefulness targets + ## Retrieval Flow).
- Feature homes: crates/retrieval (intent seam, segmentation, intent floor, priming ranker), crates/compiler / crates/mcp-server (SessionStart→Priming mapping, trigger), scripts/ (measurement).
- Seam: SkillRetriever::retrieve gains a RetrievalIntent; Task default keeps byte-identical behavior.
- Signals: recurrence = existing SeededSkill.prior (γ usage prior); freshness = NEW created_at loaded into SeededSkill (owner decision 2026-06-15).
- Review guidance: prove Task path unchanged (existing quality gate green); every priming number has a persisted raw artifact.

### Owner Decisions (locked 2026-06-15)
1. Scope = FULL mechanism + measurement this session (Units 1–4).
2. Freshness = add created_at to the snapshot (true brand-new detection), not low-usage approximation.
3. STOP-and-ask gates (post-measurement): default-ON flip; per-signal keep/drop verdicts.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | Typed RetrievalIntent seam (Task byte-identical) + SessionStart trigger→Priming | tracer-bullet | the intent distinction the whole ticket rests on | completed | 1 | unit-01-retrieval-intent-seam.md |
| 2 | Verbose fix: query-side multi-view / max-over-segments (Priming) | expansion | fixes verbose dilution (coverage 0.027) | completed | 1 | unit-02-query-side-multiview.md |
| 3 | Intent-conditional floor + priming ranker (recurrence prior + freshness slot via created_at) | expansion | raise set-coverage@3 toward ≥0.17 | completed | 2 | unit-03-intent-floor-priming-ranker.md |
| 4 | Re-measure on T18 instrument (neg-control + paired + sign-test); per-signal verdicts | hardening | the evidence + owner verdicts | completed (verdicts owner-deferred) | 1 | unit-04-measurement-verdict.md |

## Measured verdict (Unit 4, 2026-06-15)
- Neg-control PASS (62.5% crater); baseline reproduces T18 0.0685 exactly.
- Primed cov@3=0.0805, paired delta +0.012 sign_p=1.0 → FAILS the +0.10 bar.
- Per-signal (ablation-isolated): recurrence DROP (Δ=0.000 @ w=0.1 and 0.6); freshness slot DROP (isolated Δ=0.000); centrality/recent-use DROP (default). Only floor+multiview moves anything.
- Real wins: no_match 14%→0% (the motivating fix); freshness hit-rate thin 0.73→0.91 (from floor+N, not the slot).
- BLOCKER: verbose p95 ~2240ms breaches 500ms (Ollama semaphore serializes per-segment embeds; even T18 baseline single verbose embed was 560-734ms >500ms).

## Owner gate (2026-06-15)
- #1 default-ON: NOT as-is → owner chose "fix multi-view latency, then reconsider".
- #2 per-signal verdicts: owner HOLDING (reviewing raw artifacts tests/e2e/reports/retrieval/t12_priming_*.json). DO NOT finalize signal verdicts yet.
- Action: added env-tunable priming_max_segments; rebuilding image to measure the latency/coverage curve at tighter caps (1/2/3).

## Learnings Brief
- [rust] `SkillRetriever::retrieve(prompt, repo_path, intent)` is the seam; `RetrievalOrchestrator<E>` is the only real impl. Test impls all `#[cfg(test)]` (orchestrator.rs, find_skill.rs TwoSkillStub, lib.rs EmbedCountingRetriever, test_admin_tools.rs EmptyRetriever).
- [rust] orchestrator.rs test scaffolding: `versioned_snapshot(n)`, `qdrant_hybrid_snapshot()`, `ConstantEmbeddingService`→`[1.0,0.0,0.0,0.0]`. Use these for Priming-branch tests.
- [mcp-server] intent derived in `invoke_and_capture_outcome` before `retrieve`; `TriggerKind::Other` is `#[serde(other)]`, keep last.
- [build] `_unused` param prefix avoids `-D warnings` clippy without `#[allow]`. Clippy form `-p retrieval -p compiler -p mcp-server --all-targets`.
- [scope] retrieve() call sites = compile_context.rs:133 (intent), find_skill.rs:105 (Task), + 8 orchestrator test calls.
- [rust] Unit 2: orchestrator `retrieve` now branches per backend, each embeds + records on its own (breaker allow_request hoisted once). Priming = segment_prompt → embed_batch (1 call) → per-segment search_scopes_concurrently passes → merge_scope_results_max(passes, candidate_limit). `search_scopes_concurrently(prompt, &emb, Arc<snapshot>, &config, &scopes)`.
- [rust] 0.48 floor still applied per-segment pass; Unit 3 adds Priming-scoped intent floor + recurrence(prior)/freshness(created_at) ranker. Test embedding services: ConstantEmbeddingService (uniform), KeywordAwareEmbeddingService (text→vec by keyword).
