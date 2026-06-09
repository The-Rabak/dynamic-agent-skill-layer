# No-Fakes Audit — 2026-06-07

Exhaustive enumeration of every fake/stub/mock usage found across the repository as of
commit scope for ticket #206. Classification and disposition per the phased enforcement
policy (STRICT mode): e2e and production must be clean now; integration debt is frozen via
the allowlist manifest at `scripts/no-fakes-integration-allowlist.txt`.

---

## Summary Counts

| Zone | Total fake usages | Violations resolved now | Frozen debt (follow-up epic) | Status |
|---|---|---|---|---|
| tests/e2e/ | 1 file / 1 usage | 1 (FIXED) | 0 | CLEAN |
| crates/*/src (production) | 0 outside #[cfg(test)] | 0 | 0 | CLEAN |
| tests/integration/ | 10 files / ~41 usages | 0 (frozen) | 10 files | FROZEN ALLOWLIST |
| tests/bench/ | 1 file / 1 usage | 0 (bench-only) | 0 | ALLOWED (bench) |
| crates (unit tests #[cfg(test)]) | many | 0 | 0 | OK (unit-test-gated) |

---

## Banned Symbol Seed List

The guard at `scripts/check-no-fakes.sh` enforces these symbols:

- `DeterministicEmbeddingService`
- `AlwaysEquivalentVerifier`
- `TextOverlapMergeSemanticVerifier`
- `NoOpSynthesisPass`
- `NoOpMergeSemanticVerifier`
- `UnitVectorEmbeddingService`
- `NeverGeneralVerifier`

---

## Zone 1: tests/e2e/ — All findings

| File | Line | Symbol | Classification | Disposition |
|---|---|---|---|---|
| tests/e2e/test_watcher_churn_reconciliation.rs | 161 (former) | `DeterministicEmbeddingService` | VIOLATION | **FIXED** — replaced with real `OllamaEmbeddingService`, test split into ungated watcher/reconciliation assertions + `#[ignore]` live rebuild test |

**Post-fix status: ZERO fakes in tests/e2e/. Guard enforces this permanently.**

---

## Zone 2: crates/*/src Production Paths — All findings

### DeterministicEmbeddingService

| File | Line | Context | Classification | Disposition |
|---|---|---|---|---|
| crates/graph-builder/src/graph/embeddings.rs | 5, 8, 43 | `#[cfg(any(test, feature = "test-utils"))]` | OK — properly gated | No action needed |
| crates/graph-builder/src/graph/rebuild.rs | 336, 374 | Inside `#[cfg(test)] mod tests` | OK — unit test | No action needed |
| crates/maintenance/src/runtime.rs | 1132 | Inside `#[cfg(test)] mod tests` | OK — unit test | No action needed |
| crates/maintenance/src/merge.rs | 836, 856 | Inside `#[cfg(test)] mod tests` | OK — unit test | No action needed |

### AlwaysEquivalentVerifier

| File | Line | Context | Classification | Disposition |
|---|---|---|---|---|
| crates/maintenance/src/merge.rs | 841, 1171 | Inside `#[cfg(test)] mod tests` | OK — unit test | No action needed |
| crates/maintenance/src/promote.rs | 1394 | Inside `#[cfg(test)] mod tests` | OK — unit test | No action needed |

### TextOverlapMergeSemanticVerifier

| File | Line | Context | Classification | Disposition |
|---|---|---|---|---|
| crates/maintenance/src/merge_verifier.rs | 143, 148, 156 | Production struct (real text-overlap impl) | OK — this is a REAL verifier implementation, not a fake. Its unit tests at lines 198–221 are inside `#[cfg(test)]` | No action needed |

### NoOpEmbeddingService (variant found; not in original banned list)

| File | Line | Context | Classification | Disposition |
|---|---|---|---|---|
| crates/mcp-server/src/protocol.rs | 678 | Inside `#[cfg(test)] mod tests` | OK — unit test | No action needed |
| crates/retrieval/src/orchestrator.rs | 739 | Inside `#[cfg(test)] mod tests` | OK — unit test | No action needed |

**Post-fix status: ZERO production-path fakes. All crate-internal fakes are properly `#[cfg(test)]`-gated.**

---

## Zone 3: tests/integration/ — All findings (Frozen Debt)

These files contain fakes that predate constitution enforcement. They are frozen in the
allowlist manifest. The guard hard-fails if any NEW file outside this list introduces a
banned symbol. Conversion to real Ollama/LLM is tracked as a follow-up epic.

| File | Symbols Used | Status |
|---|---|---|
| tests/integration/test_admin_tools.rs | `DeterministicEmbeddingService` | FROZEN — allowlisted |
| tests/integration/test_compile_context.rs | `DeterministicEmbeddingService` (local def + usage) | FROZEN — allowlisted |
| tests/integration/test_dual_scope.rs | `DeterministicEmbeddingService` (local def + usage) | FROZEN — allowlisted |
| tests/integration/test_extract_session.rs | `CapturingEventPublisher` | FROZEN — allowlisted |
| tests/integration/test_merge_workflow.rs | `DeterministicEmbeddingService` | FROZEN — allowlisted |
| tests/integration/test_outbox_consistency.rs | `DeterministicEmbeddingService` | FROZEN — allowlisted |
| tests/integration/test_promotion_recurrence.rs | `UnitVectorEmbeddingService`, `AlwaysEquivalentVerifier`, `NeverGeneralVerifier` | FROZEN — allowlisted |
| tests/integration/test_retire_workflow.rs | `DeterministicEmbeddingService` | FROZEN — allowlisted |
| tests/integration/test_session_persistence.rs | `DeterministicEmbeddingService` (local def + usage) | FROZEN — allowlisted |
| tests/integration/test_watcher_rebuild.rs | `DeterministicEmbeddingService` | FROZEN — allowlisted |

---

## Zone 4: tests/bench/ — All findings

| File | Line | Symbol | Classification | Disposition |
|---|---|---|---|---|
| tests/bench/compile_context_bench.rs | 14 | `MockEmbeddingService` | ALLOWED — benchmark harness, not in e2e/integration scope | No action needed; benches are excluded from the guard |

---

## Zone 5: test_dream_state_contract.rs — "Deterministic" occurrences

`tests/e2e/test_dream_state_contract.rs` contains the word "Deterministic" in string
literals (documentation/description fields, not as a Rust type name). These are NOT
banned-symbol violations — they are plain string values describing test strategy, not
fake service structs. Confirmed clean.

---

## Guard and CI

- Script: `scripts/check-no-fakes.sh` — exits 0 on clean tree, exits 1 on any violation.
- Manifest: `scripts/no-fakes-integration-allowlist.txt` — frozen debt list, may only shrink.
- CI job: `no-fakes-guard` in `.github/workflows/live-e2e.yml`, runs before `live-e2e`.

---

## Hard Invariants (post-ticket state)

1. **tests/e2e/ is fake-free.** Guard enforces this permanently (exit 1 on any banned symbol).
2. **Production paths (crates/*/src outside #[cfg(test)]) are fake-free.** Guard enforces.
3. **tests/integration/ manifest can only shrink.** Any new integration fake not in the manifest fails the guard.
4. **Guard exits 0 on current tree.** Verified locally.
5. **No fake was introduced to make anything pass.** Confirmed by audit.
