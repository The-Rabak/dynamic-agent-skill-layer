# Session handoff — reacquaint prompt (written 2026-06-15, after T12 + the embedding-model experiment)

Paste everything below the line into the fresh session. Purpose: reacquaint the next iteration with
the project and the EXACT stopping point, then present the owner with the next decision. Do not start
executing work from this prompt; orient, verify git state, then discuss.

---

## What this project is

**Dynamic Agent Skill Layer** (`/home/rabak/projects/dynamic-agent-skill-layer`, Rust workspace,
branch `feat/v-1-7`). A persistent, human-gated skill memory for coding agents: real claude-code
sessions → LLM extraction distills durable skills (multi-view fields, typed graph edges) → `.pending`
drafts pass a HUMAN gate → corpus (Postgres + Qdrant, qwen3-embedding:4b, model-keyed collections) →
retrieval over MCP (`find_skill`, `compile_context` SessionStart priming, `search_skill_graph`)
injects them into future sessions. Culture = measurement-first: every claim drives the REAL running
mcp-server over HTTP; pre-registration before data; no number without its raw artifact; negative
results are recorded and acted on.

The critical path is the real-usage spine **T18 (instrument) → T12 (priming mechanism) → T15 (efficacy
gate)**. T18 and T12 are now DONE/measured; T15 is the downstream goal. Standing law unchanged from the
prior handoff — read `docs/plans/2026-06-12-next-session-handoff-prompt.md` "Pitfalls and standing law"
in full (no stubs/fail-loud; measurement drives the real server; serialize ALL heavy actions / no
parallel cargo — WSL2 crash; never delete this session's outputs; pre-registration discipline; human
gate untouchable; `docs/plans/` is gitignored → `git add -f`; pin `OLLAMA_EMBED_MODEL`).

## Read in this order (then verify git state)

1. `git log --oneline -15` and `git status` — confirm HEAD ≈ `0d22480` (T12 arm-A2 measured) and a
   clean tree. The 9 `T12 …` commits + the experiment commits are the recent work.
2. Memory `v17-t12-priming-mechanism` (the full T12 verdict + the 2026-06-15 embedding-experiment
   follow-up) and `v17-t18-priming-instrument-baseline` (the instrument T12 was graded on).
3. `docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/12-trigger-aware-retrieval-priming-mode.md`
   — status `implemented-owner-decisions-deferred`; read `status_note_2026_06_15_t12_done` + the AC.
4. `docs/execution-sessions/work-2026-06-15-t12-priming/unit-04-measurement-verdict.md` — the measured
   verdict + the owner-deferred decisions + the latency curve.
5. `docs/plans/2026-06-15-embedding-model-server-latency-quality-experiment.md` — the 4-arm plan AND
   the measured **Arm A2** results (qwen3-embedding:0.6b). Raw artifacts under
   `tests/e2e/reports/retrieval/t12_priming_*.json` + `t12_task_quality_a2_0p6b.json`.

## State of the board (V1.7, Phase B)

**T12 (trigger-aware priming) — BUILT + MEASURED, owner decisions DEFERRED.**
- Shipped (Units 1–4, Task path byte-identical, gates green): typed `RetrievalIntent {Task,Priming}`
  threaded through `retrieve()`; `compile_context` maps `TriggerKind::SessionStart`→Priming (+ hook
  trigger wiring); query-side multi-view (segment→`embed_batch`→max-merge); Priming-scoped floor 0.30 +
  recurrence/freshness ranker (`skills.created_at`→`RetrievalSnapshot.skill_age_days`); all
  `RETRIEVAL_PRIMING_*` env-tunable + compose passthrough.
- Measured on the T18 `session_start` stratum through `compile_context`: neg-control PASS (62.5%
  crater); baseline reproduced T18 0.0685 exactly; primed cov@3 0.0805 → paired **+0.012, p=1.0 →
  FAILS the +0.10 bar**. **Every reranking signal INERT @262** (recurrence Δ0.000 @ w=0.1 & 0.6;
  freshness-slot isolated Δ0.000; centrality default-DROP; **multi-view Δ0.000**, identical at caps
  1/2/3/8). The ONE real win is the **lower floor → no_match 14%→0%** (prime never empty), at
  single-embed latency. Multi-view verbose p95 2240ms@8 BREACHES the 500ms budget.
- **Owner gates (both DEFERRED):** (#1 default-ON) owner = "keep multi-view ON, flag latency for
  reconsideration" → default `priming_max_segments=8`, production flip NOT done; (#2 per-signal
  verdicts) owner = "still reviewing artifacts" → verdicts NON-FINAL, all rerank code retained.

**Embedding-model experiment — Arm A2 (qwen3-embedding:0.6b on Ollama) MEASURED; the latency flag is
substantially answered.** Root cause of the latency breach is VRAM: `4b` (~8GB) doesn't fit the host
RTX 2060 (6GB) → runs 11%/89% CPU/GPU. `0.6b` (1024-dim) loads **100% GPU**:
- Latency: Task p95 283ms; priming single-embed verbose **411ms (<500ms ✓)**; multi-view 1007ms (vs
  4b 2240ms).
- Task quality (find_skill snapshot_dense probe, validated `retrieval_metrics`): **clears ALL T11
  floors** — MRR@3 0.686 (4b 0.743, floor 0.64), cand-recall@50 0.752 (floor 0.68), no_match precision
  **1.00** (4b 0.92). Bounded dip ≈ 4b-without-dense-views.
- Priming quality: **IMPROVED** — cov@3 0.0984 (4b 0.0805), neg-control 66% crater, no_match 0%,
  multi-view no longer inert.
- **A2 verdict: 0.6b is a viable adoption candidate** (latency fixed, gate cleared, priming improved).
  NOT YET DONE before a flip: (a) full `--gate` re-confirm needs the **0.6b Qdrant collection
  re-seeded** (graph-builder skips unchanged skills via its content-addressed idempotency key →
  `skills__qwen3-embedding-0-6b` stays empty; needs a deliberate full corpus re-seed like the original
  qwen3 adoption) so the α=0 crater canary can run; (b) **recalibrate the 0.48 Task / 0.30 priming
  floors** on the 0.6b score scale; (c) **TEI arms A1/A3 NOT measured** (second-order: better batching
  for multi-view; biggest payoff paired with a VRAM-fitting model).

**Done earlier:** T01–T11, T13, T17, T20, T21, T18 (priming instrument VALID, before-number 0.0685).
**Deferred:** T19 (cross-project recurrence — needs a multi-project corpus). **Downstream:** T15
(primary efficacy gate — measures compounding THROUGH the working priming path; gated on T12).

## New pitfalls/quirks surfaced this session (add to the standing law)

- **Embedding arm swap is supported + exercised** (model-keyed collections; nomic-768 / qwen3-4b ran
  before). To switch: evict the loaded model (`ollama stop <model>`), set `OLLAMA_EMBED_MODEL`, restart
  mcp-server (boot re-embeds the snapshot from PG text; cache keyed by `(skill_id,view_kind,model_name)`
  → cold for a new model, ~155s for 262 in 0.6b). For a CLEAN latency comparison, evict the prior model
  first (4b at 11%/89% CPU/GPU competes for the 6GB VRAM). Always RESTORE to 4b after (it's the
  validated production arm). qwen3-embedding:0.6b is pulled and installed.
- **`retrieval_sweep.py --gate` guards on a non-empty model-keyed Qdrant collection** — a brand-new
  embedding model fails this until graph-builder re-seeds it. `snapshot_dense` Task quality is
  measurable WITHOUT Qdrant via `scripts/t12_task_quality_probe.py` (find_skill, validated metrics) —
  but that skips the α=0 crater canary, so it's a probe, not the full gate.
- New measurement scripts: `scripts/t12_priming_sweep.py` (priming on the session_start stratum, both
  arms, neg-control), `scripts/t12_task_quality_probe.py` (Task quality via find_skill). Qdrant host
  REST port = **16333**; mcp on 3001; graph-builder 8080.

## Where we stopped, exactly

Last commits (newest first): `0d22480` (T12 arm A2 measured) ← `dd14913` (embedding experiment plan)
← `60da508` (T12 disposition) ← `a773230` (keep multi-view ON, owner) ← … ← `4fb0521` (T12 Unit 1).
Working tree clean. **Stack is UP and RESTORED to the validated 4b arm** (`/health` healthy,
embedding_arm model=qwen3-embedding:4b dim=2560). Nothing in flight. No PR (mid-v1.7 on the shared
feature branch).

## The decision to put to the owner (do NOT execute before they choose)

T12's mechanism is built and the latency tension is now answerable by data. The open thread is **how to
close T12's latency flag + finalize its owner verdicts, then proceed to T15.** Present these paths:

1. **Adopt 0.6b** (the measured-viable latency fix): re-seed the corpus into the 0.6b Qdrant collection,
   run the full `--gate` (α=0 confirm) + recalibrate the 0.48/0.30 floors on the 0.6b scale, then flip
   the embedding arm. This unblocks keeping multi-view ON within budget AND unblocks T15.
2. **Measure TEI (arms A1/A3)** first — quality-neutral server change; biggest multi-view-latency
   headroom when paired with 0.6b. ~half-day adapter (EmbeddingService trait seam exists).
3. **Finalize T12 owner decisions** independent of the embedding path: per-signal keep/drop verdicts
   (data says DROP all four reranking signals @262, code retained); default-ON flip once latency is
   resolved by (1) and/or (2).
4. **Then T15** (efficacy compounding through the now-working priming path).

Owner's standing rule: reassess the next work run together BEFORE launching `/workflows:work`. Orient,
verify git/stack state matches this handoff, surface any drift, then present the launch decision.
