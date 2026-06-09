---
ticket_id: T08
title: Retrieval contract docs and efficacy handoff
kind: hardening
status: ready
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
source_packet_ref: "## Execution Slices > Slice 8"
feature_home: "docs/reference and docs/assessments"
depends_on:
  - T01
  - T02
  - T03
  - T04
  - T05
  - T06
dependency_type: hard
serves:
  - Honest contract docs and #205/#218 efficacy handoff
files:
  - docs/reference/retrieval-contract.md
  - docs/reference/online-retrieval-cqrs.md
  - docs/assessments/
  - docs/architecture/
test_command: "python3 scripts/retrieval_quality_live.py --split held_out --config-label v1.7-final --limit 5 --out tests/e2e/reports/v17-final__held_out.json --gate --regression-floor 0.60 && test -f docs/reference/retrieval-contract.md && test -f docs/reference/online-retrieval-cqrs.md"
tdd_mode: ralph
---

# Retrieval contract docs and efficacy handoff

## Serves

Record the measured V1.7 retrieval contract so the efficacy proof can attribute outcomes to retrieval behavior honestly.

## Scope

- Update retrieval contract docs to match shipped behavior.
- Update CQRS/read-path docs and ADR if Qdrant becomes a request-time dependency.
- Write a V1.7 measured assessment with quality, latency, default/experimental flags, and remaining gaps.
- Hand #205/#218 the fields needed to log retrieval hits, misses, graph evidence, and latency.

## Scope Fence

- Do not claim graph improves retrieval unless the measurements prove it.
- Do not leave stale docs saying Qdrant is write-side only if hybrid Qdrant becomes default.
- Do not mark the frozen 0.80 target met unless the live report supports it.

## Acceptance Criteria

- Docs match code and runtime flags.
- Default backend, embedder, reranker status, and graph-search behavior are clear.
- Final assessment states whether V1.7 hit 0.80 MRR/nDCG or remains short.
- #205/#218 can consume a clear retrieval instrumentation contract.
- If T07 was skipped, docs explicitly record reranker/decomposition as skipped or experimental rather than treating it as a blocker.

## Shared / Global Notes

Documentation is the contract for future execution. If code and docs diverge, later sessions will measure or build the wrong thing.

## Local Context

- WHY source: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`.
- This ticket serves: finish V1.7 with honest docs and a usable handoff into efficacy work.
- Current docs explicitly say Qdrant is not read at query time; only change that after the architecture really changes.
- Important unknown: T07 may remain experimental or skipped; document reality, not plan intent.

## Inherited Changes — V1.7 batch 1-2 triage (todos 228-244)

These landed on `feat/v-1-7` during the 228-243 triage swarm (2026-06-09) and are part of the contract this ticket must document honestly:

- **Model/arm attribution surfaces now exist and must be documented:** `embedding_model_metadata` table (`key='active'`, written per rebuild, #228); `/health` `embedding_arm` component = `model=…/dim=…/collection=…` (#239); the `dimension` field + per-arm latency in T01 report arms (#229/#230). The #205/#218 instrumentation handoff can attribute runs to the persisted active-model row.
- **Document the harness honesty contract** (#229): `--require-dimension` / `--gate` fail loud on a null dimension for a live arm; `OLLAMA_HOST` is SSRF-validated; `QDRANT_COLLECTION` is charset-guarded (#241). Measurement never silently ships an unattributable arm.
- **Correct the "always fatal" DimensionMismatch claim** in any doc: the guard is **fatal only when Qdrant is reachable at boot** (#235); the offline window is observable but not fully closed (relay re-`ensure_collection` on reconnect is documented remaining work) — record this caveat honestly rather than overstating.
- **Collections are model-keyed (`skills__<slug>`), not `"skills"`** (#234/#236); if hybrid Qdrant becomes the default read path, the CQRS/read-path docs must reflect the model-keyed, charset-guarded naming and the `Result`-returning derivation.
- **`find_skill`/`search_skill_graph` will carry `retrieval_context` provenance** (T06, routed via #243) — reconcile the retrieval-contract doc with that field once T06 lands.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`

## Deeper-Dive Refs

- `docs/reference/retrieval-contract.md`
- `docs/reference/online-retrieval-cqrs.md`
- `docs/assessments/2026-06-07-retrieval-quality-234-corpus-measured.md`
- `docs/assessments/2026-06-08-community-graph-why-harmful-and-grounded-path-208.md`

## Coupling Notes

This must run last. Its job is to reconcile documentation with measured implementation, not to backfill unbuilt claims.
