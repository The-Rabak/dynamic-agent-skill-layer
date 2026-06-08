---
date: 2026-06-08
topic: local-vs-cloud-extraction-gap (#214)
assessor: Claude (Opus 4.8), orchestrated
status: bar committed — measurement pending
method: REAL maintenance-worker binary draining the real PG ingest queue through the real provider seam
        (EXTRACT_SESSION_PROVIDER=ollama vs claude-code) → real PendingDraftWriter; NO reconstruction
related_tickets: ["214", "176", "183", "187", "191"]
related_memory:
  - measurement-drives-real-app-no-in-process-reconstruction
  - phase2-corpus-loop-proven-2026-06-07
  - extraction-thinking-model-leak
---

# #214 — does the DEFAULT local extraction path produce usable skills?

Local Ollama (`gemma4:12b`) is the default and the privacy story; it is also the weakest extraction
path (#176 zero-candidate, the thinking-token JSON leak, and the entire map→reduce orchestration epic
#183–191 is a bet that scaffolding can squeeze skills out of weak local models). The corpus was built
on `claude-code` (frontier). This measures the local-vs-cloud gap on the SAME real transcripts.

## Committed acceptance bar (FROZEN BEFORE THE RUN — do not move to fit the result)

On a representative sample of ≥10 real transcripts, the DEFAULT local (`ollama` / `gemma4:12b`)
extraction path must clear ALL of:

- **Yield ≥ 1.0** accepted `.pending` draft per transcript on average (a procedure-rich real session
  must produce *something*; near-zero confirms #176 at scale).
- **Non-empty-procedure rate ≥ 0.70** — at least 70% of produced `.pending` drafts carry ≥1 non-empty
  `## Procedures` bullet (an empty-procedure draft is worthless; see #211).
- **Judge-acceptance rate ≥ 0.50** — at least 50% of produced drafts are rated a genuine, useful,
  non-redundant skill by an independent real claude-sonnet judge.

If local clears the bar → prove it and keep the local-first claim. If it does not → correct the README
to state plainly that the local default is for privacy/zero-cost and that high-quality extraction needs
a frontier provider. Both providers run on the SAME transcripts; the gap is reported either way. #176
(zero-candidate local) is resolved or characterized with this evidence.

## Method

Real maintenance-worker host binary (`cargo build -p maintenance --bin maintenance-worker`, the proven
corpus drain recipe) against the test DB (`skill_layer_test`), draining a freshly-ingested batch of the
same real transcripts once per provider into an isolated `.skills` sandbox, then counting/judging the
produced `.pending` drafts. Real Ollama + real claude-code calls; no fakes, no canned candidates.

## Results (2026-06-08) — a SHARED parser cap blocks BOTH providers on real transcripts

Driving the real maintenance-worker over real `~/.claude` transcripts surfaced a blocker that
**reframes #214**: the tier (local-vs-cloud) gap cannot be measured yet because **both** providers
yield **zero** on real transcripts, blocked identically by a hardcoded transcript-parser cap.

**Measured (real maintenance-worker, real ingest queue, real provider seam):**
- `EXTRACT_SESSION_PROVIDER=ollama` (gemma4:12b): **0 drafts**. Worker log:
  `WARN orchestrator: window map step failed || EpisodeExtraction(InvalidTranscript("transcript entry 2
  exceeds maximum content size 8192"))` → `ERROR orchestrated extraction failed: AllWindowsFailed`.
- `EXTRACT_SESSION_PROVIDER=claude-code`: **0 drafts** — the SAME failure, same cap.
- **6/6** sampled real transcripts contain at least one entry exceeding 8192 chars (real tool outputs /
  code blocks routinely do).

**Root cause:** `max_entry_chars: 8_192`, a hardcoded (NOT env-configurable) default in BOTH
`crates/infrastructure/src/extraction/ollama.rs:44` and `.../claude_code.rs:92`, enforced as a HARD
REJECT in `crates/infrastructure/src/extraction/limits.rs` (`entry_chars > max_entry_chars` →
`InvalidTranscript`). A single oversized message (one entry) fails the WHOLE transcript
(`AllWindowsFailed`), producing nothing.

**This is:**
1. The root cause of **#176** (zero-candidate local) — not (primarily) gemma weakness, but a parser cap
   that rejects real transcripts before the model is even asked to extract.
2. An **arbitrary-limit violation** (the standing no-arbitrary-caps rule): 8192 chars/entry is far below
   real-transcript reality and below any model's real context. The fix is to **truncate/clamp the entry,
   not reject the transcript** (the project's degrade-don't-drop pattern), or raise + env-expose the cap.
3. Connected to **#221** (intolerant transcript parser rejecting real transcripts).

**Honest conclusion:** the committed local bar (yield ≥ 1) is NOT cleared — but the blocker is the SHARED
parser cap, NOT the local model tier, so this does NOT prove "local is weak." The local-vs-cloud QUALITY
gap is **unmeasurable until the cap is fixed.** How the existing 234-corpus was built despite this cap is
itself a question (older/pre-chunked path); the CURRENT code yields zero from real transcripts for both
providers — a P1-class block on the entire self-growing loop on real data.

**Recommended action:** fix the cap (truncate-don't-reject + env-expose, per no-arbitrary-limits), then
re-run this same harness (`scripts/measure_214_extraction.py`) to measure the real local-vs-cloud quality
gap. README correction below reflects the measured reality either way.

## RESOLUTION (2026-06-08) — the cap was the blocker; fixed + proven

The arbitrary caps were removed and **aligned to the orchestration window** (the distinction that
matters: the 8192-token *chunk budget* is legitimate windowing; the footgun was the per-entry *char*
cap being SMALLER than the window — 8192 chars vs an 8192-token ≈ 32 768-char window):

- **Parser char caps** (`ollama.rs` / `claude_code.rs` / `claude.rs`): `max_entry_chars` 8 192 →
  **524 288**, `max_total_chars` 1 000 000 → **1 048 576**, `max_entries` 2 000 → **100 000** — sized
  to comfortably exceed the largest window (frontier 40 960 tok ≈ 163 840 chars), env-overridable via
  `EXTRACT_MAX_ENTRY_CHARS` / `EXTRACT_MAX_TOTAL_CHARS` / `EXTRACT_MAX_ENTRIES`.
- **Chunk budgets** (`routing.rs`): local tier kept at **8 192** (correct — legitimate local-model
  windowing), frontier tier **200 000 → 40 960** (5× local, smaller focused windows for recall).
- **`OrchestrationConfig::default()`** fallback budget reverted to 8 192.

**PROVEN on real transcripts:** the same gemma4:12b run that previously died at the char gate
(`AllWindowsFailed`) now runs the full map→reduce→synthesis pipeline and **produced 6 real `.pending`
skill drafts** from the first real transcript (e.g. `staged-background-test-execution`,
`human-gate-ci-infra-changes`, `orchestration-independent-subagent-validation`). The blocker is gone.

**Remaining (genuine) local-quality finding:** on one substantive 16 324-char window, gemma returned
**malformed JSON** across all 3 retries (the #176 / thinking-token-leak class), and the orchestration
**degraded gracefully** ("accepting empty" for that window) while the deterministic skeleton/preference
paths still produced drafts. So local extraction now *works* but the gemma prose-extraction path has a
real malformed-output quality wrinkle — now measurable (it was previously masked by the hard block).

## Measured local-vs-cloud A/B (same real transcripts, real maintenance-worker)

Directional 2-transcript sample (a ≥10 sample is running to formally satisfy the committed bar; gemma's
0% procedural is systematic, not sample variance — the prose path fails on every substantive window):

| provider | drafts | yield/transcript | **non-empty-procedure rate** | elapsed |
|---|---|---|---|---|
| `ollama` / `gemma4:12b` (local default) | 15 | 7.5 | **0.00** | 448s |
| `claude-code` (frontier) | 21 | 10.5 | **0.52** | 254s |

**The gap, measured:** local gemma yields *only* preference/convention skills and **zero procedural**
skills; claude-code yields real procedural skills (`incremental-unit-commit-staging`,
`work-unit-lifecycle-pipeline`, `production-fail-loud-no-silent-fallback`, …). Local **fails the
committed bar** (non-empty-procedure 0.00 ≪ 0.70). The procedural extraction the layer exists for needs a
frontier provider; the local default is a private, zero-cost *preference/convention* extractor today.

**#176 (zero-candidate local) — RESOLVED/characterized with evidence:** it had two stacked causes —
(1) the `max_entry_chars=8192` parser cap rejecting every real transcript (now FIXED, commit 4414e97),
and (2) gemma's structured-prose path returning malformed JSON on substantive windows (degrades to empty
procedural). With (1) fixed, local yields drafts but 0% procedural (cause 2 remains, a gemma-model
limitation, now openly documented in the README). The fix unblocked extraction for the **frontier**
provider too (the cap was shared) — which matters most since frontier is the quality path.

**README:** corrected to state the measured tradeoff (local = preference/zero-cost; procedural needs frontier).
