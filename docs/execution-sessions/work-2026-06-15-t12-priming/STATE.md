---
source_type: ticket
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/12-trigger-aware-retrieval-priming-mode.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "promoted from todo #220 (P1); restructured 2026-06-12 (instrument→T18, recurrence→T19)"
brainstorm_ref: n/a
started: 2026-06-15
status: in_progress
execution_shape: vertical-slices
current_unit: 1
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
| 1 | Typed RetrievalIntent seam (Task byte-identical) + SessionStart trigger→Priming | tracer-bullet | the intent distinction the whole ticket rests on | pending | -- | -- |
| 2 | Verbose fix: query-side multi-view / max-over-segments (Priming) | expansion | fixes verbose dilution (coverage 0.027) | pending | -- | -- |
| 3 | Intent-conditional floor + priming ranker (recurrence prior + freshness slot via created_at) | expansion | raise set-coverage@3 toward ≥0.17 | pending | -- | -- |
| 4 | Re-measure on T18 instrument (neg-control + paired + sign-test); per-signal verdicts | hardening | the evidence + owner verdicts | pending | -- | -- |

## Learnings Brief
_No learnings yet._
