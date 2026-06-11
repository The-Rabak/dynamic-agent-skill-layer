---
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
execution_shape: vertical-slices
ticket_set_status: in_progress
last_completed_batch: 10
total_batches: 17
amendment_note: "2026-06-11 — owner-directed Phase B tightening from docs/assessments/2026-06-11-v1-7-midpoint-deep-grok-assessment.md: T11 made instrument-first (negative-control gate, paired diagnostics, candidate-recall metric, anti-circularity fixture rule, conditional lexical-ranking arm); T12 gains T11 dep + pre-registered signal ROI; T14 gains pre-registration/paired-design/placebo-arm/attribution; T15 gains minimum-detectable-effect pre-registration; NEW T17 mcp-server boot-readiness honesty. T13 intentionally untouched (in-flight session)."
phase_a_status: "closed 2026-06-11 (T01-T09 done; T07 skipped). Efficacy substrate handed to Phase B with explicit honest gaps — see docs/assessments/2026-06-11-v1-7-retrieval-contract-measured.md. The frozen 0.80 MRR/nDCG target is NOT yet validated on the qwen3 262-corpus; held-out measurement is blocked on a corpus-aligned eval fixture (T11)."
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
- T11 `corpus-multiview-resweep-hybrid-validation`: depends on T10, T09, T06. **Amended 2026-06-11: instrument-first** — α=0 negative-control gate before any verdict, anti-circularity fixture rule (use_when-derived queries demoted), per-query paired diagnostics, candidate-recall@limit as first-class metric, MRR@10 resolution arm, conditional env-gated lexical-ranking arm (the structural reason all arms tied: candidate gen can't affect eq.3 ranking — see ticket). Earns the real dense-vs-hybrid verdict; may flip the default (→ update T08 docs).
- T12 `trigger-aware-retrieval-priming-mode`: depends on T10, **T11 (amended 2026-06-11 — signal ROI must be measured on T11's validated instrument; session-start query stratum + pre-registered thresholds)**.
- T14 `efficacy-task-outcome-ab-harness`: depends on T10, T13 (+T08 handoff). **Amended 2026-06-11: pre-registered pass criterion, paired per-task design, placebo arm (matched-mass irrelevant context), per-pull attribution, PASS/FAIL/UNDERPOWERED outcomes.**
- T15 `swebench-compounding-efficacy`: depends on T10, T14; also resolve #217 (cold-start) first. **Amended 2026-06-11: minimum-detectable-effect committed in advance; UNDERPOWERED is a reportable outcome.**

Independent hardening (slot anywhere — non-retrieval feature homes, parallel-safe):
- T16 `maintenance-run-once-robustness-trio`: no deps.
- T17 `mcp-server-boot-readiness-honesty`: no deps (NEW 2026-06-11). /health must not claim ready during the qwen3 boot re-embed window (~7 min); load precomputed vectors at boot. **Sequencing: do not execute until the in-flight T13 session lands (shared crates/mcp-server files).** Strongly recommended before T11's measured sweeps (until then T11 gates on a probe query, not /health).

## Execution Batches

- **Batch 1:** T01. Status: completed (session work-2026-06-09-070746; held-out MRR 0.767 / no-match 1.0, gate exit 0).
- **Batch 2:** T02. Status: completed (session work-2026-06-09-084626; qwen 2560-dim arm live MRR 0.767/nDCG 0.709 vs nomic 0.767/0.749 — nomic stays default; model-keyed collections; migration 008 + orphaned 007 registered).
- **Batch 3:** T03. Status: completed (session work-2026-06-09-T03; 7 optional multi-view fields wired LLM→writer→reader→skills row; migration 009 WRITE-AHEAD live-applied+skip-verified on real Postgres; real writer↔reader roundtrip green; owner-approved skills-row persistence — reader deferred to T04/T05).
- **Batch 4:** T04. Status: completed (session work-2026-06-09-T04; owner chose FULL scope: snapshot_hybrid + qdrant_hybrid both real. Backend selector + fail-loud RETRIEVAL_BACKEND; in-memory Okapi BM25; Qdrant named dense+sparse(idf) collection + write-side sparse + Query-API read arm; reboot_arm + live sweep. MEASURED held-out: snapshot_dense/snapshot_hybrid/qdrant_hybrid all MRR 0.767, p95 114/128/119ms — NO uplift from hybrid on this corpus; snapshot_dense stays default; 0.80 unmet (embedding/scoring ceiling). Selected backend path = snapshot_dense default, qdrant_hybrid experimental real (CQRS break, ADR→T08). 2 qdrant_hybrid prod bugs caught by the live run.).
**Phase A — V1.7 retrieval core:**
- **Batch 5:** T05 typed-skill-graph-edge-storage. Status: completed (session work-2026-06-10-T05; `skill_edges` migration 010 + typed `EdgeType`/`EdgeOrigin` domain semantics + deterministic cold-start proposer (`depends_on` from requires↔produces auto-committed ≥0.9; mutual→composes_with; `similar_to` from tools/artifacts Jaccard) + backbone-acyclicity/self-loop fail-loud validation + `replace_skill_edges`/`list_skill_edges` persistence wired into the rebuild write path. Owner decisions: Postgres-only storage, auto-commit high-confidence. Unit suites green: graph-builder 28, infra persistence 36, domain 13; workspace compiles. Live PG (127.0.0.1:15432) verified: migration-10 apply/skip count gate + edge roundtrip (type/origin/reason/JSONB-evidence persist, conflicts_with stored, replace semantics) both PASS; full live persistence suite green (no regression). `specializes`/`conflicts_with` auto-proposal + edge mutation-history table deferred to T06.).
- **Batch 6:** T09 dense-multiview-embedding-views. Status: **completed (code delivered; measured sweep AC delegated to T11)** — owner decision 2026-06-11 (session work-2026-06-11): the live mcp-server now serves the T10 262-skill qwen3 corpus, but the only labeled eval fixture (`retrieval_quality_234_corpus_labeled.json`) is **0/30 aligned** with that corpus (verified via PG join `30|0`), so an ON/OFF sweep on it is a guaranteed-0 (fake) result. The aligned-fixture sweep that produces the real dense-views ON/OFF delta is T11's deliverable; dense-views stays default-OFF until then. Original code-complete note follows. (session work-2026-06-10-T09; new `crates/retrieval/src/dense_views.rs` shared view-text helper for `e_task`/`e_needs`/`e_negative`; `SeededSkill` gains the three in-memory view embeddings; `RETRIEVAL_DENSE_VIEWS` fail-loud `BoolFlag` flag DEFAULT-OFF; α/`l1_semantic` fusion = max-over-views {e_summary,e_task,e_needs} in both the snapshot and qdrant arms, flag-OFF == byte-for-byte pre-T09 ranking; `e_negative` STRUCTURALLY excluded from positive fusion; views built unconditionally at mcp-server boot with fail-loud per-batch length guards; `DenseViewsMetadata` on the snapshot + `/health` markers; dense-views ON/OFF arm wired into the harness via `docker-compose.test.yml` passthrough + `retrieval_quality_sweep.py` arm key. Unit suites green: retrieval 78, mcp-server --lib 33. **Remaining for `completed`:** the live real-server ON-vs-OFF sweep (orchestrator-driven). Owner-acknowledged: the current 234-corpus predates T03 → expected delta ≈ 0; views stay default-OFF; meaningful multi-view validation is T11. NOTE: T04/T05 left a pre-existing `cargo fmt --check` final-gate blocker — see session learnings.). (NEW)
- **Batch 7:** T06 skilldag-style-agent-retrieval-tools (owns folded #255 + #260). Status: **completed** (session work-2026-06-11-T06; commits 32cd968 + live-test hardening). New `search_skill_graph` tool returns separate matches/neighbors/conflicts (from T05 typed edges, filtered to the matched-skill neighborhood, correct in/outbound direction, fail-loud on edge-read error); `find_skill` gains `rationale` + relevance-meaningful `score` (#260, threaded `semantic_score` on `ScoredSkill`) + `fusion_rank_score` provenance; 7 multi-view fields readable via `inspect_skill` (#255 P1-A, JSON-RPC round-trip asserted); `/health` surfaces `retrieval_backend` (#255 P2-C/D); `retrieval_context{embedding_model,collection,graph_version}` on both tools (#243). Live proof: **3/3 `--ignored test_skill_graph_tools` PASS** vs the real 262-skill qwen3 mcp-server. **#260 validated live** (score 0.836/0.740/0.748 vs RRF 0.0164). Unit/integration green: retrieval 78, mcp-server lib 40, admin integration 6. Orchestrator fix cycle caught + fixed: whole-graph edge dump, silent edge-read swallow, fragile score string-parse, dead test code. compiler untouched. No new migrations.
- **Batch 8:** T07 optional-local-reranker. Status: **skipped** (owner decision 2026-06-11). Optional-by-design + gated on earning its runtime cost; T04 measured zero candidate-gen uplift (ceiling = embedding/scoring, not candidate gen) and the user wants retrieval fast. Skip recorded per the index instruction so T08 documents reranker/decomposition as not-shipped/experimental. Revisit only if a corpus-aligned sweep (T11) reveals a closeable ranking gap within p95.
- **Batch 9:** T08 retrieval-contract-docs-efficacy-handoff. Status: **completed** (session work-2026-06-11-T08; T07 intentionally skipped). Reconciled `retrieval-contract.md` (+§0 V1.7 delta) + `online-retrieval-cqrs.md` (qdrant_hybrid query-time read exception) + new `docs/assessments/2026-06-11-v1-7-retrieval-contract-measured.md`. Honest gap recorded: the live held-out quality gate is un-runnable on the dogfood corpus (234-fixture 0/30 aligned) → 0.80 NOT validated, measured gate → T11; not faked. **Closes Phase A.**

**Phase B — corpus, validation, efficacy:**
- **Batch 10:** T13 convert-integration-fakes-to-live. Status: **completed** (session work-2026-06-11-T13; policy=relocate-or-live). All 9 fake-bearing files relocated from tests/integration into their owning crates' test-only code (crates/{mcp-server,graph-builder,maintenance}/tests/ + src/#[cfg(test)] where applicable); allowlist drained empty; guard Zone 3 now hard-fails (verified via probe) with the test-location taxonomy documented explicitly. CapturingEventPublisher/fault-injection providers recorded as acceptable observers (test_extract_session stays). 49 relocated tests pass (+2 ignored live-PG); no regression. Orchestrator caught+fixed an agent guard-blind-spot (now explicit policy) and a real inlined-test bug (lib.rs self-inspection) that the agent had hidden by skipping --features test-utils. **Unblocks T17** (was sequenced to wait on this in-flight session).
- **Batch 11:** T10 seed-corpus-self-ingestion-foundation. Status: completed (session work-2026-06-10-T10; 24 genuine dev sessions → 262 skills via real pipeline, 71% multi-view, 60 communities; `skill_layer_test` + `skills__qwen3-embedding-4b`; report: `tests/e2e/reports/replica-run/VALIDATION-REPORT.md`).
- **Batch 12:** T11 corpus-multiview-resweep-hybrid-validation. Status: **ready** (deps T10 ✅, T09 ✅, T06 ✅ all met). **Now also absorbs T09's deferred sweep AC:** T11 must build a 262-corpus-aligned labeled eval fixture (the committed `234_corpus` fixture is 0/30 aligned with the live corpus) and run the dense-views ON/OFF sweep as part of the dense-vs-hybrid verdict. **Amended 2026-06-11 (instrument-first):** α=0 negative-control gate before any arm verdict; anti-circularity fixture rule (headline queries from held-out transcript problem statements, use_when-derived only as secondary stratum — owner decision recorded in the ticket); paired per-query rank diagnostics + sign-test verdicts; candidate-recall@limit reported per arm; MRR@10 resolution arm; conditional env-gated lexical-ranking arm if rankings tie again; floor re-calibration on qwen3.
- **Batch 13:** T12 trigger-aware-retrieval-priming-mode. Status: blocked (T11 — amended dep 2026-06-11; T10 ✅).
- **Batch 14:** T14 efficacy-task-outcome-ab-harness. Status: blocked (T10 ✅, T13). **Amended 2026-06-11:** pre-registered pass criterion, paired design + sign test, placebo arm, per-pull attribution, PASS/FAIL/UNDERPOWERED.
- **Batch 15:** T15 swebench-compounding-efficacy. Status: blocked (T10, T14; resolve #217 first). **Amended 2026-06-11:** minimum-detectable-effect pre-registered; UNDERPOWERED reportable.

**Independent hardening:**
- **Batch 16:** T16 maintenance-run-once-robustness-trio. Status: ready. (No deps; parallel-safe — slot anywhere.)
- **Batch 17:** T17 mcp-server-boot-readiness-honesty. Status: ready (NEW 2026-06-11). No ticket deps, but **must not execute until the in-flight T13 session lands** (shared `crates/mcp-server` files). Best slotted before T11's measured sweeps.

Batches are singleton by default (shared retrieval config / graph-builder / persistence / Qdrant / MCP / docs must stay in lockstep). Exceptions explicitly parallel-safe (different feature homes): **T13** (tests/integration) and **T16** (crates/maintenance) may run alongside any Phase-A retrieval batch.

File-overlap safety notes:

- T02/T04/T07/T09 all affect retrieval model/backend/ranking/embedding behavior — sequential.
- T03/T05 both affect graph-builder + skill data shape — sequential to avoid schema/parser drift.
- T06 must follow T04/T05 (+benefit from T09) so the agent surface reflects the real candidate/edge/multi-view models.
- T08 is documentation/handoff after Phase-A code+measurements are known; if T07 is skipped, T08 records the skip.
- T11 may flip the production default; if it does, it must update the T08 retrieval-contract doc.
- T13, T16 touch non-retrieval homes — safe to parallelize.
- T17 touches `crates/mcp-server`, which the in-flight T13 session is also modifying (tests relocated into `crates/mcp-server/tests/`, `src/lib.rs` edits) — T17 must NOT start until that session's work lands.
- T11's conditional lexical-ranking arm touches `crates/retrieval` scoring — if exercised, T11 must not run concurrently with T12's retrieval changes (T12 already hard-depends on T11, so the ordering holds).

## Ticket Table

| Ticket | Title | Batch | Depends on | Feature home | Status |
|---|---:|---:|---|---|---|
| [T01](01-measurement-harness-arms.md) | V1.7 measurement harness arms | 1 | none | `tests/e2e` quality harness and `scripts/retrieval_quality_*` | completed |
| [T02](02-qwen3-embedder-rebuild-safety.md) | Local Qwen3 embedder backend and rebuild safety | 2 | T01 | `crates/infrastructure/src/embeddings`, `crates/graph-builder`, `crates/mcp-server` | completed |
| [T03](03-expanded-skill-format-multiview-fields.md) | Expanded skill format and multi-view extraction fields | 3 | T02 | `crates/session-extractor` and `crates/graph-builder/src/extraction` | completed |
| [T04](04-hybrid-dense-bm25-candidate-generation.md) | Hybrid dense/BM25 candidate generation | 4 | T01, T02, T03 | `crates/retrieval` | completed |
| [T05](05-typed-skill-graph-edge-storage.md) | Typed skill graph storage and cold-start edge proposals | 5 | T03 | `crates/graph-builder` and persistence graph schema | completed |
| [T09](09-dense-multiview-embedding-views.md) | Dense multi-view embedding views (e_task/e_needs/e_negative) | 6 | T03 | `crates/graph-builder`, `crates/mcp-server`, `crates/retrieval` | completed (code; measured sweep AC → T11) |
| [T06](06-skilldag-style-agent-retrieval-tools.md) | SkillDAG-style agent retrieval tools (owns folded #255, #260) | 7 | T04, T05 | `crates/mcp-server/src/tools` and `crates/retrieval` | completed |
| [T07](07-optional-local-reranker-cheap-decomposition.md) | Optional local reranker and cheap query decomposition | 8 | T04 | `crates/retrieval` | skipped |
| [T08](08-retrieval-contract-docs-efficacy-handoff.md) | Retrieval contract docs and efficacy handoff | 9 | T01-T06 hard (+T09); T07 optional | `docs/reference` and `docs/assessments` | completed |
| [T13](13-convert-integration-fakes-to-live.md) | Convert integration fakes → live (drain allowlist) | 10 | none | `tests/integration` and no-fakes guard | completed |
| [T10](10-seed-corpus-self-ingestion-foundation.md) | Seed ≥200-skill corpus by dogfooding ingestion (was #216) | 11 | T03 | ingestion pipeline end-to-end | completed |
| [T11](11-corpus-multiview-resweep-hybrid-validation.md) | Multi-view re-sweep — validate the hybrid bet (was #259; amended 2026-06-11 instrument-first) | 12 | T10, T09, T06 | `tests/e2e` quality harness, `scripts/retrieval_quality_*` (+ conditional `crates/retrieval` lexical arm) | ready |
| [T12](12-trigger-aware-retrieval-priming-mode.md) | Trigger-aware retrieval — priming mode (was #220; amended 2026-06-11) | 13 | T10, T11 | `crates/retrieval`, `crates/compiler` | blocked |
| [T14](14-efficacy-task-outcome-ab-harness.md) | Efficacy A/B harness — layer ON vs OFF (was #205; amended 2026-06-11) | 14 | T10, T13 | efficacy harness over live stack | blocked |
| [T15](15-swebench-compounding-efficacy.md) | SWE-bench Lite compounding efficacy (was #218; amended 2026-06-11) | 15 | T10, T14 | efficacy harness + SWE-bench integration | blocked |
| [T16](16-maintenance-run-once-robustness-trio.md) | Maintenance run-once robustness trio (was #222) | 16 | none | `crates/maintenance` | ready |
| [T17](17-mcp-server-boot-readiness-honesty.md) | mcp-server boot readiness honesty (NEW 2026-06-11) | 17 | none (sequence after in-flight T13 session) | `crates/mcp-server`, `crates/infrastructure` | ready |

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
