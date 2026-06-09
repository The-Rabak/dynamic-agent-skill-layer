---
ticket_id: T07
title: Crash-safe transcript reconciliation (the level-triggered guarantee)
kind: hardening # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: superseded # ready | in_progress | blocked | completed | superseded
superseded_by: todo-103 # folded into the durable PG transcript-ingest queue
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.4: Crash-safe transcript reconciliation"
feature_home: crates/maintenance
depends_on: [T04, T05]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-B (self-growing trigger exists)
files:
  - crates/maintenance/src/runtime.rs
  - crates/maintenance/src/transcript_reconcile.rs
  - crates/infrastructure/migrations/003_processed_transcripts.sql
test_command: cargo test -p maintenance
tdd_mode: inherit
---

# Crash-safe transcript reconciliation (the level-triggered guarantee)

> **SUPERSEDED — folded into todo 103 (durable PG transcript-ingest queue), 2026-06-01.**
> The crash-safe guarantee this ticket specified is now delivered by the queue, not a
> filesystem-scan reconcile:
> - **Marker table** (`003_processed_transcripts.sql`, `session_id, content_hash, processed_at`)
>   → folded into the content-bearing `transcript_ingest_queue`
>   (`crates/infrastructure/migrations/002_transcript_ingest_queue.sql`). The queue row IS the
>   marker; dedup is keyed on `content_hash`.
> - **`reconcile_transcripts()` FS-scan pass** → replaced by the queue **drain** in
>   `crates/maintenance/src/transcript_drain.rs`, wired into the maintenance cron loop **and**
>   worker startup (`crates/maintenance/src/runtime.rs`), bounded per sweep, retry/park on failure.
> - **`MCP_TRANSCRIPT_RECONCILE=off`** rollback flag → `MAINTENANCE_TRANSCRIPT_DRAIN=off`.
> - **Crash-safety belt-and-suspenders decision:** the original FS-scan caught sessions that
>   fired *no* hook. The queue is push-only, so that residual gap (host killed before *any* of
>   `PreCompact`/`SessionEnd` fires) is **not** covered. Decision: **accept the gap for v1.5**,
>   mitigated by **dual capture** at PreCompact + SessionEnd; do **not** reintroduce a host
>   `~/.claude` filesystem mount + scan (Option 4 deliberately removed that fragility). Revisit
>   an optional FS-scan sweep post-v1.5 if field data shows the gap matters.
>
> See `todos/103-...md` and `docs/reference/transcript-ingress.md`.

## Serves
- **SC-V1.5-B** — closes the crash hole: a session killed before `SessionEnd` fires is still extracted, exactly once, with no duplicate drafts for hook-handled sessions.
- Plan SC-3; constitution Principle 3 (human-gate intact).

## Scope
Add one idempotent `reconcile_transcripts()` pass run from the maintenance cron loop AND once at maintenance-worker startup. It detects quiescent, unprocessed transcripts (anti-join against a persisted marker), invokes the existing `extract_session` flow (`.pending` only), and records the marker. Includes the human-gated `003_processed_transcripts.sql` marker table.

- **Owns:** the transcript reconciler + its marker table + cron/startup wiring.
- **Non-goals:** changing the `SessionEnd` hook (T04), changing extraction internals (T05), end-marker detection (mtime quiescence is sufficient).

## Scope Fence
Read-only on transcripts; only writes `.pending` (via the existing gated path) + the marker row; reuses `extract_session` — no new extraction logic. Never auto-approves.

## Acceptance Criteria
- [ ] A session killed before clean exit is extracted by the reconciler (cron and/or startup), producing `.pending`.
- [ ] A session already handled by the `SessionEnd` hook is NOT re-extracted (marker-based dedup; no duplicate `.pending`).
- [ ] A still-active session (recently-written transcript) is NOT extracted (mtime-quiescence window, ~10 min, respected).
- [ ] Dedup is keyed on the persisted `processed_transcripts` marker, never on `.pending`/approved file existence (those are human-mutable — side-effect dedup is the anti-pattern).
- [ ] Per-sweep work is bounded (cap N transcripts); a large post-downtime backlog drains across cycles without stampeding the extractor.
- [ ] Run from **two** places sharing one function: (a) the existing maintenance cron loop (~10–15 min), (b) once at maintenance-worker startup (laptop-was-closed catch-up).
- [ ] `003_processed_transcripts.sql` (`session_id, content_hash, processed_at`) is staged for human approval (HUMAN GATE).
- [ ] `MCP_TRANSCRIPT_RECONCILE=off` flag carries `// TODO(remove-after-v1.5-green)` + removal criterion.

## Shared / Global Notes
- **Graph schema migration — HUMAN GATE:** `003_processed_transcripts.sql` is staged and confirmed before applying.
- Model the reconciler on `graph-builder`'s `WatcherRecovery` set-reconcile (existing idempotent pattern) — reuse the shape, do not invent a new one.
- Reuses the `extract_session` path delivered/hardened by T04+T05; do not duplicate extraction logic.

## Local Context
**WHY:** `SessionEnd` is an edge trigger that does not fire on crash/SIGKILL. Per the Kubernetes control-loop principle, edge triggers are optimizations; the level-triggered reconcile loop is the guarantee. Crash-recovery reconciliation does not exist today — a "close the loop" release needs it or the loop has a designed-in hole. The `maintenance` cron already exists, so this is one new pass, not a subsystem.

**Open question to surface:** confirm `CLAUDE_TRANSCRIPT_ROOT` is the canonical transcript dir env and that `extract_session` can be invoked from `maintenance` without pulling an mcp-server dependency cycle; if a cycle appears, flag it.

## Parent Refs
- Plan → Slice 2.4; Architecture artifact.
- Source packet: `## Execution Slices > Slice 2.4`.

## Deeper-Dive Refs
- Plan §Deepening Research Insights §2.4 (Kubernetes edge-vs-level; WatcherRecovery model; mtime quiescence; persisted-marker dedup; Brandur idempotency-key guidance).
- Plan Open Question 5 (answered: build now).

## Coupling Notes
One unit because the reconciler function, its marker table, and the cron+startup wiring are a single guarantee — the function without the marker table cannot dedup, and without both call sites it misses either steady-state or post-downtime cases. Hard-depends on T04 (the `SessionEnd`/`extract_session` contract it backstops) and T05 (reliable extraction it invokes). Singleton batch: shares `maintenance/src/runtime.rs` with T06.
