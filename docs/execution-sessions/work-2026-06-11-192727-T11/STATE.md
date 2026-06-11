---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/11-corpus-multiview-resweep-hybrid-validation.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: "promoted from todo #259 (P2); amended 2026-06-11 instrument-first"
brainstorm_ref: none
started: 2026-06-11T19:27:27Z
status: completed
execution_shape: vertical-slices
current_unit: 6
total_units: 6
session_id: work-2026-06-11-192727-T11
---

## WHY Linkage
- Canonical WHY source: plan `## Success Criteria` + standing rule [[measurement-drives-real-app-no-in-process-reconstruction]]
- Parent plan: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- This execution serves: the EARNED verdict on whether hybrid beats dense once the corpus actually carries multi-view content, plus a measurement instrument that can SEE arm differences (α=0 negative control, paired diagnostics, candidate-recall@limit).
- Success-criteria focus: SC — "best local-first V1.7 arm reaches judge-aug MRR ≥ 0.80 and nDCG@3 ≥ 0.80 with no-match precision ≥ 0.90; if 0.80 unmet, shippable only if the measured delta over the 0.767 default is positive, documented, and useful for #218."

### TDD Contract
- Effective mode: Ralph-driven, adapted for a MEASUREMENT ticket (tdd_mode: ralph). "Red" = instrument that cannot discriminate (α=0 does NOT crater) / fixture 0-aligned; "Green" = α=0 craters ≥50% (instrument gate passes) + arms measured with paired diffs; "Post-Refactor Green" = full report reproduced + gate honest.
- Required evidence: live real-server sweep outputs (HTTP), per-query paired rank dumps, α=0 crater proof, candidate-recall numbers. NO in-process reconstruction, NO fabricated zeros.
- Exceptions: code-change scope is conditional only (the env-gated lexical-ranking Rust arm). OWNER DECISION 2026-06-11: STOP at the tie gate — if dense ≡ hybrid ties again, report paired-diff evidence and do NOT write the Rust arm (separate owner decision). T11 stays measurement-only unless that gate is not hit.

### Constitution Context
- Embedding-model changes + schema migrations are approval-sensitive (index Blockers). T11 introduces NEITHER by default (measurement-only). The conditional lexical-ranking arm would touch crates/retrieval ranking but is owner-deferred (see Exceptions).
- Standing machine rule: no stubs/fakes in production or non-unit tests; fail loud. Applies to the fixture + sweeps: no fabricated zeros, no canned Passed, real server end-to-end.
- No arbitrary caps on churners: qwen3 embed/rebuild + claude-judge drains run to completion; deadlines are stuck-detectors only.

### Architecture Handoff
- Artifact: plan-derived handoff (no separate architecture doc). Feature home: `tests/e2e` quality harness + `scripts/retrieval_quality_*` (+ conditional `crates/retrieval` lexical arm — owner-deferred).
- Key structural fact (from midpoint assessment): candidate-generation backends (BM25/Qdrant) only expand the candidate POOL; final ranking is ALWAYS eq.3 over dense cosine (`crates/retrieval/src/dual_scope.rs:353-417,:580`). So at 262 skills with candidate_limit=50, identical top-3 across arms is expected BY CONSTRUCTION — candidate-recall@limit is the ONLY signal candidate gen can move at this scale. This is WHY T11 is instrument-first.
- Seams this must honor: real mcp-server over HTTP (find_skill / search_skill_graph), real claude CLI judge, real qwen3 Ollama, CQRS (snapshot_dense read = in-memory; qdrant_hybrid = read-path break, experimental).
- Review guidance: verify no fabricated measurement; α=0 gate recorded BEFORE verdict; sign-test (not mean equality); default flip (if any) updates T08 contract doc.

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | Bring up live qwen3 stack on 262 corpus | infra-packet | real server is the measurement substrate | completed | 1 | unit-01-bring-up-live-stack.md |
| 2 | Build 262-aligned anti-circularity fixture | tracer-bullet | an instrument that can discriminate arms | completed | 2 | unit-02-anticircularity-fixture.md |
| 3 | Extend harness (α=0, candidate-recall, MRR@10, paired diffs) | expansion | the instruments scope items 1/4/5/6 require | completed | 1 | unit-03-measurement-instruments.md |
| 4 | Instrument gate: α=0 negative control | hardening | fixture validity proof (AC#1) | completed (100% crater, p=0.0000) | 2 | unit-04-instrument-gate.md |
| 5 | Full arm matrix sweep | expansion | the dense-vs-hybrid + dense-views verdict | completed | 3 | unit-05-arm-matrix.md |
| 6 | Verdict + report + contract update; tie-gate stop | hardening | the earned [[hybrid-is-the-retrieval-bet]] verdict | completed | 1 | unit-06-verdict-report.md |

## KEY VERDICT (anchor-only, all 137 positives; judge-aug pending)
- SPARSE/BM25 hybrid FALSIFIED (snapshot_hybrid HURTS: MRR 0.686→0.522, lost 23 golds). qdrant_hybrid EXACTLY ties dense (137 ties, p=1.0) — CQRS break not worth it.
- DENSE multi-view (T09 RETRIEVAL_DENSE_VIEWS) VALIDATED: MRR 0.686→0.743, cand_recall 0.723→0.796, nDCG 0.696→0.755, sign p=0.0074. THE multi-view bet pays off via dense views, not BM25.
- Candidate-recall (not ranking) is the lever: MRR@3==MRR@10 all arms (gold is top-3 or missed). 
- Tie gate hit (dense≡qdrant_hybrid) → STOP, no Rust lexical arm (owner decision).
- Floor 0.48 well-calibrated (real top-1 scores 0.58-0.93; old "0.016 compressed" was the RRF artifact).
- POSSIBLE DEFAULT FLIP: RETRIEVAL_DENSE_VIEWS default-OFF→ON (needs T08 contract-doc update; T09 flag).

## Learnings Brief
- Live stack was fully DOWN at session start (no containers, Ollama unreachable). PG is ephemeral (no volume in compose.test); test-project-skills volume was empty. The 262-skill corpus survives ONLY in `tests/e2e/reports/replica-run/skills/` (262 SKILL.md, rich multi-view). Bring-up = re-seed volumes from there → compose up → cold rebuild/embed.
- Each skill's `source_session_id` (e.g. `replica-0013-37dd1e8d`) maps to one of the 24 transcripts in `tests/e2e/reports/replica-run/genuine_manifest.txt` (~/.claude/projects/.../<uuid>.jsonl). This mapping is the backbone of the anti-circularity fixture: query from transcript problem statement → gold = skill from that transcript's resolution.
- Fixture schema (from existing 234 fixture + retrieval_quality_live.py): `{queries:[{id,kind,split,text,anchor (gold skill NAME),relevant:[names]}]}`; negatives have anchor=null, relevant=[]. find_skill returns skill NAMES. Judge = real claude CLI sonnet.
- Owner decisions this session: (1) run-to-completion autonomously; (2) STOP at tie gate — no Rust lexical arm unless separately approved.
