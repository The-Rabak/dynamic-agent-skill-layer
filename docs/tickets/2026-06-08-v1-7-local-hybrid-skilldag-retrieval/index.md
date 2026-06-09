---
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
execution_shape: vertical-slices
ticket_set_status: in_progress
last_completed_batch: 2
total_batches: 8
---

# Ticket Set: V1.7 Local Hybrid SkillDAG Retrieval

Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`

Architecture handoff: no separate architecture artifact exists. Use the parent plan's `## Architectural Context`, `## Design Decisions`, and `## Proposed V1.7 Architecture` as the explicit handoff. The feature homes are already named per slice; keep shared/global changes limited to adapter, persistence, protocol, and documentation boundaries.

Execution shape: `vertical-slices`

## Dependency Graph

- T01 `measurement-harness-arms`: no dependencies.
- T02 `qwen3-embedder-rebuild-safety`: depends on T01.
- T03 `expanded-skill-format-multiview-fields`: depends on T02.
- T04 `hybrid-dense-bm25-candidate-generation`: depends on T01 and T02; should execute after T03 so hybrid can consume structured fields.
- T05 `typed-skill-graph-edge-storage`: depends on T03.
- T06 `skilldag-style-agent-retrieval-tools`: depends on T04 and T05.
- T07 `optional-local-reranker-cheap-decomposition`: depends on T04; optional enhancement.
- T08 `retrieval-contract-docs-efficacy-handoff`: hard-depends on T01-T06; consumes T07 only if T07 was executed, otherwise documents reranking/decomposition as skipped or experimental.

## Execution Batches

- **Batch 1:** T01. Status: completed (session work-2026-06-09-070746; held-out MRR 0.767 / no-match 1.0, gate exit 0).
- **Batch 2:** T02. Status: completed (session work-2026-06-09-084626; qwen 2560-dim arm live MRR 0.767/nDCG 0.709 vs nomic 0.767/0.749 — nomic stays default; model-keyed collections; migration 008 + orphaned 007 registered).
- **Batch 3:** T03. Status: pending.
- **Batch 4:** T04. Status: pending.
- **Batch 5:** T05. Status: pending.
- **Batch 6:** T06. Status: pending.
- **Batch 7:** T07. Status: pending, optional enhancement.
- **Batch 8:** T08. Status: pending. May proceed after T06 if T07 is intentionally skipped.

All batches are singleton. Parallel grouping was intentionally avoided because the work touches shared retrieval config, graph-builder parsing, persistence schema, Qdrant adapter behavior, MCP protocol surfaces, and docs that must stay in lockstep.

File-overlap safety notes:

- No multi-ticket batch exists, so there is no parallel file overlap to prove.
- T02/T04/T07 all affect retrieval model/backend/ranking behavior and must remain sequential.
- T03/T05 both affect graph-builder and skill data shape; keep sequential to avoid schema/parser drift.
- T06 must follow T04/T05 so the agent-facing tool surface reflects the actual candidate and edge models.
- T08 is final documentation/assessment only after required code behavior and measurements are known. If T07 is skipped, T08 records that skip instead of waiting on it.

## Ticket Table

| Ticket | Title | Batch | Depends on | Feature home | Status |
|---|---:|---:|---|---|---|
| [T01](01-measurement-harness-arms.md) | V1.7 measurement harness arms | 1 | none | `tests/e2e` quality harness and `scripts/retrieval_quality_*` | completed |
| [T02](02-qwen3-embedder-rebuild-safety.md) | Local Qwen3 embedder backend and rebuild safety | 2 | T01 | `crates/infrastructure/src/embeddings`, `crates/graph-builder`, `crates/mcp-server` | completed |
| [T03](03-expanded-skill-format-multiview-fields.md) | Expanded skill format and multi-view extraction fields | 3 | T02 | `crates/session-extractor` and `crates/graph-builder/src/extraction` | ready |
| [T04](04-hybrid-dense-bm25-candidate-generation.md) | Hybrid dense/BM25 candidate generation | 4 | T01, T02, T03 | `crates/retrieval` | ready |
| [T05](05-typed-skill-graph-edge-storage.md) | Typed skill graph storage and cold-start edge proposals | 5 | T03 | `crates/graph-builder` and persistence graph schema | ready |
| [T06](06-skilldag-style-agent-retrieval-tools.md) | SkillDAG-style agent retrieval tools | 6 | T04, T05 | `crates/mcp-server/src/tools` and `crates/retrieval` | ready |
| [T07](07-optional-local-reranker-cheap-decomposition.md) | Optional local reranker and cheap query decomposition | 7 | T04 | `crates/retrieval` | ready |
| [T08](08-retrieval-contract-docs-efficacy-handoff.md) | Retrieval contract docs and efficacy handoff | 8 | T01-T06 hard; T07 optional | `docs/reference` and `docs/assessments` | ready |

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
