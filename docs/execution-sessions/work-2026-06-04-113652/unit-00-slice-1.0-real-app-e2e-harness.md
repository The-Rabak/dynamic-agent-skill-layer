---
unit: "Slice 1.0 — Proper real-app E2E harness (drives real containers; per-stage logs)"
unit_number: 0
unit_kind: tracer-bullet/foundation
serves: "The TRUE-e2e bar; foundation ALL e2e tests adopt"
status: completed
attempt_count: 1
domains: [rust, e2e, harness, docker, http-transport, observability]
plan_file: docs/plans/2026-06-04-test-brutal-real-infra-e2e-suite-plan.md
contract: docs/reference/e2e-harness-contract.md
session_id: work-2026-06-04-113652
---

## What Was Implemented
`tests/e2e/harness/` (the canonical real-app harness, per docs/reference/e2e-harness-contract.md):
- `stack.rs` — full-stack up (handles #157 cold-start restart), kill/stop/start/pause/unpause/restart; URL/DSN/volume constants.
- `app.rs` — `McpClient` over reqwest → real `:3001`: `compile_context`/`extract_session`/`health`/`ingest_transcript`, JSON-RPC tools/call.
- `seed.rs` — sidecar volume writer/approver (`docker run -v <vol>:/skills alpine`): `write_pending`/`approve`(rename)/`remove`/`seed_and_approve`; `SkillScope`.
- `observe.rs` — read-only `PgObserver`/`QdrantObserver`/`RedisObserver` + `InfraSnapshot` (concurrent).
- `poll.rs` — `poll_until`, `wait_for_rebuild(prev, 90s)` (PG graph_version + served HTTP graph_version), `wait_for_health`.
- `stagelog.rs` — per-run/per-stage `NN-<name>.json` (input+output+infra_snapshot) + `<scenario>.md` + `report.rs`-compatible E2EReport.
- `test_golden_path_real_app.rs` — golden-path tracer bullet; include via `#[path = "harness/mod.rs"] mod harness;`.

## LIVE validation (full real stack up)
- ✅ HARNESS GREEN live: `/health` 200; real `compile_context` HTTP response captured; sidecar seed+approve mutated the
  real `test-global-skills` volume; PG/Qdrant/Redis observers read real values; 9 per-stage JSON + `golden-path.md` + E2EReport written.
- 🔴 GOLDEN-PATH RED (correct/honest): fails at `wait_for_rebuild` — "snapshot did not advance from v7 within 90s — see #156;
  PG graph_version=8, served graph_version=2". Deterministically reproduces #156. This RED is the regression guard for the #156 fix.

## Files Changed
- `tests/e2e/harness/{mod,stack,app,seed,observe,poll,stagelog}.rs` — created
- `tests/e2e/test_golden_path_real_app.rs` — created
- `crates/mcp-server/Cargo.toml` — redis dev-dep + `[[test]]` registration

## Patterns Discovered
- Volume name = `dynamic-agent-skill-layer_test-global-skills` / `_test-project-skills`. Sidecar pass SKILL content via env var (no heredoc quoting).
- `compile_context` returns `DuplicateSuppressed` for repeated `(session_id, repo_path, graph_version)` → poller uses timestamp-suffixed session_id.
- Real-app symptom of #156: served graph_version frozen at 2 while PG climbs (5→6→7→8); Redis stream len=1; qdrant points=0; outbox_events pile up.

## TDD Evidence
- **Red:** golden-path fails LIVE at wait_for_rebuild (#156), bounded 90s, clear message + stage logs written. NOT skipped/faked.
- **Green:** harness capabilities proven live (health, HTTP compile_context, seed/approve, observers, stage logs); 6 report unit tests pass.
- **Post-Refactor Green:** fmt clean; recompiles; same stable RED.

## Test Results
- `cargo test --test test_golden_path_real_app ... --nocapture` → harness green; golden-path RED (#156), as designed. fmt clean; targets compile.
