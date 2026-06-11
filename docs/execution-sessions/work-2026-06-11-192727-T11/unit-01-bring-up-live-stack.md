---
unit: "Bring up live qwen3 stack on 262 corpus"
unit_number: 1
unit_kind: infra-packet
serves: "real mcp-server is the measurement substrate for every T11 sweep"
status: completed
attempt_count: 1
domains: [infra, docker, retrieval]
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/11-corpus-multiview-resweep-hybrid-validation.md
session_id: work-2026-06-11-192727-T11
---

## What Was Implemented
Brought the V1.7 live test stack from fully-down to a warm, honest-ready qwen3 mcp-server serving the 262-skill T10 corpus.

## Steps
1. Discovered the live corpus was gone (PG ephemeral — no volume in docker-compose.test.yml; test-project-skills volume empty). Corpus survives only in `tests/e2e/reports/replica-run/skills/` (262 SKILL.md, rich multi-view).
2. Re-seeded the volumes: `test-project-skills` ← replica-run/skills/project (262 SKILL.md), `test-global-skills` ← skills/global (0 — corpus is all project-scoped).
3. Rebuilt mcp-server + graph-builder images at HEAD (9b0ab57) — the 7h-old images predated T17's final commits; rebuild gets the T17 embedding cache + honest /health.
4. `docker compose -f docker-compose.test.yml up -d` (postgres, redis, qdrant, ollama, graph-builder, mcp-server). qwen3-embedding:4b already in ollama_data volume.
5. graph-builder cold-reconciled 262 files → PG + Qdrant + T17 embedding cache; mcp-server built the in-memory snapshot.

## Evidence (live, real server)
- PG: `select count(*) from skills` → **262**.
- T17 embedding cache `skill_embeddings`: **2979 rows** — e_summary 264, e_task 262, e_needs 188, e_negative 187 (all T09 dense views), + subunits. Cache populated → subsequent arm reboots hit the cache (T17's 32× win).
- Multi-view in PG: tools 150, invariants 188, use_when 188 (matches T10 VALIDATION-REPORT — rich corpus, ≠ old 0%).
- `/health` → **HTTP 200** (T17 honesty: 503 while warming during the cold embed, flipped to 200 only when snapshot ready; `graph refresh applied after graph.rebuilt applied_version=2`).
- Smoke find_skill "migration file exists on disk but never added to the migrations array" → migration-file-unwired-from-registry (score=0.755), orphaned-migration-file-detection (0.781), migration-test-triple-update (0.608). #260 real eq.3 scores (≠ RRF 0.0164). Retrieval is healthy.

## Patterns Discovered
- T17 /health-200 gate works exactly as designed and replaces the old warmup-query readiness hack — confirmed the 503→200 transition tracks the real embed window.
- #260 scores on qwen3 sit ~0.6–0.78 (real semantic relevance), NOT the "compressed 0.016" RRF artifact — so the 0.48 floor recalibration (scope item 8) is measured against ~0.6–0.78, not 0.016.
- Container clock is UTC (host UTC+3); logs at 16:4x == local 19:4x.

## Test Results
- Command: `curl /health` + find_skill smoke + PG counts
- Result: PASS (262 skills, /health 200, retrieval correct)
- Attempts: 1
