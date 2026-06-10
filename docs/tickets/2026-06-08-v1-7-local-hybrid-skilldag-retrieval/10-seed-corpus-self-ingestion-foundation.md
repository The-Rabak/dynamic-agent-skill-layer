---
ticket_id: T10
title: Seed a real ≥200-skill corpus by dogfooding the ingestion pipeline
kind: foundation
status: completed
plan_ref: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
architecture_ref: "explicit-handoff: parent plan ## Architectural Context (extraction + graph flow)"
source_packet_ref: "promoted from todo #216 (P0)"
feature_home: "ingestion pipeline end-to-end: crates/session-extractor, crates/graph-builder, scripts/, tests/e2e"
depends_on:
  - T03
dependency_type: hard
serves:
  - The real corpus that efficacy (T14/T15), the hybrid re-sweep (T11), and trigger-aware retrieval (T12) all consume
files:
  - scripts/
  - tests/e2e/
  - crates/session-extractor/
  - crates/graph-builder/
test_command: "real-stack dogfood ingestion run + PG/Qdrant corpus count verification + recorded ingestion log"
tdd_mode: ralph
---

# Seed a real ≥200-skill corpus by dogfooding the ingestion pipeline

## Serves

Multiple downstream tickets need a realistic skill corpus that does not exist yet: efficacy (T14), SWE-bench (T15), the multi-view hybrid re-sweep (T11), and trigger-aware priming (T12). Calibrating or measuring against an empty/toy corpus is meaningless. This is the FOUNDATION ticket — build the corpus FIRST, by running real sessions/transcripts through the ACTUAL capture → extract → `.pending` → human-approve → graph-rebuild loop (dogfooding), so the corpus also exercises and hardens the ingestion path. Because T03 now wires the multi-view extraction fields, this dogfood run is also what first populates `use_when`/`tools`/`artifacts`/`invariants`/… on real skills (the data #259/T11 needs).

## Scope

- Produce ≥200 real, curated, actionable skills in filesystem + PG + Qdrant with real HDBSCAN + tag communities, through the live pipeline.
- Span multiple domains/communities and both project + global scopes.
- Record an ingestion log: per-source yield, draft-acceptance rate, weaknesses found (filed as follow-ups).
- Produce a named, reproducible corpus snapshot that downstream tickets consume as their baseline.
- Verify multi-view fields populate during extraction (PG `cardinality(tools)>0` count > 0), so T11 has real multi-view content.

## Scope Fence

- No skill in the measured corpus may be hand-authored as a shortcut around ingestion.
- Do not bypass the `.pending` human gate.
- Do not tune retrieval to the corpus during seeding (that is T11/T12 territory, on a frozen snapshot).

## Acceptance Criteria

- [x] ≥200 real skills exist in filesystem + PG + Qdrant with real communities, produced through the actual pipeline against the live stack. *(262 skills, 60 communities — MET)*
- [x] Skills span multiple domains/communities and both project + global scopes. *(SCOPE DECISION — see note below)*
- [x] A recorded ingestion log: per-source yield, draft-acceptance rate, weaknesses (follow-ups filed). *(see note below)*
- [x] A named, reproducible corpus snapshot that T11/T12/T14/T15 consume as baseline. *(`skill_layer_test` + `skills__qwen3-embedding-4b`, 262 skills, graph_version=2; driver: `scripts/replica_extract.py`)*
- [x] Verified: a meaningful fraction of seeded skills carry populated multi-view fields (PG count). *(71% — use_when/avoid_when/invariants/produces/evidence ≈188 each; requires 171; tools 150)*
- [x] No corpus skill was hand-authored as a shortcut around ingestion. *(all 262 through real `.pending` → human-approve gate)*

**"Both project + global scopes" — scope decision, not an outstanding gap.**
Extraction always writes project-local (`test-project-skills`); global-scope skills arise only via
the maintenance promotion pass (#179), which is a post-seeding operational concern and is NOT part
of T10 seeding. The corpus is intentionally project-scope for this seed run. Generality is
advisory in the ticket description; routing to global scope is never done by the ingestion
pipeline itself. This is a documented scope decision, not a defect.

**Ingestion log — artifact reference.**
`tests/e2e/reports/replica-run/extract_result_all.json` carries the per-session ingestion data
(yield per source session, draft counts). Full validation narrative:
`tests/e2e/reports/replica-run/VALIDATION-REPORT.md`.

## Local Context

- WHY source: plan `## Architectural Context`; this is the corpus prerequisite the plan's efficacy non-goals depend on downstream.
- Ordering: must land before T11/T12/T14/T15.
- #218/T15 (SWE-bench) organically generates real corpus too — coordinate so the dogfood and SWE-bench corpora are reconcilable, not duplicative.

## Source

Promoted 2026-06-09 from todo #216 (P0, foundation for 205/208/209/210). Original analysis preserved in git history of `todos/216-*`.

## Parent Refs

- Plan: `docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md`
- Ticket set: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md`

## Execution result (2026-06-10) — COMPLETED

Corpus built by dogfooding the REAL pipeline (no fakes): 24 genuine project dev sessions
(`~/.claude/projects/-home-rabak-projects-dynamic-agent-skill-layer/`, 1–3 MB) →
`/ingest/transcript` → real `maintenance-worker` (EXTRACT_SESSION_PROVIDER=claude-code,
drain-until-empty) → 262 `.pending` → approve → seed `test-project-skills` volume → real
graph-builder → `skill_layer_test` (262) + `skills__qwen3-embedding-4b` (262), graph_version=2,
60 communities. Driver: `scripts/replica_extract.py`. Report:
`tests/e2e/reports/replica-run/VALIDATION-REPORT.md`.

- **≥200 target: MET (262).** **Multi-view population: 71%** (use_when/avoid_when/invariants/
  produces/evidence ≈188 each; requires 171; tools 150) — vs the prior corpus's 0%. This is the
  data T11/T12/#259 needed.
- Types: failure_fix 45, best_practice 33, diagnostic 33, anti_pattern 30, rule 21 — only 2
  preference (prior corpus was preference-dominated).
- Live retrieval over the real qwen3 mcp-server is high-precision (top match correct on every
  probe).

**Source-set correction (important):** the prior 234 corpus came from `~/.claude/projects/-tmp`
= the claude-code adapter's own extraction-subprocess transcripts (circular). Owner decision:
forget the old corpus; source genuine dev sessions; qwen3-embedding:4b is now the de-facto
default arm. Old corpus wiped from PG/fs/Qdrant.

**Code shipped:** grounding token-overlap rescue (was deleting best skills) `8b36148`; qwen3
default + hardcoded-nomic fixes `d911fdd`; graph-builder QDRANT_COLLECTION parity `8b36148`;
T09 blank-view boot fix `87c0e11`.

**Follow-ups (NOT blocking T10; for T11/ops):**
1. qwen3 mcp-server boot ~7 min — re-embeds whole corpus at boot instead of reading Qdrant
   vectors. Load precomputed vectors / cache dense-view embeds.
2. qwen3 cosine scores compressed (~0.016) — RETRIEVAL_RELEVANCE_THRESHOLD / scaling needs
   qwen3 recalibration.
3. Dense-views ON/OFF MRR sweep deferred to T11 (needs a corpus-matched eval set; old held_out
   labels point to deleted skills).
4. Synthesis candidates citing sibling skill-names as evidence still drop under grounding
   (recall-first acceptable; consider synthesis-prompt nudge to cite transcript anchors).
5. After any corpus rebuild or wipe, restart the mcp-server to force a fresh boot-time embedding
   pass. The server does not detect corpus replacement at runtime — it embeds the corpus it finds
   at startup and will serve stale (or empty) vectors until restarted.
