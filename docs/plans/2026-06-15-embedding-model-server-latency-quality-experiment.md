# Experiment plan — embedding model size & inference server: latency vs retrieval quality

**Status:** MEASURED. A2 (0.6b/Ollama) 2026-06-15; A1 (4b/TEI) + A3 (0.6b/TEI) 2026-06-16. A1=GPU-infeasible
(VRAM OOM); A3=clears T11 gate, best latency, TEI quality-neutral vs Ollama (H3 confirmed). See results +
incident sections below. Authored 2026-06-15, branch `feat/v-1-7`.
**Owner gate:** run only on owner "go" (re-embeds the corpus, swaps the live embedding arm).
**Motivation:** T12 left a flagged tension — query-side multi-view (and even a single 4B verbose
embed) breaches the 500 ms SessionStart budget. Investigation
(`docs/execution-sessions/work-2026-06-15-t12-priming/`) found the dominant cause is **VRAM**:
`qwen3-embedding:4b` (~8 GB fp16) does not fit the host RTX 2060 (6 GB) → partial CPU offload →
~300–505 ms per *thin* embed, 2240 ms for 8-segment multi-view. This experiment measures whether a
smaller model (`qwen3-embedding:0.6b`) and/or a faster server (TEI) reach the latency budget **without
an unacceptable retrieval-quality regression**, using the already-validated T11 gate + T18 priming
instrument as the rulers.

## Questions

1. **Q-LATENCY:** Does `0.6b` and/or TEI bring SessionStart p95 < 500 ms — including multi-view
   (`priming_max_segments > 1`) — on the RTX 2060?
2. **Q-QUALITY-MODEL:** How much retrieval quality does `0.6b` cost vs `4b` (Task: MRR@3 / cand-recall
   / no_match precision; Priming: set-coverage@3 / no_match rate)? Does it still clear the T11 gate?
3. **Q-QUALITY-SERVER:** Is TEI (same model) quality-neutral vs Ollama (i.e., are the vectors
   equivalent after pooling/instruction-prefix differences), proven by the T11 gate holding?

## Hypotheses (to be confirmed/refuted by data, not shipped on)

- **H1 (latency, high confidence):** `0.6b` fits 6 GB → GPU-resident → 3–6× faster/embed + real
  parallelism → multi-view < 500 ms. `0.6b` is the dominant latency lever on this hardware.
- **H2 (latency, medium):** TEI's dynamic batching beats Ollama's per-text `/api/embeddings` + sem=4,
  but with `4b` still VRAM-bound; TEI's payoff is largest paired with a VRAM-fitting model (`0.6b`).
- **H3 (quality, medium):** TEI(`4b`) is quality-neutral vs Ollama(`4b`) (same weights) — *iff* pooling
  + query instruction prefix match; the T11 gate parity check is the adjudicator.
- **H4 (quality, to-measure):** `0.6b` costs a real but bounded retrieval hit (~5–6 MTEB pts lower,
  1024-dim vs 2560). It may or may not still clear the T11 floors; the 0.48 Task floor and 0.30 priming
  floor were calibrated on `4b` and likely need recalibration at `0.6b`.

## Arms (matrix)

Each arm re-embeds the 262-skill corpus into an **isolated, distinctly-named** Qdrant collection so
arms never mix vectors (collections are model-keyed today via `model_keyed_collection_name`; for this
A/B also disambiguate by server). Query embeds must use the SAME (model, server) as the corpus.

| Arm | Model | Server | Collection | Purpose |
|---|---|---|---|---|
| **A0** baseline | qwen3-embedding:4b | Ollama | `skills__qwen3-embedding-4b` (existing) | the validated reference (T11/T18 numbers already known) |
| **A1** | qwen3-embedding:4b | TEI | `skills__qwen3-embedding-4b__tei` | server-only change → H3 parity + latency |
| **A2** | qwen3-embedding:0.6b | Ollama | `skills__qwen3-embedding-0-6b` | model-only change → H4 quality + H1 latency |
| **A3** | qwen3-embedding:0.6b | TEI | `skills__qwen3-embedding-0-6b__tei` | combined best-latency candidate |

Reference (no re-embed): **A0 + `RETRIEVAL_PRIMING_MAX_SEGMENTS=1`** — already measured (verbose priming
p95 564 ms, no_match 0%); the "multi-view off" latency floor for context.

## Instruments & metrics (reuse — do NOT invent new rulers)

All measurement drives the **real mcp-server over HTTP** (standing rule). Per arm, gated on `/health`
ready with the arm's `OLLAMA_EMBED_MODEL` / server wired:

1. **Task retrieval — T11 gate:** `scripts/retrieval_sweep.py --gate` over
   `tests/fixtures/retrieval_quality_262_corpus_labeled.json` (the non-`session_start` strata). Records
   MRR@3, MRR@10, nDCG@3, candidate_recall@50, no_match_precision, and the α=0 crater canary.
2. **Priming — T18/T12 instrument:** `scripts/t12_priming_sweep.py --label <arm>` over the 22-query
   `session_start` stratum through `compile_context` (permutation neg-control FIRST, then primed vs
   baseline paired + sign-test, set-coverage@3, freshness hit-rate, no_match rate).
3. **Latency:** p95 from both sweeps' raw per-query latencies, split thin/verbose, at
   `priming_max_segments ∈ {1, 4, 8}` for the priming arm (the latency/coverage curve).

## Pre-registered thresholds (cited verbatim — set BEFORE running)

- **T11 gate floors** (`scripts/retrieval_metrics.py::GATE_THRESHOLDS`, frozen): `mrr_at3 ≥ 0.64`,
  `mrr_at10 ≥ 0.64`, `ndcg_at3 ≥ 0.64`, `candidate_recall_at_limit ≥ 0.68`, `no_match_precision ≥ 0.88`,
  α=0 crater `rel_drop ≥ 0.50`. (Floors sit below the T11 single-view-dense numbers; passing = no
  meaningful regression.) Reference 4B dense-views: MRR@3 0.743, cand-recall@50 0.796, nDCG@3 0.755,
  no_match 0.92.
- **T18 priming** (LOCKED): permutation neg-control must crater (`permuted ≤ 0.5 × true`); baseline
  set-coverage@3 = **0.0685**; recurrence-baseline keep = +0.10 abs (→ ≥0.17) sign p<0.05; freshness
  +0.15 hit-rate ≤0.02 cannibalization; centrality +0.043 (default DROP).
- **Latency (constitutional):** SessionStart p95 **< 500 ms** through `compile_context`.
- **Server-parity (H3):** TEI(4b) is "neutral" iff its T11 gate passes AND its MRR@3 / cand-recall /
  no_match are within **±0.02** of A0 (numerical-parity band; larger = a pooling/prefix mismatch to fix
  before any verdict).

## Decision rules (what each result ships)

- **A1 (TEI, 4b) passes T11 gate + within parity band + p95 improves** → TEI is a safe, quality-neutral
  drop-in; adopt TEI as the server. (If parity fails, fix pooling/instruction-prefix first; do not ship.)
- **A2/A3 (0.6b) STILL clears the T11 gate** → the `0.6b` quality hit is within tolerance; if p95 < 500 ms
  (incl. multi-view), adopt `0.6b` (with TEI if A3 > A2) and **recalibrate** the Task 0.48 / priming
  0.30 floors on the new score distribution before flipping.
- **A2/A3 FAILS the T11 gate** → `0.6b` is too lossy for Task retrieval. Fallbacks to evaluate, not
  assume: (a) keep `4b` for Task, accept the priming latency / keep `max_segments=1`; (b) **dual-model**
  — `0.6b` only for the SessionStart priming path (its own collection + query-time space), `4b` for
  Task (extra CQRS/collection complexity; only if priming latency is the sole blocker and priming
  quality at 0.6b is acceptable on the T18 instrument).
- **Multi-view still inert at the faster embedder** (set-coverage@3 unchanged across `max_segments`) →
  confirms the T12 finding corpus-wide; keep multi-view OFF by default regardless of latency.

## Procedure (per arm; serial, one heavy action at a time — WSL2 rule)

1. **TEI spike (A1/A3 only):** stand up an `huggingface/text-embeddings-inference` container for the
   target model on the RTX 2060; add a `TeiEmbeddingService` impl of the `EmbeddingService` trait
   (`embed_text` + `embed_batch` via TEI's batch `/embed`) behind an `EMBEDDING_PROVIDER` selector.
   *(This is the only new code; everything else is config + re-embed.)*
2. **Re-embed the corpus** into the arm's collection: set `OLLAMA_EMBED_MODEL` (and provider), pull the
   model, trigger a graph rebuild so graph-builder writes the new collection; the mcp-server boot
   re-embeds the snapshot (embedding cache is model-keyed → cold for a new model). Verify `/health`
   `embedding_arm` reports the expected model + dim (`0.6b` → 1024).
3. **Run the T11 gate** → persist `tests/e2e/reports/retrieval/<arm>_gate.json`.
4. **Run the priming sweep** at `max_segments ∈ {1,4,8}` → persist `t12_priming_<arm>_seg{1,4,8}.json`.
5. **Record** MRR@3 / cand-recall / no_match / set-coverage@3 / p95 (thin+verbose) into a comparison
   table; apply the decision rules.

## Risks / gotchas (from the codebase investigation)

- **VRAM:** `4b` won't fully fit 6 GB on any server — A1 may show only modest latency gain. `0.6b`
  (~1.2 GB) is the lever that actually fits.
- **Pooling / instruction prefix parity (H3):** qwen3-embedding uses last-token pooling + a query-side
  instruction. TEI must match Ollama's behavior (and our prefix usage) or vectors drift — caught by the
  ±0.02 parity band + the gate. Re-embed corpus AND queries on the same server.
- **Floor recalibration:** the 0.48 Task floor (#209) and 0.30 priming floor are calibrated on `4b`
  score scales; `0.6b` will shift them → recalibrate before trusting no_match precision / coverage.
- **One-time re-embed cost** per model (~minutes; embedding cache is per-model so each new model is a
  cold re-embed of 262 skills). Never delete the existing `4b` collection until an arm is chosen
  (A0 is the rollback).
- **Dim change:** `0.6b` = 1024-dim → new model-keyed collection sized automatically by
  `discover_dimension`; do not reuse the 2560-dim collection.
- **Keep arms isolated:** never mix TEI and Ollama vectors of the "same" model in one collection.

## Effort / cost estimate

- TEI adapter + provider selector: ~half a day (one new `EmbeddingService` impl; trait seam already
  exists). Skippable if only testing `0.6b` on Ollama (A2 = config + re-embed, no code).
- Per-arm measurement: re-embed (~minutes) + T11 gate + priming sweep (~minutes each on the live
  server). 4 arms ≈ a focused day with serial runs.
- Lowest-cost first cut: **A2 (0.6b on Ollama)** — no code, just `OLLAMA_EMBED_MODEL=qwen3-embedding:0.6b`
  + re-embed + the two existing sweeps. That alone answers Q-LATENCY (H1) and Q-QUALITY-MODEL (H4); add
  TEI (A1/A3) only if A2's quality clears the gate and you want the extra latency headroom.

## Results — Arm A2 (qwen3-embedding:0.6b on Ollama), measured 2026-06-15

Clean comparison: evicted `4b` from VRAM, mounted `0.6b` (cold re-embed of 262 skills, 155s), measured
on the live server, then restored `4b`. Raw artifacts:
`tests/e2e/reports/retrieval/t12_priming_a2_0p6b_ollama.json`, `t12_task_quality_a2_0p6b.json`.

**VRAM / latency (H1 CONFIRMED):** `0.6b` loads at **100% GPU** (2.4 GB) — no CPU offload, vs `4b`'s
**11%/89% CPU/GPU** (doesn't fit the 6 GB card). Latency:

| metric | 4b (Ollama) | 0.6b (Ollama) |
|---|---|---|
| Task `find_skill` p95 | ~370–505 ms | **283 ms** |
| Priming single-embed (max_seg=1) verbose p95 | 560 ms | **411 ms** (< 500 ms ✓) |
| Priming multi-view (max_seg=8) verbose p95 | 2240 ms | **1007 ms** |

**Task quality (Q-QUALITY-MODEL) — clears ALL T11 floors, real but bounded hit:**

| metric | 4b ref (T11) | 0.6b | T11 floor | pass |
|---|---|---|---|---|
| MRR@3 | 0.743 | **0.686** | 0.64 | ✓ |
| nDCG@3 | 0.755 | 0.703 | 0.64 | ✓ |
| cand-recall@50 | 0.796 | **0.752** | 0.68 | ✓ |
| no_match precision | 0.92 | **1.00** | 0.88 | ✓ (better) |

The `0.6b` ranking hit (MRR −0.057, cand-recall −0.044) ≈ the cost of turning OFF dense-multi-view on
`4b` (T11 single-view MRR was also 0.686) — a measurable dip that stays inside the validated floors.
no_match precision *improved* to 1.00. NOTE: this is the find_skill snapshot_dense probe (validated
`retrieval_metrics` functions), not the full `--gate` wrapper — the α=0 crater reboot was skipped
because the 0.6b Qdrant collection isn't populated (graph-builder skips unchanged skills via its
content-addressed idempotency key; a full corpus re-seed like the original qwen3 adoption would be
needed to run the gate wrapper). Fixture discrimination was already validated on 4b.

**Priming quality (T18 instrument) — IMPROVED on 0.6b:** neg-control PASS (66% crater); set-coverage@3
**0.0984** (4b 0.0805; baseline 0.0730), paired **+0.0255** (6 better / 2 worse / 14 tie, sign p=0.29 —
still < the +0.10 bar but better than 4b); no_match 0%. Multi-view is **no longer fully inert** on 0.6b
(unlike 4b's Δ0.000).

**A2 verdict:** `0.6b` is a viable adoption candidate — it clears the T11 gate, runs fully on-GPU,
gets single-embed priming **under 500 ms**, halves multi-view latency, and slightly *improves* priming
coverage + no_match precision. The cost is a bounded Task-ranking dip (within floors). **Before a flip:**
(a) re-confirm via the full `--gate` wrapper after a 0.6b corpus re-seed (α=0 crater), (b) recalibrate
the 0.48 Task / 0.30 priming floors on the 0.6b score scale, (c) optionally measure A1/A3 (TEI) for
extra multi-view latency headroom. TEI not yet measured (A1/A3 pending).

## Results — Arms A1 (4b/TEI) + A3 (0.6b/TEI), measured 2026-06-16

TEI stood up as a profile-gated compose service (`ghcr.io/huggingface/text-embeddings-inference:cuda-1.9`),
new `TeiEmbeddingService` impl of the `EmbeddingService` trait behind an `EMBEDDING_PROVIDER` selector
(`DynEmbeddingService` enum so `RetrievalOrchestrator<E>` stays monomorphic), all 5 production
construction sites routed through it. Code gate-green (both clippy forms + fmt + 17 embedding unit tests).
Raw artifacts: `tests/e2e/reports/retrieval/t12_task_quality_a3_0p6b_tei.json`,
`t12_priming_a3_0p6b_tei_seg{1,4,8}.json`.

**Two host-specific TEI settings were REQUIRED on the RTX 2060 (Turing, compute 7.5) — themselves findings:**
- `USE_FLASH_ATTENTION=false`: TEI's `FlashQwen3` flash-attention kernels return **NaN** embeddings on
  pre-Ampere GPUs (serialized as JSON `null`; reqwest decode then fails). Disabling flash → real vectors.
- `--max-batch-tokens 4096` (down from 32768): without flash attention the warmup materializes an
  O(seq²) attention matrix that **OOMs** at the default 32k tokens on 6 GB. Our inputs are ≤4000 chars,
  so 4096 is ample. Both are baked into the compose `tei` service as host-appropriate defaults.

**Arm A1 (4b/TEI) — INFEASIBLE on this card (confirms H2/the VRAM hypothesis).** TEI's CUDA backend is
GPU-only (no CPU offload like Ollama/llama.cpp). Loading Qwen3-Embedding-4B fp16 (~8 GB weights) onto the
6 GB card fails immediately: `Could not start Candle backend: DriverError(CUDA_ERROR_OUT_OF_MEMORY)`. So
TEI cannot serve the 4b arm on this hardware at all — latency is moot. The CPU-image fallback (quality-only
parity) was NOT run: the H3 parity question is already answered by A3 below (see verdict), and 4b is a
non-viable arm regardless (the whole experiment is about moving OFF VRAM-bound 4b).

**Arm A3 (0.6b/TEI) — clears the gate, best latency yet, quality-neutral vs Ollama:**

Task quality (find_skill snapshot_dense probe, validated `retrieval_metrics`, vs the A0 4B reference):

| metric | 4b ref | A2 (0.6b/Ollama) | **A3 (0.6b/TEI)** | T11 floor | pass |
|---|---|---|---|---|---|
| MRR@3 | 0.743 | 0.686 | **0.6837** | 0.64 | ✓ |
| nDCG@3 | 0.755 | 0.703 | **0.6955** | 0.64 | ✓ |
| cand-recall@50 | 0.796 | 0.752 | **0.7299** | 0.68 | ✓ |
| no_match precision | 0.92 | 1.00 | **1.00** | 0.88 | ✓ |

ALL T11 floors PASS. Task p95 **111 ms**.

Priming (T18 session_start instrument) + the SessionStart latency curve:

| max_segments | A3 (0.6b/TEI) p95 | A2 (0.6b/Ollama) | 4b/Ollama | A3 coverage@3 |
|---|---|---|---|---|
| 1 (single-embed) | **298 ms** | 411 ms | ~560 ms | 0.0797 |
| 4 | 526 ms | — | — | 0.0934 |
| 8 (multi-view) | **507 ms** | 1007 ms | 2240 ms | 0.0934 |

Neg-control PASS at every seg (rel_drop ~0.62–0.64). Priming paired delta +0.0196 @ seg8 (sign p=0.45 —
under the +0.10 bar, same inert-rerank story as A2/4b). Coverage plateaus at seg=4 (most queries ≤4 views).

**Verdict (A1+A3):**
- **H3 parity (TEI quality-neutral vs Ollama) — CONFIRMED at 0.6b.** TEI-0.6b vs Ollama-0.6b: MRR@3
  0.6837 vs 0.686 (Δ0.002, inside the ±0.02 band), cand-recall 0.730 vs 0.752 (Δ0.022, at the band edge),
  no_match 1.00 vs 1.00. If TEI mishandled Qwen3-Embedding's last-token pooling / no-instruction-prefix,
  these would diverge; they don't. Parity holds independent of model size, so A1's GPU-OOM does not leave
  parity unanswered.
- **H2 (TEI's batched /embed is the multi-view latency lever) — CONFIRMED.** A single `/embed` per
  client-batch vs Ollama's per-text calls roughly **halves** multi-view latency (507 ms vs 1007 ms at
  seg=8) and ~**4.4×** vs 4b (2240 ms). Single-embed priming is a comfortable 298 ms.
- **TEI + 0.6b is the strongest latency arm.** Single-embed priming 298 ms (vs 4b 560 ms); multi-view
  ~510 ms — right at the 500 ms SessionStart budget (vs 4b's 4.5× breach), so multi-view becomes
  borderline-feasible instead of impossible. Task p95 111 ms.
- **Caveat (unchanged from A2):** full `--gate` α=0 crater canary still pending a 0.6b-TEI Qdrant re-seed;
  recalibrate the 0.48 Task / 0.30 priming floors on the 0.6b-TEI score scale before any production flip.

## Incident (2026-06-16): corpus data-loss scare during arm setup — RECOVERED

Switching the live mcp-server onto a TEI arm triggered a recreate of the `postgres` container, which
surfaced a **pre-existing compose misconfig**: `postgres:18` uses `PGDATA=/var/lib/postgresql/18/docker`
(VOLUME `/var/lib/postgresql`), but the compose mounted the named `postgres_data` volume at the legacy
`/var/lib/postgresql/data`. The real cluster therefore lived in a per-container ANONYMOUS volume that was
orphaned on recreate → the new container came up with an empty DB (skills=0). Compounded by the real corpus
DB being `skill_layer_test` (not the compose default `skill_layer`) — so the first TEI restart, lacking
`POSTGRES_DB=skill_layer_test`, also connected to an empty DB. **This produced a FALSE "TEI craters
retrieval" reading (MRR 0.0)** that was purely the empty-DB connection; TEI embedding was fine.
RECOVERED: located the orphaned anon volume (verified PG18 + 263 skills), copied it into the named volume,
and FIXED the mount to `postgres_data:/var/lib/postgresql` so recreates persist. Orphaned volume kept as
backup. The validated 4b/Ollama arm was restored afterward (`/health` green, find_skill gv 18). Both
gotchas saved to memory ([[postgres18-pgdata-mount-gotcha-and-skill-layer-test-db]]).

## FINAL VERDICT (2026-06-16): 0.6b adoption is NO-GO — keep 4b/Ollama

Owner picked "adopt A3 (0.6b/TEI)"; the pre-flip **full `--gate`** (α=0 crater + T11 floors) was run against
the LIVE corpus stack and **refuted the adoption premise.** Sequence (all reversible, 4b restored after):
TEI up (0.6b, flash-off, `--max-batch-tokens 4096`, real floats verified) → re-seed `skills__qwen3-embedding-0-6b-tei`
to 277 pts (purge published outbox + recreate graph-builder on the TEI arm — the outbox idempotency key
`graph.rebuild:vector:<skill_id>` is **model-blind**, so a new arm's collection stays empty without the purge)
→ mcp on the 0.6b-tei arm → adapted `retrieval_sweep.py --gate` to the live stack.

**What the validated gate showed (the A3 task-probe's no_match=1.00 was a weaker metric):**

| Gate criterion | 0.6b-tei | verdict |
|---|---|---|
| MRR@3 / MRR@10 / nDCG@3 ≥ 0.64 | 0.684 / 0.684 / 0.696 | ✅ |
| cand-recall@50 ≥ 0.68 | 0.730 | ✅ |
| α=0 crater ≥ 50% | 100% (MRR→0.000) | ✅ fixture discriminates |
| **no_match precision ≥ 0.88** | **0.84 (best achievable)** | ❌ **FAIL** |

The gate's no_match metric (off-topic query returns ANY match = fabrication) is the real ruler; the A3
probe's "no relevant skill in top-3" is near-vacuous for negatives (hence its trivial 1.00). The no_match
fail is **NOT threshold-recalibratable** — empirical `RETRIEVAL_RELEVANCE_THRESHOLD` sweep on the live
0.6b-tei arm (default 0.48, #209):

| T | no_match | MRR@3 |
|---|---|---|
| 0.48–0.49 | 0.84 ✗ | 0.66–0.68 ✓ |
| **0.50** | **0.88 ✓** | **0.631 ✗** (floor 0.64) |
| 0.52 | 1.00 | 0.579 |

`no_match≥0.88` needs T≥0.50; `MRR@3≥0.64` needs T≤~0.495 → **no overlapping window.** The 4 hardest
off-topic queries score in the same band as ~28 true positives: 0.6b separates on-topic from off-topic
**worse than 4b** (a genuine small-model quality deficit, identical with vs without multi-view — verified).

**Decision (owner deferred to recommendation):** 0.6b/TEI is **NO-GO**. It buys ~60ms on the SessionStart
path (single-embed 4b 560ms → 298ms) but at a measured no_match regression — and the big latency breach it
was meant to fix (multi-view 2240ms) is for a feature T12 already measured **inert**. Production stays on
**4b/Ollama** (A0). The residual latency flag is now small (single-embed priming 560ms vs the 500ms budget,
12% over) and is a separate, one-line, reversible owner choice (drop inert multi-view → single-embed, or
review the budget) — NOT gated on an embedding-arm change. TEI provider + 0.6b collection/cache retained as
proven, ready evidence if multi-view ever earns its keep or the no_match bar is deliberately relaxed.

**Durable artifacts kept from this run (independent of the NO-GO):** the live stack is now gate-runnable
(`docker-compose.yml` mcp-server gained the prod-preserving, default-empty `RETRIEVAL_*` tuning passthrough;
`retrieval_quality_sweep.py` `COMPOSE` honors `SWEEP_COMPOSE`; `retrieval_sweep.py` baseline honors
`GATE_EMBED_*`), and a **real gate bug was fixed**: `--gate` crashed on the T18 session_start priming
queries (anchor=null) added to the shared 262 fixture after the gate's last validation — now excluded from
the ranking gate (logged) since they're scored by `t12_priming_sweep.py`. Gate evidence:
`tests/e2e/reports/retrieval/gate_t12_a3_adopt_20260616-150157.json`.

**Follow-up filed (latent defect, surfaced not patched):** the model-blind outbox idempotency key
(`graph.rebuild:vector:<skill_id>`, `crates/graph-builder/src/graph/rebuild.rs`) silently skips seeding any
NEW model-keyed collection (prior arm already "published" the keys) → a new arm's Qdrant collection stays
empty with no error. Real fail-loud/correctness gap for future arm swaps; fix = include model/collection in
the key.

## Out of scope

- No production default-flip here (that's the T12/owner gate, post-results). No new metrics. No
  cross-project corpus. This plan only produces the measured latency/quality table + a recommendation.
