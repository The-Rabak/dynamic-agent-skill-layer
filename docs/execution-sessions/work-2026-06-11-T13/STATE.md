---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/13-convert-integration-fakes-to-live.md
started: 2026-06-11
status: in_progress
execution_shape: vertical-slices
current_unit: 1
total_units: 1
session_id: work-2026-06-11-T13
---

## WHY Linkage
- This execution serves: a no-fakes test substrate so efficacy measurement (T14/T15) cannot be faked. Drain the frozen `tests/integration` fake allowlist to empty.
- Success-criteria focus: allowlist EMPTY; guard hard-fails on any banned fake in tests/integration; CapturingEventPublisher/bench-mock decision recorded; full live suite green.

### RECORDED DECISION (T13 AC — policy fork, owner decision 2026-06-11)
**Policy = "relocate-or-live"** (owner-chosen over strict-true-e2e):
- A test that verifies pure LOGIC with controlled inputs — ranking/scoring math, embedder FAULT-INJECTION (`fail_first`), verifier-verdict-driven promotion/merge logic — is a **unit test misfiled in tests/integration**. RELOCATE it into the owning crate under `#[cfg(test)]`, where deterministic doubles are legitimately allowed (machine rule: fakes OK in unit tests behind cfg(test)). The shared `graph_builder::graph::embeddings::DeterministicEmbeddingService` is already `feature="test-utils"`-gated and allowed there.
- A test that genuinely exercises the INTEGRATED stack (real PG / Ollama / Redis / LLM) → CONVERT to a live `#[ignore]`-gated test driving real services, with assertions robust to real embeddings (structural/relative, not fragile exact rankings — cf. the T06 live test).
- **CapturingEventPublisher / bench-mocks: ACCEPTABLE, not banned.** An event-observer SPY that records the REAL emitted `EventEnvelope`s (and a bench harness double) does not fake/replace production *behavior* — it observes it. It is a legitimate test observer, stays, and is NOT added to the guard banlist. (A fault-injection provider that returns a simulated error to exercise the failure path is likewise a legitimate unit-test double → lives behind cfg(test) after relocation.)
- The guard's banlist stays the 7 behavior-faking embedder/verifier symbols. After the allowlist is empty, flip the guard so tests/integration is treated like tests/e2e for those symbols (no allowlist branch).

### Constitution Context
- Machine-wide + constitution no-fakes mandate: zero stubs/fakes outside unit tests; fakes allowed ONLY behind `#[cfg(test)]`/test-utils. tests/integration is NOT unit tests → must not carry banned fakes. Relocation moves a genuinely-unit test to where the double is allowed; conversion makes a genuinely-integration test live.

### Architecture Handoff
- Feature home: tests/integration + scripts/check-no-fakes.sh + scripts/no-fakes-integration-allowlist.txt. Relocations land in the owning crate's src under `#[cfg(test)]` (mcp-server, retrieval, maintenance, graph-builder, session-extractor).
- Parallel-safe (non-retrieval home). Allowlist policy: SHRINK-ONLY — partial drains are valid, guard stays green throughout.

## Triage (orchestrator pre-analysis; agent confirms per file)
| File | Banned symbols | Local-defined | Likely disposition |
|---|---|---|---|
| test_extract_session.rs | none (spy + fault provider only) | 0 | CLEAN under policy → remove from allowlist |
| test_admin_tools.rs | DeterministicEmbeddingService | 0 (shared) | triage: relocate or live |
| test_merge_workflow.rs | DeterministicEmbeddingService | 0 | triage |
| test_outbox_consistency.rs | DeterministicEmbeddingService | 0 | triage |
| test_retire_workflow.rs | DeterministicEmbeddingService | 0 | triage |
| test_watcher_rebuild.rs | DeterministicEmbeddingService | 0 | triage |
| test_compile_context.rs | DeterministicEmbeddingService (+fault) | 1 | likely RELOCATE (compile_context logic + fault-injection) |
| test_dual_scope.rs | DeterministicEmbeddingService (+fault) | 1 | likely RELOCATE (ranking/scope math + fault-injection) |
| test_session_persistence.rs | DeterministicEmbeddingService | 1 | triage |
| test_promotion_recurrence.rs | UnitVector/AlwaysEquivalent/NeverGeneral | 3 | likely RELOCATE (promotion/recurrence logic w/ controlled verifiers) |

## Work Status
| # | Unit | Kind | Serves | Status | Session File |
|---|------|------|--------|--------|--------------|
| 1 | T13 drain integration fake allowlist (relocate-or-live) | hygiene | no-fakes substrate for efficacy | in_progress | unit-01-convert-integration-fakes-to-live.md |

## Learnings Brief
- [policy] relocate-or-live decided; spies/fault-injection providers are acceptable unit-test doubles; guard banlist stays the 7 embedder/verifier symbols.
- [safety] WSL2: single serial agent; no parallel/background cargo; orchestrator runs the heavy live `--ignored` suite. Live PG = 127.0.0.1:15432; mcp-server = 127.0.0.1:3001; stack is up.
- [guard] allowlist is SHRINK-ONLY; keep `scripts/check-no-fakes.sh` GREEN after every file. Do NOT weaken the guard to pass — relocate/convert the fakes.
