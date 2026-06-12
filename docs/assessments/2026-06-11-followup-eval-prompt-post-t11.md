# Follow-up Evaluation Prompt — post-T11 (2026-06-11)

Paste the prompt below into a fresh deep-assessment session (the "grok-style" adversarial evaluator
that produced `2026-06-11-v1-7-midpoint-deep-grok-assessment.md`). It carries the T11 findings and the
T12 considerations as context so the evaluator can grade against the finish line that assessment set —
specifically its §131 "What the follow-up assessment should focus on" list, whose **keystone question
(#1: did T11 build a ruler that can see?) is now answerable**.

---

## PROMPT

You are conducting the follow-up V1.7 deep assessment. The prior assessment is
`docs/assessments/2026-06-11-v1-7-midpoint-deep-grok-assessment.md` (trust-basis ~87%, endgame-basis
~66%, efficacy 1.5/10, **measurement-integrity 6.0 as the binding constraint**). Its §131 defined six
focus areas for you; **#1 is the keystone: "Did T11 build a ruler that can see?"** Grade measurement
integrity primarily on it.

Do NOT take this prompt's summary as ground truth — **verify every claim against the repo** before
grading. Drive or read the real artifacts; reward observed evidence, never "written."

### Read first (primary evidence)
- `tests/e2e/reports/t11/T11-VALIDATION-REPORT.md` — the T11 verdict + all numbers.
- `tests/e2e/reports/t11/sweep_gate.json`, `sweep_matrix.json`, `sweep_judged.json` — raw per-arm
  metrics + per-query first-relevant-rank vectors (recompute the paired directions yourself; note the
  `n_a_better`/`n_b_better` labeling caveat documented in the report §6).
- `tests/fixtures/retrieval_quality_262_corpus_labeled.json` + `scripts/build_t11_fixture.py` +
  `tests/e2e/reports/t11/fixture_build_summary.json` — judge the anti-circularity discipline yourself.
- `scripts/retrieval_metrics.py` (run `--self-test`), `scripts/retrieval_sweep.py` (the /health-gated orchestrator; renamed from `t11_metrics.py`/`t11_sweep.py` by T20).
- `docs/reference/retrieval-contract.md` (§0 V1.7 delta) + `docs/assessments/2026-06-11-v1-7-retrieval-contract-measured.md` (§8 added).
- `docs/tickets/.../11-*.md` (ACs all checked) and `docs/tickets/.../12-*.md` (## Rethink section).
- Git: `git log --oneline` for `4bdd76b` (T11 close) and `7fe8912` (dense-views default flip).

### T11 findings highlights (verify, then weigh)
1. **The "0.767 five times" mystery is RESOLVED — reading #1 was right.** The old fixture couldn't
   discriminate. On a corpus-aligned, anti-circular fixture (137 positives, gold mapped via skill
   `source_session_id` to the 24 transcripts' problem statements; `use_when` demoted to a secondary
   stratum), the arms **separate cleanly**.
2. **α=0 negative-control gate PASSED at 100% MRR crater (p=0.0000)** before any arm verdict — the
   keystone discrimination proof §131#1 demanded. Confirm this ran *first* and gated the verdict.
3. **Earned hybrid verdict (anchor-only, 137 pos):** snapshot_dense MRR@3 0.686 / cand-recall@50 0.723;
   **snapshot_hybrid (BM25) net-NEGATIVE** 0.522 / 0.555 (lost 23 golds from the pool, 0 improved,
   p=0.0000); **qdrant_hybrid EXACTLY ties dense** (137/137, p=1.0); **dense_views_on (T09) WINS**
   0.743 / 0.796 (sign p=0.0074, recovered 9 golds into the pool).
4. **Frozen 0.80 aspiration MET** (judge-aug held-out): dense 0.884/0.804/0.92, dense_views 0.912/0.839/0.92.
   Previously un-validatable (234 fixture 0/30 aligned). This is the first real retrieval-quality number
   on the dogfood corpus.
5. **Candidate-recall, not ranking, is the lever:** MRR@3 == MRR@10 for every arm (gold is top-3 or
   missed). The sparse "hybrid bet" is falsified; the multi-view bet pays off through **dense** views.
6. **`RETRIEVAL_DENSE_VIEWS` flipped to default-ON** (commit 7fe8912, T11-validated, p95 369ms<500ms,
   retrieval/mcp-server unit tests pass, live-proven). The earlier "hybrid is the retrieval bet"
   decision is now split: sparse falsified, dense multi-view validated.
7. **Floor 0.48 re-verified** on qwen3 (top-1 scores 0.58–0.93, all above floor); the old "compressed
   ~0.016 scores" alarm was the RRF `fusion_rank_score` artifact, not the #260 eq.3 score.

### Deviations from the prior assessment's prescriptions (grade honestly)
- **The conditional lexical-ranking arm (δ·BM25 in eq.3) was NOT built.** §109 wanted the hybrid bet
  "tested as a *ranking* signal at least once." The owner chose to **stop at the tie gate** (dense ≡
  qdrant_hybrid) and defer the Rust arm, on the prior that BM25-as-candidate already hurt. Decide
  whether this is a justified pre-registered stop or an unproven gap, and grade accordingly.
- T11 measured on `all` (137) and reported held-out subsets, rather than tuning-only winner selection
  (no parameter tuning was performed, so no held-out leakage). Confirm that reasoning holds.

### T12 considerations (the next ticket on the critical path — assess the reframing)
T12's `## Rethink (post-T11)` section reframes it. Evaluate whether the reframing is correct:
- Task-retrieval re-ranking signals (centrality/recent-use) are argued **inert** because ranking is
  saturated and candidate-recall is the lever — so they're dropped unless they raise candidate-recall.
- Priming must use a **priming-appropriate metric** (set-coverage + freshness), NOT MRR.
- The T11 fixture has **no session-start stratum**; T12 must author it.
- No new candidate sources (BM25 hurt); freshness = bounded injection over the dense pool.
- Is this the right scope, or is there a task-retrieval signal worth keeping? Is the priming metric
  well-posed and falsifiable? Does it preserve the attribution prize (§69) that decides T12's fate?

### Deliver
1. **Measurement-integrity score** — move it off 6.0 only if the α=0 gate actually cratered and
   paired/sign-test verdicts replaced mean-equality (verify in the JSON). State the new number and why.
2. **A corrected reading of "0.767 five times"** now that the discriminating fixture exists.
3. **Verdict on the default flip** — is promoting dense-views to default-ON warranted on this evidence,
   or is the uplift within noise at N≈137? (Judge the judge-aug paired p=0.09 vs anchor p=0.0074 vs the
   candidate-recall delta.)
4. **T12 go/no-go + scope** — endorse, amend, or reject the reframing; if task-retrieval signals are
   dead, say so and redirect the effort.
5. **Re-grade the remaining §131 items** you can verify (dream contracts live pass/fail, CL-bench
   framing in T14/T15, pre-registration discipline, workspace gates green). Flag what you could NOT
   verify rather than assuming.
6. **Updated critical-path recommendation** — does T11's result change the T12→T13→T14→T15 sequence or
   the fallback levers (extraction density / injection UX / intent split)?

Hold the project's own honesty bar: no number cited without its source artifact, no "written" counted
as "passing," and any place T11's evidence is thinner than this prompt implies must be called out.
