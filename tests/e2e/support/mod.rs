// Shared across e2e test binaries; #[path]-included per-binary, so any helper a given binary
// doesn't exercise is dead_code only as a per-binary compilation artifact, not a real orphan.
#![allow(dead_code)]

// Reusable fault-injection harness for the brutal real-infra E2E suite.
//
// This module exposes four focused submodules:
//
//   - `infra`  — stop/start named compose services via `docker compose` commands.
//   - `drift`  — inject PG/Qdrant divergence (orphaned rows, orphaned vectors).
//   - `load`   — watcher churn driver and concurrent compile_context load generator.
//   - `poll`   — bounded readiness/convergence polling (replaces fixed sleeps).
//
// # Including this module from a sibling test file
//
// Add the following at the top of any test file that needs the harness:
//
// ```rust
// #[path = "support/mod.rs"]
// mod support;
// ```
//
// The path must be relative to the test file; all sibling files in
// `tests/e2e/` resolve to `support/mod.rs` with the line above.
//
// # Cargo registration
//
// Smoke tests that use this module must be registered in the crate's
// `[[test]]` table with `required-features = ["test-utils"]`, matching the
// pattern of all other live E2E tests.

pub mod drift;
pub mod infra;
pub mod load;
pub mod poll;
