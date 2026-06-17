# Work prompt — Multi-view field backfill → recall re-measurement (next session)

Paste the block below as the opening prompt of a fresh session on branch `feat/v-1-7`.

---

## Task

We proved (2026-06-17/18, committed `232a0e5` + `0d9bff7`, see
`docs/assessments/2026-06-18-multiview-recall-vs-rerank-decomposition.md`) that skill-side dense
views (`RETRIEVAL_DENSE_VIEWS`) are a **recall** lever on the 4b production arm: dense OFF→ON raises
`candidate_recall@50` 0.810→0.883 (+0.073) and MRR@3 0.684→0.741 (+0.057), concentrated on the
`transcript` stratum (recall 0.583→0.778). But **31% of the corpus lacks `use_when`/`e_needs`** (86
of 277 skills) — so the `e_task`/`e_needs` views are empty for those skills and contribute nothing.

**Hypothesis to test:** backfilling the missing multi-view fields raises recall further, especially
on the transcript/use_when strata. **Goal: prove or disprove it with a real-server measurement.**

### Step 1 — Diagnose WHY the 86 are empty (do not assume)

Identify them and split into two causes — this decides whether backfill is honest:
```sql
-- db skill_layer_test, user/pass skill_layer/skill_layer, container psql on :15432
SELECT id, name, array_length(use_when,1) uw, array_length(requires,1) rq,
       array_length(invariants,1) iv
FROM skills
WHERE COALESCE(array_length(use_when,1),0)=0 OR COALESCE(array_length(requires,1),0)=0;
```
For a sample, inspect the source session/SKILL.md body: did extraction MISS a trigger that was
present (recoverable), or did the session genuinely carry no use_when/requires signal (leave empty —
forcing a field here would be fabrication, which violates the no-fakes rule)?

### Step 2 — Backfill the recoverable ones HONESTLY

Re-extract the recoverable skills' source sessions with the current frontier prompt
(`crates/infrastructure/src/extraction/prompt_contract.rs`, which elicits all 7 views) and merge the
new `use_when`/`avoid_when`/`requires`/`invariants` into the skills table. Fields must be **grounded
in real session evidence** — no body-only synthesis that invents triggers. After updating the
columns, rebuild so the new `e_task`/`e_needs` view embeddings are produced (graph-builder rebuild;
the embedding cache is keyed by `(skill_id, view_kind, model_name)` + content_hash, so changed
field text re-embeds automatically). Log how many were backfilled vs left genuinely-empty.

### Step 3 — Re-measure (real server, exactly as the decomposition)

```bash
# CRITICAL: find_skill truncates to config.max_results (default 3) BEFORE .take(limit),
# so candidate_recall is only truthful with MAX_RESULTS=50.
restart() { env "$@" POSTGRES_DB=skill_layer_test docker compose up -d --no-deps --force-recreate mcp-server; }
# dense ON, deep pool, post-backfill:
restart RETRIEVAL_MAX_RESULTS=50
python3 scripts/t12_task_quality_probe.py --label recall_postbackfill_dvON_mr50 \
  --out tests/e2e/reports/retrieval/recall_postbackfill_dvON_mr50.json --limit 50
# compare against the pre-backfill baselines already on disk:
python3 scripts/retrieval_stratum_diff.py \
  tests/e2e/reports/retrieval/recall_dvON_mr50.json \
  tests/e2e/reports/retrieval/recall_postbackfill_dvON_mr50.json
# restore production defaults when done:
restart   # all RETRIEVAL_* unset
```
Read **per-stratum** `candidate_recall_at_limit` and `mrr_at3`. Success = transcript/use_when recall
rises vs the pre-backfill `recall_dvON_mr50.json` baseline. Report the delta honestly even if flat
(a flat result means the empty fields were genuinely signal-free, not a backfill failure).

### Reference facts
- Corpus: db `skill_layer_test`, model `qwen3-embedding:4b` (Ollama, `EMBEDDING_PROVIDER=ollama`,
  raw text, no instruction prefix), 277 live skills, snapshot_dense backend. 263 orphan 4b
  embeddings exist in `skill_embeddings` (harmless; optionally purge).
- Fixture: `tests/fixtures/retrieval_quality_262_corpus_labeled.json` (184 queries; strata
  transcript/disjoint/lexical/multiview/use_when/session_start/negative). Probe excludes
  session_start; gold matched by skill name.
- Scripts: `scripts/t12_task_quality_probe.py` (now emits `per_stratum`),
  `scripts/retrieval_stratum_diff.py` (diff runs). Metrics via `scripts/retrieval_metrics.py`.
- Env knobs (compose passthrough, all default-empty → compiled defaults): `RETRIEVAL_DENSE_VIEWS`,
  `RETRIEVAL_MAX_RESULTS`, `RETRIEVAL_NEGATIVE_VIEW_WEIGHT` (kept 0.0 — measured net-harmful),
  `RETRIEVAL_RELEVANCE_THRESHOLD` (floor 0.48 — note it gates recall; a gold below floor won't
  enter the pool regardless of views).
- mcp-server image rebuilds in ~1min via `docker compose build mcp-server` (cargo cache warm);
  warm boot ~20s (embedding cache, migration 011). Restart applies env without a rebuild.

### Standing rules (machine + project)
- Measurement drives the REAL running mcp-server over HTTP end-to-end — no in-process reconstruction.
- No stubs/fakes/placeholders in production paths or non-unit tests; fail loud. No fabricated fields.
- Serial builds only; never run concurrent heavy actions (cargo build/test/clippy) in subagents.
- Never delete freshly-generated run outputs (corpus/drafts/logs/reports). Clean only build
  artifacts + stale scratch.
- Restore production defaults (all `RETRIEVAL_*` unset, dense-on) when the run finishes.

### Follow-on (if time)
After find_skill is settled, measure dense ON/OFF on the **production priming path**
(`compile_context`, session_start) — transcript is the proxy; confirm the recall lift carries into
session-start. (`scripts/t12_priming_sweep.py` drives `compile_context`.)
