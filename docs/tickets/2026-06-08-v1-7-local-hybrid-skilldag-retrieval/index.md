---
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
execution_shape: vertical-slices
ticket_set_status: in_progress
last_completed_batch: 5
total_batches: 16
reorg_note: "2026-06-09 — consolidated open large todos into tickets T09-T16 and re-batched (owner decision). Remaining todos are chunkable T03/T04 fixes only (#251/#252/#253/#256/#257)."
---

# Ticket Set: V1.7 Local Hybrid SkillDAG Retrieval

Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`

Architecture handoff: no separate architecture artifact exists. Use the parent plan's `## Architectural Context`, `## Design Decisions`, and `## Proposed V1.7 Architecture` as the explicit handoff. The feature homes are already named per slice; keep shared/global changes limited to adapter, persistence, protocol, and documentation boundaries.

Execution shape: `vertical-slices`

## Dependency Graph

Phase A — V1.7 retrieval core (T01-T09):
- T01 `measurement-harness-arms`: no dependencies.
- T02 `qwen3-embedder-rebuild-safety`: depends on T01.
- T03 `expanded-skill-format-multiview-fields`: depends on T02.
- T04 `hybrid-dense-bm25-candidate-generation`: depends on T01, T02; after T03.
- T05 `typed-skill-graph-edge-storage`: depends on T03.
- T09 `dense-multiview-embedding-views`: depends on T03. (NEW — the dense counterpart to T04's sparse multi-view document; the plan's `e_task`/`e_needs`/`e_negative` views, never previously ticketed.)
- T06 `skilldag-style-agent-retrieval-tools`: depends on T04, T05; benefits from T09. (Now also OWNS folded todos #255 agent-native parity + #260 relevance-score exposure.)
- T07 `optional-local-reranker-cheap-decomposition`: depends on T04; optional.
- T08 `retrieval-contract-docs-efficacy-handoff`: hard-depends T01-T06 (+T09); consumes T07 if executed. Closes Phase A and hands a measured retrieval substrate to Phase B.

Phase B — corpus, validation, efficacy (T10-T15) [efficacy promoted into the set per owner decision 2026-06-09; the plan lists it as a downstream Non-Goal]:
- T13 `convert-integration-fakes-to-live`: no deps; hygiene gate — must precede efficacy measurement. (Parallel-safe.)
- T10 `seed-corpus-self-ingestion-foundation`: depends on T03 (extraction pipeline). Foundation for everything below; first run that populates real multi-view fields.
- T11 `corpus-multiview-resweep-hybrid-validation`: depends on T10, T09, T06. Earns the real dense-vs-hybrid verdict; may flip the default (→ update T08 docs).
- T12 `trigger-aware-retrieval-priming-mode`: depends on T10 (corpus to measure).
- T14 `efficacy-task-outcome-ab-harness`: depends on T10, T13 (+T08 handoff).
- T15 `swebench-compounding-efficacy`: depends on T10, T14; also resolve #217 (cold-start) first.

Independent hardening (slot anywhere — non-retrieval feature homes, parallel-safe):
- T16 `maintenance-run-once-robustness-trio`: no deps.

## Execution Batches

- **Batch 1:** T01. Status: completed (session work-2026-06-09-070746; held-out MRR 0.767 / no-match 1.0, gate exit 0).
- **Batch 2:** T02. Status: completed (session work-2026-06-09-084626; qwen 2560-dim arm live MRR 0.767/nDCG 0.709 vs nomic 0.767/0.749 — nomic stays default; model-keyed collections; migration 008 + orphaned 007 registered).
- **Batch 3:** T03. Status: completed (session work-2026-06-09-T03; 7 optional multi-view fields wired LLM→writer→reader→skills row; migration 009 WRITE-AHEAD live-applied+skip-verified on real Postgres; real writer↔reader roundtrip green; owner-approved skills-row persistence — reader deferred to T04/T05).
- **Batch 4:** T04. Status: completed (session work-2026-06-09-T04; owner chose FULL scope: snapshot_hybrid + qdrant_hybrid both real. Backend selector + fail-loud RETRIEVAL_BACKEND; in-memory Okapi BM25; Qdrant named dense+sparse(idf) collection + write-side sparse + Query-API read arm; reboot_arm + live sweep. MEASURED held-out: snapshot_dense/snapshot_hybrid/qdrant_hybrid all MRR 0.767, p95 114/128/119ms — NO uplift from hybrid on this corpus; snapshot_dense stays default; 0.80 unmet (embedding/scoring ceiling). Selected backend path = snapshot_dense default, qdrant_hybrid experimental real (CQRS break, ADR→T08). 2 qdrant_hybrid prod bugs caught by the live run.).
**Phase A — V1.7 retrieval core:**
- **Batch 5:** T05 typed-skill-graph-edge-storage. Status: completed (session work-2026-06-10-T05; `skill_edges` migration 010 + typed `EdgeType`/`EdgeOrigin` domain semantics + deterministic cold-start proposer (`depends_on` from requires↔produces auto-committed ≥0.9; mutual→composes_with; `similar_to` from tools/artifacts Jaccard) + backbone-acyclicity/self-loop fail-loud validation + `replace_skill_edges`/`list_skill_edges` persistence wired into the rebuild write path. Owner decisions: Postgres-only storage, auto-commit high-confidence. Unit suites green: graph-builder 28, infra persistence 36, domain 13; workspace compiles. Live PG (127.0.0.1:15432) verified: migration-10 apply/skip count gate + edge roundtrip (type/origin/reason/JSONB-evidence persist, conflicts_with stored, replace semantics) both PASS; full live persistence suite green (no regression). `specializes`/`conflicts_with` auto-proposal + edge mutation-history table deferred to T06.).
- **Batch 6:** T09 dense-multiview-embedding-views. Status: in_progress — code-complete (session work-2026-06-10-T09; new `crates/retrieval/src/dense_views.rs` shared view-text helper for `e_task`/`e_needs`/`e_negative`; `SeededSkill` gains the three in-memory view embeddings; `RETRIEVAL_DENSE_VIEWS` fail-loud `BoolFlag` flag DEFAULT-OFF; α/`l1_semantic` fusion = max-over-views {e_summary,e_task,e_needs} in both the snapshot and qdrant arms, flag-OFF == byte-for-byte pre-T09 ranking; `e_negative` STRUCTURALLY excluded from positive fusion; views built unconditionally at mcp-server boot with fail-loud per-batch length guards; `DenseViewsMetadata` on the snapshot + `/health` markers; dense-views ON/OFF arm wired into the harness via `docker-compose.test.yml` passthrough + `retrieval_quality_sweep.py` arm key. Unit suites green: retrieval 78, mcp-server --lib 33. **Remaining for `completed`:** the live real-server ON-vs-OFF sweep (orchestrator-driven). Owner-acknowledged: the current 234-corpus predates T03 → expected delta ≈ 0; views stay default-OFF; meaningful multi-view validation is T11. NOTE: T04/T05 left a pre-existing `cargo fmt --check` final-gate blocker — see session learnings.). (NEW)
- **Batch 7:** T06 skilldag-style-agent-retrieval-tools (owns folded #255 + #260). Status: ready.
- **Batch 8:** T07 optional-local-reranker. Status: ready, optional.
- **Batch 9:** T08 retrieval-contract-docs-efficacy-handoff. Status: ready. Closes Phase A. (May proceed after T06/T09 if T07 is intentionally skipped.)

**Phase B — corpus, validation, efficacy:**
- **Batch 10:** T13 convert-integration-fakes-to-live. Status: ready. (Hygiene gate before efficacy; parallel-safe — may run any time in Phase A too.)
- **Batch 11:** T10 seed-corpus-self-ingestion-foundation. Status: ready.
- **Batch 12:** T11 corpus-multiview-resweep-hybrid-validation. Status: blocked (T10, T09, T06).
- **Batch 13:** T12 trigger-aware-retrieval-priming-mode. Status: blocked (T10).
- **Batch 14:** T14 efficacy-task-outcome-ab-harness. Status: blocked (T10, T13).
- **Batch 15:** T15 swebench-compounding-efficacy. Status: blocked (T10, T14; resolve #217 first).

**Independent hardening:**
- **Batch 16:** T16 maintenance-run-once-robustness-trio. Status: ready. (No deps; parallel-safe — slot anywhere.)

Batches are singleton by default (shared retrieval config / graph-builder / persistence / Qdrant / MCP / docs must stay in lockstep). Exceptions explicitly parallel-safe (different feature homes): **T13** (tests/integration) and **T16** (crates/maintenance) may run alongside any Phase-A retrieval batch.

File-overlap safety notes:

- T02/T04/T07/T09 all affect retrieval model/backend/ranking/embedding behavior — sequential.
- T03/T05 both affect graph-builder + skill data shape — sequential to avoid schema/parser drift.
- T06 must follow T04/T05 (+benefit from T09) so the agent surface reflects the real candidate/edge/multi-view models.
- T08 is documentation/handoff after Phase-A code+measurements are known; if T07 is skipped, T08 records the skip.
- T11 may flip the production default; if it does, it must update the T08 retrieval-contract doc.
- T13, T16 touch non-retrieval homes — safe to parallelize.

## Ticket Table

| Ticket | Title | Batch | Depends on | Feature home | Status |
|---|---:|---:|---|---|---|
| [T01](01-measurement-harness-arms.md) | V1.7 measurement harness arms | 1 | none | `tests/e2e` quality harness and `scripts/retrieval_quality_*` | completed |
| [T02](02-qwen3-embedder-rebuild-safety.md) | Local Qwen3 embedder backend and rebuild safety | 2 | T01 | `crates/infrastructure/src/embeddings`, `crates/graph-builder`, `crates/mcp-server` | completed |
| [T03](03-expanded-skill-format-multiview-fields.md) | Expanded skill format and multi-view extraction fields | 3 | T02 | `crates/session-extractor` and `crates/graph-builder/src/extraction` | completed |
| [T04](04-hybrid-dense-bm25-candidate-generation.md) | Hybrid dense/BM25 candidate generation | 4 | T01, T02, T03 | `crates/retrieval` | completed |
| [T05](05-typed-skill-graph-edge-storage.md) | Typed skill graph storage and cold-start edge proposals | 5 | T03 | `crates/graph-builder` and persistence graph schema | completed |
| [T09](09-dense-multiview-embedding-views.md) | Dense multi-view embedding views (e_task/e_needs/e_negative) | 6 | T03 | `crates/graph-builder`, `crates/mcp-server`, `crates/retrieval` | in_progress (code-complete; live sweep pending) |
| [T06](06-skilldag-style-agent-retrieval-tools.md) | SkillDAG-style agent retrieval tools (owns folded #255, #260) | 7 | T04, T05 | `crates/mcp-server/src/tools` and `crates/retrieval` | ready |
| [T07](07-optional-local-reranker-cheap-decomposition.md) | Optional local reranker and cheap query decomposition | 8 | T04 | `crates/retrieval` | ready |
| [T08](08-retrieval-contract-docs-efficacy-handoff.md) | Retrieval contract docs and efficacy handoff | 9 | T01-T06 hard (+T09); T07 optional | `docs/reference` and `docs/assessments` | ready |
| [T13](13-convert-integration-fakes-to-live.md) | Convert integration fakes → live (drain allowlist) | 10 | none | `tests/integration` and no-fakes guard | ready |
| [T10](10-seed-corpus-self-ingestion-foundation.md) | Seed ≥200-skill corpus by dogfooding ingestion (was #216) | 11 | T03 | ingestion pipeline end-to-end | ready |
| [T11](11-corpus-multiview-resweep-hybrid-validation.md) | Multi-view re-sweep — validate the hybrid bet (was #259) | 12 | T10, T09, T06 | `tests/e2e` quality harness, `scripts/retrieval_quality_*` | blocked |
| [T12](12-trigger-aware-retrieval-priming-mode.md) | Trigger-aware retrieval — priming mode (was #220) | 13 | T10 | `crates/retrieval`, `crates/compiler` | blocked |
| [T14](14-efficacy-task-outcome-ab-harness.md) | Efficacy A/B harness — layer ON vs OFF (was #205) | 14 | T10, T13 | efficacy harness over live stack | blocked |
| [T15](15-swebench-compounding-efficacy.md) | SWE-bench Lite compounding efficacy (was #218) | 15 | T10, T14 | efficacy harness + SWE-bench integration | blocked |
| [T16](16-maintenance-run-once-robustness-trio.md) | Maintenance run-once robustness trio (was #222) | 16 | none | `crates/maintenance` | ready |

## Blockers

- Qdrant hot-path promotion is approval-sensitive because it changes the current CQRS resilience contract.
- Embedding model changes and schema migrations are approval-sensitive under the constitution.

Execution notes:

- A separate architecture artifact does not exist. The parent plan has enough explicit architecture handoff to proceed, but execution agents should not invent answers to the open architecture questions.
- The project-local `ticket-flow-auditor` was loaded from `.github/agents/ticket-flow-auditor.agent.md` and dispatched after ticket generation.

## Review Summary

### Blocking gaps

- None after repair. The audit initially found stale blocked status, weak evidence commands, and T08's hard dependency on optional T07; those were repaired in this ticket set.
- Final targeted re-audit found no remaining blocking gaps after T04's backend-selectable evidence command was repaired.

### Recommendations

- Run `$workflows-architecture` before implementation if the team wants a durable ADR-level decision on Qdrant hot-path promotion versus snapshot-hybrid fallback.
- Keep T04 behind a flag until the real-server quality and p95 latency report proves the backend should become default.
- Revisit T07 only after T04 measurements; local reranking should remain optional unless it earns its runtime cost.
- T04 completion evidence must record the selected backend path: `qdrant_hybrid` versus `snapshot_hybrid`, Qdrant version support, and health semantics.
- T06 may touch `crates/compiler/src/` only if the execution explicitly documents a measured `compile_context` change; otherwise keep compiler behavior unchanged.
- If T07 is intentionally skipped, record the skip in the index/session evidence before executing T08 so progress tracking stays unambiguous.
