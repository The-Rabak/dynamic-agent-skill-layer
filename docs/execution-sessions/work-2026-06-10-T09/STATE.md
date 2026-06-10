---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/09-dense-multiview-embedding-views.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "plan ## Proposed V1.7 Architecture > Embedding views (lines 243-251); ## Execution Slices > Slice 4 (dense indexing)"
brainstorm_ref: none
started: 2026-06-10
status: in_progress
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-06-10-T09
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: Let DENSE retrieval exploit T03's multi-view fields, not just sparse/BM25. Today dense embeds only `e_summary` (name+description+tags); the 9 multi-view fields (use_when/tools/artifacts/invariants/requires/produces/avoid_when) are read by the BM25 path only. T09 builds the missing dense views (e_task/e_needs/e_negative) so the hybrid bet is fully testable on both channels (unblocks the real verdict in T11).
- Success-criteria focus: "e_task/e_needs/e_negative built from T03 fields with bounded text, observable in graph/snapshot metadata; a real-server sweep records ON-vs-OFF held-out MRR/nDCG/no-match delta; views that don't improve quality are left OFF by default with the measured delta recorded; p95 within the 500ms budget; `cargo test -p retrieval` green; no eq.3 regression."

### TDD Contract
- Effective mode: Ralph-driven TDD (plan overrides local; plan `tdd.mode: ralph`).
- Effective loop: Failing tests first -> minimal implementation -> refactor -> post-refactor rerun.
- Required evidence: Unit tests for view-building (bounded text from T03 fields) + multi-view fusion at the α/l1_semantic seam (`cargo test -p retrieval && cargo test -p mcp-server --lib`). PLUS a real-server e2e retrieval sweep ON-vs-OFF (orchestrator-driven over the live mcp-server, NOT in-process).
- Exceptions: None.

### Constitution Context (v2.1.0)
- Local-first preserved: views are built from local Ollama embeddings only; no external API.
- Zero-touch session start: `compile_context` p95 MUST stay < 500ms with views enabled — measure before defaulting.
- Approval-sensitive: NO schema migration is intended for T09 (dense view embeddings live ONLY in the in-memory snapshot, recomputed at boot exactly like the existing `e_summary` and subunit embeddings — see lib.rs:1128-1171). If the agent finds a migration is unavoidable, it must STOP and surface it for owner approval rather than adding one silently.
- No-stubs / fail-loud (machine-wide + constitution): embedding-batch length mismatches must fail loud, exactly like the existing `embed_batch` guards at lib.rs:1119 and lib.rs:1149. No silent zip truncation.

### Architecture Handoff
- Artifact: plan-derived handoff (parent plan ## Architectural Context, ## Proposed V1.7 Architecture > Embedding views / Indexing Additions > Dense, ## Design Decisions #5).
- Feature homes: `crates/mcp-server/src/lib.rs` (build_graph_from_pg — view text assembly + embedding), `crates/retrieval/src/orchestrator.rs` (SeededSkill view storage + config flag), `crates/retrieval/src/dual_scope.rs` (α/l1_semantic fusion seam), `crates/retrieval/src/scoring.rs` (only if fusion needs a helper — keep eq.3 shape intact). `crates/graph-builder/src/graph/build.rs` is named by the ticket but the live dense path that the sweep exercises is mcp-server's `build_graph_from_pg`; mirror there first.
- Shared/global decisions: the per-view bounded-text assembly (which fields → which view) should be a single shared helper (one source of truth, mirroring how `skill_lexical_document` centralizes the BM25 field policy), reused if both graph-builder and mcp-server need it. Do NOT duplicate the field-mapping in two places and let them drift.
- Deletion test: the dense-view machinery is concrete now; default-OFF means the only behavioral change with the flag unset is zero. The MEANINGFUL quality validation is T11's job (needs T10's multi-view-rich corpus).
- Interfaces as test surfaces: (a) view text builder = pure function over the multi-view fields (unit-testable: bounded length, correct field inclusion, empty-field handling); (b) multi-view fusion at the α term (unit-testable: max-over-views / weighted fuse, flag OFF == today's e_summary-only behavior exactly).
- Seams/adapters/contracts: dense view embeddings are recomputed at boot from PG fields and live ONLY in the in-memory `SeededSkill`/`RetrievalSnapshot` — NO write-side Qdrant/PG storage, NO migration. "Observable in graph/snapshot metadata" = record WHICH views were built (names/dims/count) in health/snapshot metadata, not new DB columns.
- Review guidance for /workflows:review later: verify eq.3 floor calibration unchanged with flag OFF; no community/scalar boost reintroduced; bounded text honors the embedding-window discipline (no full-body blob, plan line 251); fail-loud batch guards preserved; flag default-OFF.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | T09 Dense multi-view embedding views (e_task/e_needs/e_negative) | expansion | Dense channel exploits T03 fields; unblocks the full hybrid verdict in T11 | code-complete (unit green); live sweep pending | 2 | unit-01-dense-multiview-embedding-views.md |

## Execution Notes
- Owner decisions (2026-06-10):
  1. Delegate code + scoped unit tests to ONE sonnet execution-agent (project rule: execution-agents run sonnet). The agent runs only `cargo test -p retrieval` and `cargo test -p mcp-server --lib` (scoped, serial) — NOT the full real-server sweep and NOT a workspace-wide build, to keep the heavy footprint minimal (WSL2 agent-death precedent from T05).
  2. The orchestrator (not the agent) drives the live real-server ON-vs-OFF sweep after the agent returns. The current 234-skill corpus predates T03 and likely has empty multi-view fields, so the measured delta may be ~0 — record it honestly; ship views default-OFF; the meaningful validation is T11 (depends on T10 corpus).
- T03 fields that actually exist on PersistedGraphSkillRecord (rebuild.rs:161-185): use_when, avoid_when, artifacts, tools, invariants, requires, produces, subunits(title/content). There is NO separate anti_patterns or failure_modes column — map views to fields that EXIST; do not invent columns.

## Learnings Brief
- [retrieval] `BoolFlag` newtype (retrieval crate) is the reusable fail-loud boolean env-flag primitive — use it for future `RETRIEVAL_*` bools instead of ad-hoc parsing.
- [retrieval] Dense views are built UNCONDITIONALLY at mcp-server boot; the flag gates only the READ (fusion). Mirrors the T04 BM25 "build always, gate at read" pattern → ON/OFF sweep = pure mcp-server env-flip + `reboot_mcp`, no graph rebuild.
- [retrieval] α-fusion = max-over-views {e_summary, e_task, e_needs}; `e_negative` is STRUCTURALLY excluded (no `fuse_dense_views` parameter), not just by convention.
- [gate, BLOCKER carried forward] Workspace fails `cargo fmt --check` at HEAD from PRE-EXISTING T04/T05 un-formatted commits (`graph-builder/edges.rs`, `graph-builder/rebuild.rs`, `infrastructure/persistence/rebuild.rs`, `infrastructure/vector/qdrant.rs`). NOT T09. Needs a dedicated fmt cleanup before the V1.7 final gate. (Compare to the standing [[workspace-clippy-e2e-harness-deadcode-blocker]] — the fmt gate is a second pre-existing final-gate blocker.)
- [WSL2] Latency-envelope tests (`*_meets_parallel_latency_envelope`) are timing-flaky under load — rerun once before treating a single failure as a regression.
