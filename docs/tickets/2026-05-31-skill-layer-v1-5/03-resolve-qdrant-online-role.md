---
ticket_id: T03
title: Resolve Qdrant's online role (Option A + CQRS docs + honest health)
kind: hardening # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: completed # ready | in_progress | blocked | completed
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 1.3: Resolve Qdrant's online role"
feature_home: crates/retrieval
depends_on: [T01]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-F (no production stub paths remain)
files:
  - crates/retrieval/src/orchestrator.rs
  - crates/retrieval/src/dual_scope.rs
  - crates/infrastructure/src/vector/qdrant.rs
  - crates/infrastructure/src/health.rs
  - docs/architecture/adr-0001-online-graph-source-v1-5.md
  - docs/reference/online-retrieval-cqrs.md
test_command: cargo test -p retrieval
tdd_mode: inherit
---

# Resolve Qdrant's online role (Option A + CQRS docs + honest health)

## Serves
- **SC-V1.5-F** — eliminate the write-only-Qdrant ambiguity and the false `qdrant: "ok"` health claim on the in-memory read path.
- Plan SC-2/SC-8 (scalable dual-scope retrieval disposition).

## Scope
Ratify Option A in writing: Qdrant is the durable write-side store (CQRS read model = the refreshable in-memory snapshot). Fix health markers that lie, write the ADR + a CQRS read-model note, and **define** (not implement) the DS-003 expectation that Slice 3.3/T10 honors.

- **Owns:** the online vector-source disposition + its documentation + honest health markers.
- **Non-goals:** implementing live Qdrant query (Option B = V2); team/remote Qdrant; the DS-003 *test rewrite* itself (that is T10).

## Scope Fence
Must not introduce remote scope or cross-tenant surface (V2). Must not query Qdrant online (Option A keeps the read path on the in-memory snapshot). No eager per-request Qdrant liveness check (would breach <500ms + re-couple read→Qdrant).

## Acceptance Criteria
- [ ] No code path implies Qdrant is queried online when it is not (Option A documented; the in-memory cosine path in `search_qdrant` is labelled as such).
- [ ] Retrieval correctness unchanged (T09 fixtures still pass).
- [ ] **`healthy_markers()` / `degraded_marker()` (`orchestrator.rs:168–186`) no longer claim `qdrant: "ok"` (nor `postgres: "ok"`) on the read path** — drop the key or relabel `qdrant: "write-store-only"` / `qdrant_write_side`. Add a named test asserting the false claim cannot reappear. *(Fix is ~2 lines at `orchestrator.rs:168,180`.)*
- [ ] The mcp-server `/health` checker (`infrastructure/src/health.rs:205–213`) does not present Qdrant as a read-path dependency (relabel `qdrant_write_store`).
- [ ] ADR `adr-0001-online-graph-source-v1-5.md` written: context / decision = Option A / consequences (5000 cap, Qdrant unused at read time) / V2 trigger for Option B (cap exceeded or team-scope) + a one-paragraph CQRS read-model note in `docs/reference/`. Health should report `qdrant_write_side` (durable store reachable) and `skill_snapshot_sync` (age since last rebuild).
- [ ] **DS-003 expectation DEFINED (RATIFIED — honored in T10):** under Option A, stopping Qdrant must NOT degrade `compile_context`. Contract: Qdrant down ⇒ `compile_context` still returns `Ok`/`NoMatch`, only the **write-side** marker (`qdrant_write_side`) shows degraded. Record this as the contract DS-003 will be rewritten to (positive CQRS-resilience proof, not `#[ignore]`). See WHY Reassessment R-5.

## Shared / Global Notes
- **Shared adapters touched:** `infrastructure/src/vector/qdrant.rs` and `infrastructure/src/health.rs` are cross-feature infra — change only the labelling/role semantics, not the operational client behavior.
- **Docs are on-demand context** — the ADR + CQRS note are the durable record; do not inline the full rationale into health code.
- Human-gate: none.

## Local Context
**WHY:** graph-builder drains the outbox into Qdrant, but online retrieval scores against the in-memory snapshot, so `qdrant: "ok"` has been false since T03 (git-history). The assessment flagged the read/write inconsistency. Option A is ratified; this ticket removes the lie and records the decision so the system is honest, and seeds the DS-003 rewrite T10 performs.

**Open question to surface:** confirm the exact health-marker call sites (`orchestrator.rs:168/180`, `health.rs:205–213`) match current line numbers before editing; if drifted, locate via `semble search "health markers qdrant ok"`.

## Parent Refs
- Plan → Slice 1.3; Architecture artifact.
- Source packet: `## Execution Slices > Slice 1.3`.

## Deeper-Dive Refs
- Plan §Deepening Research Insights §1.3 (ADR skeleton, CQRS note, health wording; git-history bonus on `search_qdrant`).
- Plan WHY Reassessment R-5 (DS-003 vs Option A → rewrite).
- Decision: Ratified Decisions #1 (Option A; Option B = V2 migration behind unchanged `SkillRetriever`).

## Coupling Notes
One unit because the health-marker fix, the disposition decision, and the ADR/CQRS docs all express a single decision (Qdrant = write-side store) and would be incoherent if split. Hard-depends on T01 only (needs the renamed snapshot type for the read-path labelling). Parallel-safe with T04 in Batch 2: code files are disjoint (retrieval/infra vs config/docs-reference/tools) and the two tickets write **different** doc files (T03 → ADR + `online-retrieval-cqrs.md`; T04 → `capability-catalog.md` + runbooks).
