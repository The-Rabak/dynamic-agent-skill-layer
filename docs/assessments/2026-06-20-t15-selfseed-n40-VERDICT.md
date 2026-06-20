# T15 self-seeding compounding loop — POWERED N=40 VERDICT (2026-06-20)

Branch `feat/v-1-7`. Pre-registration: `docs/assessments/2026-06-20-t15-selfseed-precommit.md` (+ N=40
addendum, committed before any N=40 solve). N=10 precursor: `…-t15-selfseed-result.md`.
Report: `tests/e2e/reports/swebench/selfseed_selfseed-django-n40.json`.

## VERDICT: **COMPOUNDS on EFFICIENCY (significant), NULL on resolved-rate**

The skill layer does **not** make Sonnet solve *more* SWE-bench instances — but it makes it solve the
**same** instances measurably **cheaper**. This is the project's **first statistically-significant,
pre-registered, no-fakes efficacy result**, and it lands exactly where the de-risk probe predicted the
signal would live: *exploration cost, not pass/fail*.

## The numbers (40 django instances, each solved OFF then TREAT, real swebench oracle)

### Resolved-rate — NULL
| | value |
|---|---|
| X (OFF, Round 1) | 24/40 = **0.600** |
| Y (TREAT, Round 2, self-seeded) | 25/40 = **0.625** |
| Δ (Y−X) | **+0.025** (net +1 instance) |
| McNemar | gained **2** / lost **1**, p = **1.000** |

2 flips (`12700` via its own self-mined skill; `16408` via a cross-instance skill) barely outweigh 1
regression (`13551`, where injection distracted a would-be pass). **Not significant.** Of the 16 OFF-
failures, only **2 flipped** — and 14 held even when the *exact* relevant skill was injected (e.g. `15202`
got `catch-urlsplit-valueerror-in-validators`, `14155` got `unwrap-functools-partial`, `11630` got the E028
fix — none completed to green in 40 turns). **The gap is execution, not knowledge.**

### Efficiency (turns/tokens/cost/time to resolve) — SIGNIFICANT, on the 23 instances BOTH arms solved
| metric | mean Δ (TREAT−OFF) | cheaper | sign p | bootstrap 95% CI | verdict |
|---|---|---|---|---|---|
| **output_tokens** | **−3584** | 17/23 | **0.035** | **[−7177, −593]** | **CI excludes 0 ✓** |
| **duration_ms** | **−55670** (−55.7s) | 16/23 | 0.093 | **[−106570, −12050]** | **CI excludes 0 ✓** |
| num_turns | −1.4 | 11/23 | 0.332 | [−5.1, +2.5] | ns (spans 0) |
| total_cost_usd | −$0.077 | 12/23 | 1.000 | [−0.20, +0.04] | ns (spans 0) |

**All four deltas are negative (consistent direction); two reach a bootstrap CI excluding zero.** The
self-seeded layer cuts the model's **generated work by ~3,600 output tokens** and **wall-clock by ~56s**
per instance, on instances it was going to solve anyway. That is the compounding mechanism made measurable:
the layer hands the model the repo-procedural knowledge it would otherwise **re-derive every session**, so
it stops re-exploring and goes straight to the fix.

## Against the pre-registered rule

> **COMPOUNDS** — Y > X with McNemar (sign p < 0.10), **OR an efficiency mean-delta < 0 with bootstrap CI
> excluding zero on ≥1 metric.**

Two efficiency metrics (output_tokens, duration_ms) satisfy the CI-excludes-zero clause → **COMPOUNDS**
by the locked rule. Resolved-rate is independently **NULL** (reported, not hidden).

## Why this is the honest, expected shape

The original T15 framing called it: *"a strong pretrained model already knows most things you'd put in a
skill, so pass/fail ties; the benefit most likely shows as saved exploration (turns/tokens), not
fails→passes."* At N=10 the efficiency signal was noise (n=6, all CIs spanned zero); the **powered N=40
(n=23 resolved-by-both) resolved it into significance.** This is precisely what a powered run is for.

## Honest caveats (stated, not buried)

1. **Multiple comparisons.** The "≥1 of 4 metrics" rule is a disjunction → inflates false-positive risk.
   Mitigants: all 4 deltas point the same way; the 2 significant ones (tokens, duration) are correlated
   measures of "work done," not independent lucky draws. Still, a strict Bonferroni (α/4 = 0.0125) would
   leave output_tokens' sign-test p=0.035 short — the **bootstrap CI** (the pre-registered gate) is what
   carries it, and it clears comfortably ([−7177, −593]).
2. **num_turns and cost are ns.** Turns is coarse (capped at 41, so big wins get clipped); cost is muddied
   by prompt-cache dynamics. Output-tokens (raw generation work) is the cleanest lens and the clearest win.
3. **Resolved-rate is genuinely flat.** Do not claim the layer raises pass-rate — at this scale, with
   Sonnet, it does not. Its value is *efficiency*, not *capability extension*.
4. **n=23** for efficiency (resolved-by-both) is solid but not large; the CI is wide.

## Mechanism evidence (attribution)

Injection fired on **40/40** instances (rich, on-topic retrieval from the 56-skill self-seeded corpus).
Both compounding modes were observed: **self-reapplication** (`12700` flipped on its own
`recursive-sanitizer-must-handle-all-container-types` skill, mined from its own failed OFF session) and
**within-bench transfer** (`16408` flipped on another instance's ORM skill). The efficiency win is broad-
based (17/23 cheaper on tokens), not driven by the 2 flips.

## Provenance & integrity

- **Clean isolated layer**, started EMPTY (`swebench_t15` dropped+recreated → 0 skills; Qdrant collection
  cleared; all prior scopes archived). Round-1 OFF ran with no layer; Round-2 TREAT injected only the 56
  skills self-mined from this bench's own Round-1 transcripts (scope `swebench-django-n40`).
- **Dogfood `skill_layer_test` byte-identical** before/after — sha256 `1eb8d9fb…`. Production untouched.
- Every `resolved` bit from the official SWE-bench F2P/P2P oracle. No fakes: max-turns/empty-patch solves
  banked as real not-resolved; the only retries were genuine auth failures (none banked as data).
- **N is deterministic + pre-committed** (`test_block_ordered[:40]`, disjoint from the N=10 set).

## Cost

Solve cost: Round 1 **$30.30** + Round 2 **$29.17** = **$59.47** (40 hard django instances × 2 rounds;
many ran the full 40-turn budget at ~$1+). Plus frontier extraction of 40 transcripts (~$8–10). The
`test_block` instances proved harder/pricier than the N=10 `seed_block`, putting the run modestly above the
$40–60 estimate — flagged to the owner during Round 1.

## Robustness notes (run-quality issues hit + fixed; none affected the result)

Four transient infra failures surfaced and were fixed at zero result-cost, each hardened into the tooling:
(1) HF 429 on per-instance problem fetch → one-shot bulk dataset cache; (2,3) auth-guard had to learn that
`is_error=True` marks BOTH a real auth-401 *and* a legitimate max-turns cap-hit — final guard retries only
true crashes / explicit auth failures, banks max-turns-empty-patch as a real not-resolved; (4) draining all
40 transcripts at once (frontier extraction = many claude subprocesses) overloaded WSL2 and the drain
**failed loud** rather than gating a partial corpus — recovered by `t15_complete_seed.py` (drain only the
re-queued rows, no duplicates). Per-instance checkpointing made every interruption a clean resume.

## Bottom line

**The skill layer compounds — by eliminating the re-exploration tax, not by extending capability.** On a
clean SWE-bench django bench, learning from its own first pass let the model re-solve the same work with
**significantly fewer output tokens and less wall-clock time** (bootstrap CIs exclude zero), while pass-rate
held flat. That is a real, measured, pre-registered win for the compounding thesis — appropriately scoped:
the layer makes a capable agent *faster and cheaper on recurring work*, which is exactly what repo-specific
procedural memory should do.
