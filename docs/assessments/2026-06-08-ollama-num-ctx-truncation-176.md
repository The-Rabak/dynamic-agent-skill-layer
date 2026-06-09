---
date: 2026-06-08
topic: gemma malformed-JSON on substantive windows — root cause is silent Ollama context truncation (#176)
assessor: Claude (Opus 4.8), live trace
status: ROOT CAUSE PROVEN + FIXED (num_ctx always sent, aligned to the window)
method: REAL parser → REAL segmenter → REAL prose prompt POSTed to the REAL Ollama, A/B on num_ctx,
        reading the model server's own prompt_eval_count + truncated flag. No reconstruction.
related_tickets: ["176", "214", "221", "183", "190"]
related_memory:
  - extraction-caps-align-to-window-not-footguns
  - extraction-thinking-model-leak
  - no-arbitrary-limits-on-churners
  - measurement-drives-real-app-no-in-process-reconstruction
---

# #176 — gemma "malformed JSON on substantive windows" is SILENT CONTEXT TRUNCATION

The orchestrated local-extraction path produced malformed/empty JSON from substantive windows even
after `think:false` + `format:"json"` were set (the previously-suspected thinking-token leak). #214's
finding noted "on one substantive 16324-char window, gemma returned malformed JSON across all 3 retries."
This traces that to its real root cause.

## Root cause — Ollama silently truncates the prompt at a 4096-token default context

Ollama serves `gemma4:12b` with a **4096-token context window**, and our code **never sent `num_ctx`**
on any `/api/generate` request. Meanwhile the local-tier segmentation `token_budget` is **8192 tokens**
— a **2× mismatch**. The segmenter also budgets on *raw* event tokens while the prompt is built from
*sanitized* content, so the windows that overflow 4096 are precisely the ones **dense with kept prose
(assistant/user turns)** — the skill-rich ones. Tool-heavy windows shrink under sanitization and fit.

When a window's prompt exceeds 4096 tokens, Ollama **silently truncates the input** (`n_keep` retains
only the first few tokens and drops the rest — including the trailing JSON-contract instructions). The
model then emits malformed / keyless JSON → `serde` parse fails → the orchestrator retries at the SAME
size → same truncation → the window yields nothing. That is #176, and a major driver of gemma's
measured "0% procedural" in #214.

Confirmed **four independent ways** (4096 is real, not inferred):
- `ollama ps` → `gemma4:12b … CONTEXT 4096` (and `nomic-embed-text … CONTEXT 2048`).
- server log → `new prompt, n_ctx_slot = 4096, n_keep = 4`.
- server log → `stop processing: … truncated = 1` on an over-budget window.
- `/api/generate` response → `prompt_eval_count` capped at 4095.

## Live A/B proof (real model, real prompt)

Harness: `crates/session-extractor/examples/ollama_window_trace.rs` — drives the REAL
`parse_session_events` → `segment_session` → `render_sanitized_transcript_lines` →
`build_text_json_extraction_prompt`, then POSTs the exact prod request shape, A/B on `num_ctx`. Probed a
real 17 461-char window (≈4 757 actual tokens) from a real session, `num_predict=1` (truncation happens
during *input* processing, so 1 output token suffices):

| arm | num_ctx | prompt_eval_count | server `truncated` flag |
|---|---|---|---|
| **A — current prod (no num_ctx)** | 4096 (default) | **4095** | **truncated = 1** (≈662 tokens dropped) |
| **B — fix (num_ctx sent)** | 6656 | **4757** (full prompt) | **truncated = 0** |

Same prompt; the ONLY difference is `num_ctx`, and it determines whether the model sees the whole window
or a truncated fragment. **Control:** a 3 528-token window (under 4096) reports `truncated = 0` and
parses cleanly on both arms — confirming the failure is purely size-vs-context.

## Fix — always send `num_ctx`, sized to the window (the align-to-window rule, applied to the model context)

`num_ctx` is now sent on **every** Ollama `/api/generate` request across the whole orchestrated
extraction path — all four builders that previously sent only `temperature`:
- `infrastructure/extraction/ollama.rs` — single-shot + **prose map step** (the universal floor).
- `infrastructure/extraction/text_llm.rs` (via `http.rs`) — the seam transport (skeleton labeling,
  synthesis, preamble normalization).
- `infrastructure/extraction/merge_verifier.rs` — LLM equivalence (reduce step).
- `infrastructure/extraction/generality_verifier.rs` — generality gate.

Single source of truth: `EXTRACTION_OLLAMA_NUM_CTX = 16_384` (in `http.rs`), resolved by
`extraction_ollama_num_ctx()` (env `OLLAMA_NUM_CTX`, fail-loud on a non-integer). 16 384 is sized so the
largest local-tier window fits with headroom:

> **Alignment invariant:** `LOCAL_TIER_TOKEN_BUDGET (8 192, window content)` + mined preamble + prompt
> scaffold ≤ `EXTRACTION_OLLAMA_NUM_CTX (16 384)`, leaving ~2× headroom for the JSON output. Documented
> on both constants. Frontier windows (40 960) only route to claude/claude-code, which don't use
> `num_ctx`, so they never reach an Ollama context.

This is the same lesson as #214's char-cap fix, applied to the model context: **every size lever (window
budget, parser char caps, and now the Ollama context) must be mutually aligned so content that fits a
chunk is never silently truncated.**

Unit tests green (78 extraction tests), including a new assertion that `options.num_ctx` is on the wire.

## Operational note (correctness vs speed on a VRAM-limited host)
Raising `num_ctx` to 16 384 enlarges gemma's KV cache, so on a box where the 9.4 GB model is already
CPU/GPU-split (here `gemma4:12b … 60%/40% CPU/GPU`) it gets *slower* per token. That is the right
trade: a truncated prompt is garbage, and the no-arbitrary-limits rule says size to correctness and let
the background churner run, not cap to make it fast. Throughput is a hardware/provisioning concern
(give Ollama enough VRAM to hold the model + a 16 k context on GPU), not a reason to under-size the
context.

## What this does NOT claim — and what the clean re-measurement showed
This removes the *silent-truncation* failure mode; it does not make gemma a frontier extractor. With the
fix, the local-vs-cloud A/B was re-run on the real worker (#214): gemma's non-empty-procedure rate went
**0.00 → 0.256** (20 genuine multi-step procedural skills over 78 drafts / 9 transcripts), vs frontier
`claude-code` **0.68**. So the truncation was masking real capability — local DOES extract procedures —
but a genuine density gap (~2.6×) remains as a model-capability matter, separate from this infra bug.
The 0.00 had wrongly looked like "local can't do procedures"; it was truncated gemma.
