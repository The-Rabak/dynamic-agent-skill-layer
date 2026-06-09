---
ticket_id: T02
title: Local Qwen3 embedder backend and rebuild safety
kind: expansion
status: completed
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context and ## Proposed V1.7 Architecture"
source_packet_ref: "## Execution Slices > Slice 2"
feature_home: "crates/infrastructure/src/embeddings, crates/graph-builder, crates/mcp-server"
depends_on:
  - T01
dependency_type: hard
serves:
  - Better dense retrieval without external APIs
files:
  - crates/infrastructure/src/embeddings/ollama.rs
  - crates/graph-builder/src/graph/build.rs
  - crates/mcp-server/src/lib.rs
  - crates/infrastructure/migrations/
test_command: "cargo test -p infrastructure embeddings && cargo test -p graph-builder && OLLAMA_EMBED_MODEL=qwen3-embedding:4b python3 scripts/retrieval_quality_live.py --split held_out --config-label qwen3-embedding-4b --limit 5 --out tests/e2e/reports/v17-qwen3__held_out.json --gate --regression-floor 0.60"
tdd_mode: ralph
---

# Local Qwen3 embedder backend and rebuild safety

## Serves

Make `qwen3-embedding:4b` a measurable local dense-retrieval arm without external API dependencies or mixed-vector corruption.

## Scope

- Configure the Ollama embedding adapter to support a local Qwen3 embedding model.
- Record model name, vector dimension, and model/digest metadata where the graph/reports can see it.
- Fail loudly on dimension mismatch or mixed-vector states.
- Run graph rebuild and `find_skill` measurement through the real server with Qwen metadata visible.

## Scope Fence

- Do not tune retrieval weights in this ticket.
- Do not mix `nomic` and Qwen vectors in one comparable dense index.
- Do not make Qwen the default unless the measured arm earns it and approval-sensitive model-change rules are satisfied.

## Acceptance Criteria

- Qwen arm returns correctly dimensioned embeddings.
- Graph rebuild fails loudly if expected and observed dimensions diverge.
- Measurement reports identify the embedder model and dimension.
- Existing `nomic-embed-text` path still works.
- Live held-out retrieval report proves the Qwen arm on the real server; unit tests alone are not sufficient.

## Shared / Global Notes

Embedding model metadata is a cross-feature contract between infrastructure, graph-builder, mcp-server boot, and retrieval quality reporting. Keep the adapter generic and model-specific assumptions observable.

## Local Context

- WHY source: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`.
- This ticket serves: test the highest-leverage local embedder bet before changing retrieval architecture.
- Current embedding summary path uses bounded text and caps inputs in `crates/infrastructure/src/embeddings/ollama.rs`.
- Important unknown: actual Ollama response dimension must be discovered from the live model, not assumed from docs alone.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`

## Deeper-Dive Refs

- `docs/reference/online-retrieval-cqrs.md`
- `docs/reference/retrieval-contract.md`
- https://qwenlm.github.io/blog/qwen3-embedding/
- https://ollama.com/library/qwen3-embedding

## Coupling Notes

T04 depends on this metadata so hybrid candidate generation can compare arms honestly. Schema or migration work here is approval-sensitive.
