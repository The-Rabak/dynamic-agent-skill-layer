---
ticket_id: T13
title: Convert all tests/integration fakes to live/real (drain the allowlist to empty)
kind: hygiene
status: ready
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "constitution: no stubs/fakes in non-unit tests"
source_packet_ref: "promoted from todo #219 (P1)"
feature_home: "tests/integration and scripts/no-fakes guard"
depends_on: []
dependency_type: none
serves:
  - A no-fakes test substrate so efficacy measurement (T14/T15) cannot be faked
files:
  - tests/integration/
  - scripts/no-fakes-integration-allowlist.txt
  - scripts/check-no-fakes.sh
test_command: "scripts/check-no-fakes.sh (allowlist empty) && full live suite green"
tdd_mode: ralph
---

# Convert all tests/integration fakes to live/real (drain the frozen allowlist to empty)

## Serves

Honesty bar (machine-wide rule + constitution): no stubs/fakes/placeholders outside unit tests. `tests/integration` still carries a frozen allowlist of fake-bearing files. This must drain to empty before efficacy is measured — you cannot prove usefulness on a suite that fakes its infrastructure.

## Scope

- Convert/relocate all files in `scripts/no-fakes-integration-allowlist.txt` to live/real.
- Resolve the bench-mock and `CapturingEventPublisher` questions with a recorded decision.
- Flip the no-fakes guard to treat `tests/integration` identically to `tests/e2e` once the manifest is empty.

## Scope Fence

- Do not relax the guard to make it pass; convert the fakes.
- Mocks/stubs remain allowed ONLY behind `#[cfg(test)]` unit gating.

## Acceptance Criteria

- [ ] `scripts/no-fakes-integration-allowlist.txt` is EMPTY (all files converted/relocated).
- [ ] The bench-mock and `CapturingEventPublisher` questions resolved with a recorded decision.
- [ ] The guard hard-fails on ANY fake in `tests/integration` (manifest no longer needed).
- [ ] Full live suite green.

## Local Context

- WHY source: constitution no-fakes mandate; gates efficacy (T14/T15) credibility.
- Independent of the retrieval spine — parallel-safe, but should land before efficacy measurement.

## Source

Promoted 2026-06-09 from todo #219 (P1). Original analysis in git of `todos/219-*`.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`
