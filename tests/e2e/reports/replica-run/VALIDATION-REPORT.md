# T10 corpus rebuild — validation report (2026-06-10)

Real data layer (`skill_layer_test` + `skills__qwen3-embedding-4b`), qwen3-embedding:4b default.
Source: 24 genuine Claude Code dev sessions on this project (1–3 MB each), NOT the old
circular `-tmp` extraction-scratch.

## Pipeline (real, no fakes)
24 session JSONLs → `/ingest/transcript` (real mcp-server) → PG queue → real
`maintenance-worker` (EXTRACT_SESSION_PROVIDER=claude-code, drain-until-empty) → 262
`.pending` drafts → approve (rename) → seed into `test-project-skills` volume → real
graph-builder reconcile → `skill_layer_test` (262) + `skills__qwen3-embedding-4b` (262) →
real mcp-server serves retrieval over HTTP.

## Field-population (T10 acceptance — old corpus = 0)
262 skills total:
- use_when 188 (72%), avoid_when 187 (71%), requires 171 (65%), invariants 188 (72%),
  tools 150 (57%), produces 188 (72%), evidence 188 (72%).
- **71% of drafts carry rich multi-view fields** vs the old corpus's 0%.

Type distribution (drafts): failure_fix 45, best_practice 33, diagnostic 33, anti_pattern 30,
rule 21, principle 11, procedure 11, prerequisite 2, **preference 2** (old corpus was
preference-dominated), 74 untyped.

## Live retrieval (real mcp-server, qwen3, HTTP)
High semantic precision — top match correct for every probe:
- "migration file exists but never applied" → `migration-file-unwired-from-registry`
- "test asserts a failure that no longer happens after the fix" → `stale-test-asserts-failure-after-fix`
- "formatter changed files outside the ticket scope" → `agent-workspace-fmt-scope-creep`
- "parallel tests interfere via shared env variables" → `parallel-test-shared-scope-contamination`
- "verify infrastructure healthy before e2e" → `verify-live-infra-before-e2e-agent-dispatch`

## Code changes shipped (committed)
- Grounding validator: verbatim-substring → distinctive-token-overlap rescue (was deleting
  the best skills; 3-of-4 → all rescued live). Commit 8b36148.
- qwen3-embedding:4b is the de-facto default; fixed two hardcoded-nomic embedder configs.
  Commit d911fdd.
- graph-builder honors QDRANT_COLLECTION (parity with mcp-server). Commit 8b36148.
- T09 blank-view boot-crash fix (skip blank dense views). Commit 87c0e11.

## Findings / follow-ups (file as todos)
1. **qwen3 mcp-server boot is very slow (~7 min for 262 skills):** mcp-server re-embeds the
   whole corpus's dense views + subunits at boot with qwen3 (2560-dim) instead of reading the
   precomputed Qdrant vectors. One-time per restart but a real operational regression vs nomic.
   Fix: load precomputed vectors at boot, or cache dense-view embeds.
2. **qwen3 scores compressed (~0.016 uniform):** ranking is correct but absolute cosine scores
   are low/compressed vs nomic. RETRIEVAL_RELEVANCE_THRESHOLD / score scaling likely needs
   qwen3 recalibration (a good query briefly returned no_match before warm).
3. **`graph_state` must NOT be truncated:** it holds a required singleton row; truncating it
   stalls graph-builder ("could not read PG graph_version … no rows"). Restore with
   `INSERT INTO graph_state (singleton, graph_version) VALUES (TRUE, 0)`.
4. **Synthesis candidates citing sibling skill-names as evidence still drop** under grounding
   (recall-first acceptable). Consider a synthesis-prompt nudge to cite transcript anchors.

## Not run (needs decision)
- **Dense-views ON/OFF sweep:** `scripts/t09_dense_views_sweep.py` drives the `held_out` split,
  whose labels map to OLD-corpus skill IDs that don't transfer to this corpus. A corpus-matched
  eval set must be generated first (e.g. derive held-out queries from each skill's `use_when`,
  measure source-skill rank ON vs OFF). Deferred pending owner decision.
