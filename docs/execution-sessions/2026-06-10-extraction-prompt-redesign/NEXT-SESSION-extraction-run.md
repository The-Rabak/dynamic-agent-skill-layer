# Next session: multi-view extraction run + replica validation (executes T10)

**Created:** 2026-06-10 · **Executes:** T10 (`10-seed-corpus-self-ingestion-foundation.md`), now unblocked
by the extraction-prompt redesign. Feeds T11 (hybrid re-sweep), T09 (dense-views sweep), T12/T14/T15.

This is the handoff for the run the owner asked for: *"run another extraction with the new prompts,
into a replica PG + replica Qdrant, then run the full validation arm against original vs replica."*

---

## 0. What is already DONE (this session, 2026-06-10)

All in the working tree, compiled + unit-tested, **NOT yet committed**:

- **Extraction prompts re-architected from the ground up** — `crates/infrastructure/src/extraction/prompt_contract.rs`:
  - Multi-view fields (`use_when`/`avoid_when`/`requires`/`invariants`/`tools`/`artifacts`/`produces`)
    elevated from optional afterthoughts → **first-class CORE views**.
  - **Assess-first CoT gate**: output is `{"assessment": "...", "candidates": [...]}`; the model judges
    whether anything durable exists BEFORE extracting; an empty result is correct for throwaway sessions
    (no output pressure). `assessment` is required in the Claude tool schema; captured + `tracing::info!`-logged
    by all three providers (ollama/claude/claude-code) so zero-candidate sessions are explainable.
  - "Capture the iteration": converged solution → `procedures`, dead-ends → `avoid_when`, trigger failure
    → `use_when`. Literal-keyword triggers. `type` taxonomy + `evidence` grounding anchors added.
  - Research-backed quality dimensions (CL-bench / ReasoningBank / SkillRevise / Trace2Skill / AWM / ACE).
- **Grounding validator** — `crates/session-extractor/src/grounding.rs` + wired into `orchestrator.rs`
  (Step 5b): drops candidates whose cited `evidence` is wholly absent from the transcript (fabrication
  guard); empty-evidence kept (recall-first).
- **Domain + writer**: `ExtractedSkillCandidate` gains `skill_type` (JSON `type`) + `evidence`; writer
  persists `type:` + `evidence:` frontmatter (skip-if-empty). Synthesis prompt enriched for cross-episode
  contrast. `docs/reference/skill-md-format.md` updated.
- **T09 blank-view boot crash FIX** — `crates/mcp-server/src/lib.rs` `embed_dense_view_skipping_blank`:
  empty multi-view fields no longer crash mcp-server at boot (skip blank views → fall back to e_summary).
- Tests green: domain 13, infrastructure 191+, session-extractor 174 (incl. 5 grounding), maintenance
  roundtrip 3; workspace compiles. Full prompt text reviewed + approved by owner.
- Design doc: `docs/design/2026-06-10-multiview-extraction-prompt-redesign.md` (APPROVED).

**STEP 0 for next session: commit all of the above BEFORE the heavy rebuild** (WSL2 dirty-tree crash
risk — memory: a crash zeroes uncommitted working-tree files). Suggested split: one commit for the T09
blank-view fix, one for the extraction-prompt redesign.

---

## 1. Readiness verdict

**The prompts are READY.** The extraction RUN is **NOT one-shot ready** — it has real prerequisites and
one genuine design decision (validation eval set). Address §3 before running.

---

## 2. The pipeline (how the run actually works)

Real loop (dogfooding, no shortcuts — T10 scope fence forbids hand-authoring):

```
session transcript(s) (JSONL)
  → maintenance-worker drains them (crates/maintenance/src/transcript_drain.rs
      → session_extractor::SessionExtractor → run_orchestration → the NEW prompts)
  → writes .skills/<slug>/SKILL.md.pending   (now WITH multi-view fields + type + evidence)
  → HUMAN APPROVE gate: rename .pending → SKILL.md (bulk for a corpus run)
  → graph-builder ingests SKILL.md → PG `skills` rows + Qdrant vectors + HDBSCAN/tag communities
  → mcp-server serves retrieval over the snapshot
```

Provider: `EXTRACT_SESSION_PROVIDER=claude-code` (frontier; plan: 0.68 vs local Gemma 0.256 non-empty
rate). Routing auto-selects the frontier tier for claude-code.

---

## 3. Prerequisites / decisions to address BEFORE the run

### A. Source transcripts  ❓ identify
The original 234 corpus came from a **claude-code campaign over real Claude Code sessions on this machine**
(see `tests/e2e/reports/claude-code-campaign/FULL-RUN-claudecode-20260607-072001.log` for the original
source list / yield). Repo fixtures are only a handful (`tests/fixtures/*session*.jsonl`) — NOT the full
corpus source. **Decision:** which sessions to extract? Options: (a) same source set as the original
campaign (apples-to-apples comparison), (b) a fresh/larger set of `~/.claude/projects/**/*.jsonl`. (a) is
better for the original-vs-replica comparison the owner wants.

### B. Replica isolation  ⚙ set up (non-destructive)
Keep the original 234 corpus untouched. The replica run needs its own:
- **PG database** (e.g. `skill_layer_replica`) or a clearly separate namespace,
- **Qdrant collection** (e.g. `skills__nomic-embed-text__replica` via `QDRANT_COLLECTION` override),
- **skills volume / `.skills` dir** (separate from `test-project-skills`),
so extraction → approve → ingest writes into the replica only. Wire via compose env overrides; do not
mutate the live `skill_layer_test` DB or `skills__nomic-embed-text` collection.

### C. claude-code in-container auth  ✔ verify
The extraction container must invoke the `claude-code` CLI with valid auth. The original campaign did this,
so the harness exists — just confirm auth/subscription is still valid in-container before a long run.

### D. Validation eval set  ⚠ DESIGN DECISION (the real "what's missing")
`tests/fixtures/retrieval_quality_234_corpus_labeled.json` maps held-out QUERIES → relevant **skill IDs in
the ORIGINAL 234 corpus**. A freshly-extracted corpus has DIFFERENT skill IDs/content, so those labeled
positives **do not transfer** — you cannot score the new corpus with the old labels directly. Pick a
validation strategy:
1. **Field-population verification (primary, cheap, = T10 acceptance):** PG counts of skills with non-empty
   `use_when`/`avoid_when`/`requires`/`invariants`/`tools`/… — did the new prompts actually populate the
   views? (Old corpus = 0; target = a meaningful fraction.) Plus assessment-log review: zero-candidate
   sessions explained, not garbage.
2. **Dense-views ON/OFF sweep on the NEW corpus (primary):** now MEANINGFUL (fields populated). This is the
   T09 sweep that was a structural ≈0 on the empty corpus. `scripts/t09_dense_views_sweep.py` exists.
3. **Live A/B on shared queries (comparative — matches "validation arm against each in turn"):** run the
   SAME held-out queries against original vs replica corpus over the real mcp-server, LLM-judge top-k
   relevance. Needs no pre-labeled positives for the new corpus. Best apples-to-apples for "did richer
   multi-view skills retrieve better for the same tasks?"
4. **Fresh labeled held-out (rigorous, expensive):** label new-corpus skills for MRR/nDCG. Only if 1–3 are
   inconclusive.
Recommended: do 1 + 2 + 3. Decide before running so the run is set up to capture what 3 needs.

### E. Bulk approve recipe  ⚙ confirm
Corpus-scale `.pending` → `SKILL.md`. The original campaign had a bulk-approve step; reuse it (do not
bypass the gate — T10 scope fence).

### F. One rebuild  ⚙ folds in the T09 fix
Single in-container rebuild of `mcp-server` + `graph-builder` (+ the extraction worker image) picks up BOTH
the T09 blank-view fix AND the new extraction code. The stack infra (postgres/redis/qdrant/ollama) is
currently UP from this session; mcp-server is DOWN (crashed pre-fix — the fix is in the tree, needs rebuild).

---

## 4. Concrete run recipe (once §3 decisions are made)

1. **Commit** the working tree (§0).
2. **Rebuild** images: `docker compose -f docker-compose.test.yml build mcp-server graph-builder` (+ worker).
3. **Stand up replica isolation** (§B): replica DB + `QDRANT_COLLECTION` override + separate `.skills` dir.
4. **Drive extraction** over the chosen source sessions (§A) with `EXTRACT_SESSION_PROVIDER=claude-code`
   via the maintenance-worker drain path. Watch the `… extraction assessment` logs.
5. **Bulk-approve** `.pending` → `SKILL.md` (§E).
6. **Let graph-builder ingest**; verify corpus: PG skill count ≥200 AND multi-view population counts > 0
   (`SELECT count(*) FILTER (WHERE cardinality(use_when)>0) …`).
7. **Validate** (§D): field-population report + dense-views ON/OFF sweep on the new corpus + live A/B vs
   the original corpus. Record honestly.
8. **Snapshot** the new corpus (named, reproducible) for T11/T12/T14/T15. Update T10 status; file follow-ups.
9. **Clean up** build artifacts + scratch dirs (memory: IRONCLAD cleanup rule).

---

## 5. Open decisions for the owner (resolve at session start)
- **D1 (source):** same sessions as the original campaign, or a fresh/larger set? (§A)
- **D2 (validation):** confirm the §D strategy (recommend field-pop + dense-views sweep + live A/B).
- **D3 (grounding strictness):** keep the softened §7 (empty-evidence kept; ≥1 anchor must ground), or
  enforce stricter? (Recommend keep — recall-first.)
- **D4 (scale):** target corpus size (T10 says ≥200) and per-window candidate cap (currently none — the
  assess-first gate + quality bar handle volume; add a hard cap only if output is noisy).

## 6. References
- Design + approved prompts: `docs/design/2026-06-10-multiview-extraction-prompt-redesign.md`
- T10 ticket: `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/10-seed-corpus-self-ingestion-foundation.md`
- T11 (hybrid re-sweep, consumes this corpus): `.../11-corpus-multiview-resweep-hybrid-validation.md`
- T09 session (blank-view fix + dense-views sweep pending): `docs/execution-sessions/work-2026-06-10-T09/`
- Original campaign log: `tests/e2e/reports/claude-code-campaign/FULL-RUN-claudecode-20260607-072001.log`
- Extraction driver: `crates/maintenance/src/transcript_drain.rs`; orchestration: `crates/session-extractor/src/orchestrator.rs`
- Prompts: `crates/infrastructure/src/extraction/prompt_contract.rs`
- Dense-views sweep script: `scripts/t09_dense_views_sweep.py`
