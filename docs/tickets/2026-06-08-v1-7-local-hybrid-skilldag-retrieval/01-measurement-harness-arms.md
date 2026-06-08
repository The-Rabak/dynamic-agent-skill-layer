---
ticket_id: T01
title: V1.7 measurement harness arms
kind: tracer-bullet
status: ready
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
source_packet_ref: "## Execution Slices > Slice 1"
feature_home: "tests/e2e quality harness and scripts/retrieval_quality_*"
depends_on: []
dependency_type: none
serves:
  - Measurement before ranking changes
files:
  - scripts/retrieval_quality_live.py
  - scripts/retrieval_quality_sweep.py
  - tests/e2e/reports/
test_command: "python3 scripts/retrieval_quality_sweep.py && python3 scripts/retrieval_quality_live.py --split held_out --config-label v1.7-baseline --limit 5 --out tests/e2e/reports/v17-baseline__held_out.json --gate --regression-floor 0.60"
tdd_mode: ralph
---

# V1.7 measurement harness arms

## Serves

Add the measurement tracer bullet that lets later tickets compare current default retrieval against V1.7 arms without changing production defaults.

## Scope

- Extend the real-server retrieval quality sweep/reporting path to record V1.7 arm metadata.
- Report backend, embedder model, dense/sparse/rerank flags, latency, MRR, nDCG, hit@3, recall@3, and no-match precision.
- Preserve the standing rule that retrieval quality measurement drives the real running `mcp-server` over HTTP.

## Scope Fence

- Do not alter retrieval ranking behavior except through explicit config arms used by the real server.
- Do not add in-process reconstruction of retrieval, scope fusion, or scoring.
- Do not lower current gates or mark the 0.80 target green unless measured.

## Acceptance Criteria

- Reports identify the current default arm and each V1.7 experimental arm.
- The harness can run a baseline without Qwen, Qdrant hybrid, or reranker enabled.
- Generated reports include enough metadata to attribute quality and latency deltas to a backend/model/config arm.
- No new fake retrieval path is introduced.
- Validation drives the real running `mcp-server`; `--help` output alone is not sufficient evidence.

## Shared / Global Notes

Retrieval measurement is a repo-wide quality gate. Keep helpers reusable, but do not move business ranking logic into the harness.

## Local Context

- WHY source: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`.
- This ticket serves: create the measurement surface that all later V1.7 ranking changes must pass through.
- Current truth: #210 measured the real server; an earlier in-process rig lied badly and must not be repeated.
- Important unknown: exact arm names/env vars should follow existing `RetrievalConfig::from_env` patterns.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`

## Deeper-Dive Refs

- `docs/assessments/2026-06-07-retrieval-quality-234-corpus-measured.md`
- `docs/reference/retrieval-contract.md`
- `~/.claude/projects/-home-rabak-projects-dynamic-agent-skill-layer/memory/measurement-drives-real-app-no-in-process-reconstruction.md`

## Coupling Notes

Later tickets depend on this for honest comparison. Keep it first and avoid changing product behavior here.
