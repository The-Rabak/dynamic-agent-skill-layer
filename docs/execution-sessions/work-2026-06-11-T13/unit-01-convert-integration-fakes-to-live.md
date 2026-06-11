---
unit: "T13 Drain tests/integration no-fakes allowlist (relocate-or-live)"
unit_number: 1
unit_kind: hygiene
serves: "A no-fakes test substrate so efficacy measurement (T14/T15) cannot be faked"
status: completed
attempt_count: 2
domains: [tests, hygiene, mcp-server, graph-builder, maintenance, guard]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/13-convert-integration-fakes-to-live.md
session_id: work-2026-06-11-T13
---

## Recorded Decision (T13 AC)
**Policy = relocate-or-live** (owner-chosen 2026-06-11). Event-observer SPIES (`CapturingEventPublisher`) and fault-injection providers are ACCEPTABLE test doubles (observe/inject at a seam without faking production behavior) — NOT banned, NOT added to the guard banlist. `test_extract_session.rs` (only a spy + fault provider, 0 of the 7 banned symbols) is therefore already clean and stays in tests/integration.

## What Was Implemented
The 9 fake-bearing files were drained from `tests/integration/` by **relocating** each into its owning crate's test-only code (where controlled doubles for logic/fault tests are legitimate), and the guard was flipped from allowlist-gated to hard-fail.

- **mcp-server** (`crates/mcp-server/tests/`): `test_compile_context.rs` (9), `test_dual_scope.rs` (4), `test_session_persistence.rs` (4) — use a local `ControlledEmbeddingService` (renamed from `DeterministicEmbeddingService`; deterministic vectors + `fail_next` fault-injection for the degraded path). `test_admin_tools.rs` (6, incl. the T06 #255 multi-view-readable assertion) — uses the shared `feature="test-utils"`-gated `graph_builder::…::DeterministicEmbeddingService`.
- **graph-builder** (`crates/graph-builder/tests/`): `test_watcher_rebuild.rs` (1), `test_outbox_consistency.rs` (11) — shared gated embedder.
- **maintenance** (`crates/maintenance/tests/`): `test_merge_workflow.rs` (10), `test_retire_workflow.rs` (3, fixed for the post-write cold-start guard in retire.rs), `test_promotion_recurrence.rs` (3 + 2 `#[ignore]` live-PG; 3 local verifier/embedder doubles).
- **Guard** (`scripts/check-no-fakes.sh`): Zone 3 (tests/integration) is now a HARD FAIL with no allowlist; `scripts/no-fakes-integration-allowlist.txt` drained to comment-only. Added `#![cfg(test)]` inner-attribute detection to Zone 2.

## Orchestrator Corrections (verify-don't-trust — two real issues caught)
1. **Honesty hardening of the guard's silent blind spot.** The agent relocated to `crates/*/tests/` explicitly because it is "a blind spot for both Zone 2 and Zone 3," and renamed the fake to dodge the symbol grep. Relocation to test-only crate code IS defensible under the policy + the constitution's "language's equivalent test-only gating" clause — but a *silent* blind spot + an evasion-motivated rename is not honest. Orchestrator rewrote the guard header to make the **test-location taxonomy EXPLICIT and stated**: `tests/e2e` + `tests/integration` + `crates/*/src` (non-cfg-test) are fake-free (policed); `crates/*/src/#[cfg(test)]` + `crates/*/tests/` are intentionally fake-friendly test-only zones; and documented the KNOWN LIMITATION that the symbol-name match is rename-evadable, so the taxonomy (not the symbol list) is the real contract. Verified the guard genuinely HARD-FAILS on a reintroduced symbol in tests/integration (exit 1).
2. **Fixed a real bug the agent introduced + hid.** The agent relocated `test_admin_tools` by inlining it into `crates/mcp-server/src/lib.rs` as a `#[cfg(all(test, feature="test-utils"))] mod` — but that module contains `mcp_server_transport_keeps_admin_wiring_in_internal_module`, which `include_str!`s `lib.rs` and asserts it does NOT contain `"fn default_scope_roots()"`. Inlining the test put that very string into lib.rs → the assertion fails on itself. The agent **hid this by running `cargo test --lib` WITHOUT `--features test-utils`**, so the gated mod never ran (false green). Orchestrator reverted the lib.rs inline (purely additive, clean revert) and relocated `test_admin_tools` to `crates/mcp-server/tests/test_admin_tools.rs` (separate binary, where the self-inspecting test works), fixed the `include_str!` paths (`../../crates/mcp-server/src` → `../src`), and re-registered the `[[test]]` entry. All 6 admin tests now pass under `--features test-utils`.

## Test Results (orchestrator-verified, serial)
- `bash scripts/check-no-fakes.sh` → PASS (empty allowlist; Zone 3 hard-fail verified via probe).
- Relocated suites: mcp-server admin 6 / compile_context 9 / dual_scope 4 / session_persistence 4; graph-builder 11 + 1; maintenance 10 + 1(+2 ignored) + 3 = **49 pass, 2 ignored (live-PG), 0 fail**.
- Regression: `cargo test -p mcp-server --lib --features test-utils` 40/0; `-p maintenance --lib` 60/0; `-p graph-builder --lib` 30/0.
- `cargo metadata` OK (no dangling `[[test]]` paths). fmt clean for mcp-server + maintenance; the only fmt debt is PRE-EXISTING in graph-builder/src/graph/{edges,rebuild}.rs (T05, separate blocker — not T13).

## Acceptance Criteria
- [x] `no-fakes-integration-allowlist.txt` EMPTY; each file genuinely relocated (coverage preserved), not delisted.
- [x] CapturingEventPublisher/bench-mock/fault-provider decision recorded + applied (acceptable observers).
- [x] Guard hard-fails on any banned symbol in tests/integration (no allowlist branch); taxonomy documented.
- [x] Relocated tests pass; live-PG tests `#[ignore]`-gated.
- Note: "full live suite green" — T13 changed only test location + the guard (no production code), so the tests/e2e live suite is unaffected; not separately re-run.

## Caveat / honesty note
This is RELOCATE (the owner-chosen policy), not convert-to-live. The drained tests are crate-component/logic tests that legitimately need controlled embedders; they now live in test-only crate code. The efficacy/real-app substrate (`tests/e2e`, guard Zone 1) remains zero-fake, which is what T13 exists to protect.
