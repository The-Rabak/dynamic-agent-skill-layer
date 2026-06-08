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

## Results

_(pending the run)_
