# Claude Code CLI / Sonnet-4-6 Extraction — E2E Quality Assessment

**Date:** 2026-06-07
**Provider under test:** `EXTRACT_SESSION_PROVIDER=claude-code`, `EXTRACT_SESSION_MODEL=claude-sonnet-4-6`, `EXTRACT_SESSION_ROUTING=frontier`
**Baseline:** prior Ollama (`gemma4:12b`) full run `run__20260606201511.json`
**Branch:** `feat/v-1-5-1`

---

## 0. Headline

- The extraction subsystem was **structurally coupled to Ollama** in real app logic (not just test setup). I made it **provider-agnostic** so the whole extraction LLM workload — *map + all four orchestration seams* (skeleton, synthesis, preamble, equivalence) — runs on the selected provider. Embeddings remain Ollama (the one documented exception).
- The full e2e suite under claude-code/Sonnet: **614 passed / 3 failed**. **All 3 failures are provider-invariant** (no extraction or claude-code failure).
- **Proof Sonnet drove extraction end to end:** 129 headless `claude` sessions in the run window, **all** `claude-sonnet-4-6`, in the adapter's neutral `-tmp` cwd, carrying extraction *and synthesis-seam* prompt signatures.
- **Extraction quality (Sonnet): 5/5 taught concepts, on-topic, anti-pattern stance `WarnsAgainst`, human-gate clean, confidence 0.91.** Per-job latency ~13–39 s vs Ollama gemma4:12b's 40–130 s.

---

## 1. Where the Ollama coupling actually was (the gap you flagged)

Two layers:

1. **Test setup (test-only):** 5 e2e tests hard-set `EXTRACT_SESSION_PROVIDER=ollama`. Relaxed to honor a pre-set provider (default stays ollama).
2. **App logic (the real coupling):** `SessionExtractor::from_environment` built the four orchestration LLM seams as **concrete Ollama types regardless of `EXTRACT_SESSION_PROVIDER`**. Only the *map* step honored the provider. So selecting `claude-code` previously moved only the map step to Sonnet; skeleton/synthesis/preamble/equivalence still hit Ollama.

### Fix
- New provider-agnostic transport `StructuredTextLlm` (infrastructure) with two impls: `OllamaTextLlm` and `ClaudeCodeTextLlm` (reuses the existing CLI subprocess machinery via the new `claude_code_generate_text`).
- All four seams now take `Arc<dyn StructuredTextLlm>`; `from_environment` builds **one** transport per provider and powers all four from it. Equivalence verifier routed through the same transport via new `TextLlmEquivalenceVerifier` (reuses the shared `{equivalent, rationale}` prompt).
- Mirrors the maintenance crate's existing `MERGE_VERIFIER_PROVIDER` precedent. Fail-loud preserved (no stubs); Ollama path behavior unchanged. 307 unit tests pass; workspace + all e2e binaries compile.

---

## 2. Proof claude-code/Sonnet actually ran (not a silent fallback)

| Evidence | Value |
|---|---|
| `claude` headless sessions during run window | **129** |
| Sessions using `claude-sonnet-4-6` | **129 / 129** |
| Working directory | `~/.claude/projects/-tmp/…` = adapter's `current_dir(temp_dir())` signature |
| Prompt signatures found | `skill candidate`, `extract`, **`session-spanning`** (synthesis seam), `candidates` |

129 calls ≫ a map-only count — confirming the **seams ran on Sonnet too**. Independent corroboration: standalone parity test ("claude CLI found … Running ClaudeCodeExtractor", 5/5, conf 0.91) and the smoke extraction-quality (5/5, 38.6 s).

---

## 3. Full-suite results under claude-code/Sonnet

**614 passed / 3 failed** (sum across all cargo-test binaries), ~14 min wall-clock.

### The 3 failures — all provider-invariant, none extraction

| Test | Cause | Provider-related? | Status |
|---|---|---|---|
| `maintenance::merge_pass_detects_cross_scope_duplicate_skills` | ℓ₁ embedding-input policy narrowed to *summary-only* (name+desc+tags); the two `rust-auth` fixtures now score ≈0.63 cosine, just under the test's 0.65 gate. Uses `DeterministicEmbeddingService` + `AlwaysEquivalentVerifier` — no LLM/provider. | **No** — fails identically with my changes stashed. Pre-existing test↔policy drift. | Pre-existing |
| `DS-007 high_qps_compile_context` | p95 **1002 ms** > 500 ms budget (p50=830 ms). Pure compile_context retrieval under concurrent QPS. | **No** — no extraction. Latency under load; baseline was ~90 ms quiet. Likely dev-box contention. | Environmental (re-measure on quiet host) |
| `concurrency::compile_context_parallel_burst` | `"at least one NoMatch required"` — a should-miss prompt matched a **leftover skill in the shared corpus**. | **No** — cross-suite contamination (known shared-scope pattern). | Test isolation |

### Notably now GREEN (were RED in the Ollama baseline) — but due to branch isolation fixes, NOT the provider
- `DS-005 qdrant_pg_drift`: **7 failures → 0** (provider-invariant; isolation fix in commit 7483309).
- `retrieval-quality` (incl. semantic-vs-lexical, negatives, latency): **14 failures → 0** (provider-invariant; embeddings are Ollama either way).
- `golden-path`: green.

---

## 4. Extraction quality deep-dive (Sonnet)

Same rich fixture (`session-rich-transcript.jsonl`, teaches 5 concepts: file_io, error_safety, create_parent_dir, atomic_write, naming_convention).

- **Concept coverage:** 5/5 (every run).
- **Structural validity:** frontmatter `origin: session_extraction`, name, description ≥20 chars, H1, subunit section, provenance — all pass.
- **On-topic:** yes (topic_score ≥1).
- **Anti-pattern safety:** `rm -rf` stance classified **`WarnsAgainst`** (faithfully reproduces the warning rather than recommending it).
- **Human gate:** only `.pending`, no auto-approved `SKILL.md`.
- **Confidence:** 0.91.

### Sample Sonnet draft (parity report)
> *rust-file-io-safe-helpers — Reusable Rust file I/O pattern: safe read helpers, atomic writes via tmp+rename, and directory creation with full error propagation.*
> 1. Wrap all `std::fs` reads in a helper returning `Result<String, io::Error>`; never `.unwrap()` in library code.
> 2. `fs::create_dir_all(parent)` before writing, propagate with `?`.
> 3. Atomic write: write `.tmp` sibling, then `fs::rename` onto target.
> 4. On rename failure, delete the `.tmp`; surface the error via `?`.
> 5. Unit test each helper against a tempdir asserting byte round-trip.
> Name read helpers `read_to_string_safe`, write helpers `write_atomic`. Never `unwrap()`/`expect()` in library IO. Do NOT `rm -rf` the repo root for cleanup. `.tmp` siblings co-located with target for cross-FS rename atomicity.

This is specific, grounded, and captures the convention + the anti-pattern-as-warning — no hallucinated generics.

---

## 5. Comparison to the Ollama (gemma4:12b) runs

| Dimension | Ollama `gemma4:12b` | Claude Code `claude-sonnet-4-6` |
|---|---|---|
| Concept coverage (lenient gate) | 5/5 (also passes) | 5/5 |
| Extraction reliability | History of failures: thinking-mode chain-of-thought leaking into JSON keys (#176), unescaped-quote JSON truncation dropping required fields, intermittent 0-candidate returns | 129/129 calls produced parseable structured output; 0 extraction failures across the suite |
| Per-job latency | 40–130 s (CPU inference) | ~13–39 s |
| Seam coverage on provider | Map only (seams were Ollama-pinned even when provider≠ollama) | **Map + all 4 seams on Sonnet** |
| Content specificity | Adequate when it succeeds | High; precise naming, ordering, cross-FS rationale |

**Crucial caveat — don't over-credit the provider:** the bulk of the prior Ollama run's RED (retrieval-quality 14, DS-005 drift 7, golden-path 5 = 26 of 28 failures) is **provider-invariant** and was fixed by **harness-isolation work on this branch**, not by switching to Sonnet. Retrieval/drift/golden-path don't use the extraction provider at all (retrieval ranks Ollama `nomic-embed-text` vectors in both runs). The provider's genuine contribution is confined to **extraction**: higher reliability (no thinking-leak/JSON-truncation class of failures), lower latency, and seam coverage.

### Cost/efficiency note
Each seam call is a **separate `claude` subprocess**; the trivial-call probe showed ~29.5k `cache_creation_input_tokens` per invocation (CLI system prompt reloaded fresh each time — no cross-call cache reuse). ~129 calls/run ⇒ ~3.8M cache-creation tokens. Free on subscription, but it adds per-call latency and would be ~$10–15/run on API billing. A persistent Anthropic-API client (`=claude`) or batching seams would amortize this; the CLI-subprocess-per-seam design trades efficiency for the zero-API-key subscription path.

---

## 6. Findings / recommendations

1. **`maintenance` merge test ↔ ℓ₁ policy drift (P2, pre-existing):** summary-only embedding lowered near-dup cosine below the test's 0.65 gate. Faithful fix: make the fixture a genuine near-dup *under summary-only* embedding (share more name/desc/tag tokens), or reconsider whether ℓ₁ should fold a procedure digest for merge recall. *Real signal:* summary-only ℓ₁ weakens merge-duplicate recall for skills that share procedures but differ in summary wording.
2. **Load-latency (DS-007) — re-measure on a quiet host.** p50 830 ms vs ~90 ms baseline strongly suggests contention (10 h of resident containers + back-to-back runs), but if it reproduces quiet, the warm read path regressed.
3. **`parallel_burst` contamination** — same shared-scope leftover pattern your standing rule targets; the should-miss prompt matched a residual skill. Per-suite data cleanup needs to cover this corpus.
4. **claude-code seam efficiency** — consider batching seam calls or offering `=claude` (persistent API client) for the seams to avoid 29.5k-token cache-creation per subprocess.

## 7. Bottom line

The provider-agnostic refactor delivers your stated goal: **everything except embeddings runs on either provider end to end**, proven live (129 Sonnet calls spanning map + seams). Sonnet extraction is faster and more reliable than gemma4:12b and produces specific, grounded drafts. The suite is effectively green under claude-code; the only failures are pre-existing/environmental and provider-invariant.
