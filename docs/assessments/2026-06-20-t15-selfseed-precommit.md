# T15 self-seeding same-set loop — pre-commitment (2026-06-20)

Branch `feat/v-1-7`. Written **before Round 2 (TREAT) reveals Y** on the real bench, so the verdict
cannot be reverse-engineered from the data (the discipline that kept T18/T14/T23 honest). Supersedes the
cross-instance DiD framing of `todos/283` for *this* experiment (owner redirection 2026-06-20).

## The question (owner design)

Does the layer compound on its **own recurring work**? Not "do skills from instance A transfer to a
*different* instance B" (the cross-instance transfer test — the de-risk probe answered that pessimistically,
`docs/assessments/2026-06-20-t15-derisk-probe.md`). Instead: **run a bench, learn from that run, re-run the
same bench — does the score improve?**

> "running 10 tests, getting score X and then extracting from those sessions and running the same tests
> again should yield score Y which is higher than X." — owner, 2026-06-20

## Design (locked)

1. **Bench (N=10 django Lite instances), fixed:**
   `django__django-{14999, 16046, 13447, 15789, 11099, 10924, 11564, 13321, 11133, 13660}`.
2. **Round 1 — OFF (layer empty):** solve all 10 with no injection → resolved bits = **X**; capture each
   full claude session transcript.
3. **Seed:** extract skills from the 10 Round-1 transcripts (real host frontier worker, `claude-code`
   provider, isolated `swebench_t15` DB) into a **fresh, initially-empty scope** `swebench-django-selfseed`
   (gate = real `.pending`→`SKILL.md` rename; reconcile + snapshot rebuild).
4. **Round 2 — TREAT (self-seeded layer):** solve the **same 10** with `trigger=session_start` injection
   from that scope → resolved bits = **Y**.
5. Round-1 OFF and Round-2 TREAT are **independent fresh solves** differing only in the injection.

## Metrics (locked)

- **Primary — resolved-rate:** X = mean(Round-1 OFF resolved), Y = mean(Round-2 TREAT resolved), both by
  the swebench `FAIL_TO_PASS`/`PASS_TO_PASS` oracle. Paired **McNemar** (gained = OFF-fail→TREAT-pass;
  lost = OFF-pass→TREAT-fail) with the exact sign-test p. Report X, Y, Δ=Y−X and all per-instance bits.
- **Secondary (elevated) — efficiency:** paired (TREAT−OFF) on **resolved-by-both** instances for
  `num_turns`, `output_tokens`, `total_cost_usd`, `duration_ms`. Negative ⇒ the layer made the re-run
  cheaper. Sign test + bootstrap CI (seed `20260619`, 10000 iters) on the mean delta. This is where a
  strong base model most plausibly shows compounding (same fix, fewer steps).
- **Attribution:** per Round-2 instance, record injected `seed_hits` and split into **own** (skill sourced
  from this instance's own Round-1 session) vs **cross** (another bench instance's). Distinguishes
  self-reapplication from within-bench transfer.

## Interpretation rule (pre-committed)

- **COMPOUNDS** — Y > X with McNemar gained > lost (sign p < 0.10), **OR** an efficiency mean-delta < 0
  with bootstrap CI excluding zero on ≥1 metric. (Either scoreboard moving in the layer's favour counts.)
- **NULL / UNDERPOWERED** — Y ≈ X and no efficiency CI excludes zero at N=10. A null is UNDERPOWERED, not
  "disproven" (N=10 is small by construction — this is a directional product signal, not a powered DiD).
- **REGRESSION** — Y < X (lost > gained) or efficiency deltas positive with CI excluding zero ⇒ the layer
  hurts (e.g. injection-induced scope-creep, as seen on django-10924 in the probe).
- **INSTRUMENT-FAILURE** — TREAT injected a seed on 0/10 instances (dead injection path → Y measures
  OFF-vs-OFF; void until fixed).

## Honest framing (stated up front)

This measures improvement on the system's **own recurring task distribution** — it is **memorization-
inclusive by design** (a skill mined from instance i's Round-1 run may encode i's fix and help i in Round 2;
that is the intended "wrote it down, reapplied it" compounding). It does **NOT** claim generalization to
unseen tasks. The own-vs-cross attribution reports *how* any Y>X arises. Resolved-rate gains specifically
require instances that **failed** Round 1 yet whose (failed-session) skill flips them in Round 2 — the hard
case; efficiency gains come from instances that pass both rounds, faster the second time.

## Invariants

Fresh empty scope (Round 2 injects ONLY skills learned from this bench's Round 1). Real swebench oracle —
no fakes (empty/non-applying patch ≠ resolved). Serial solves (one at a time, WSL2 rule). Drain-until-empty
(no arbitrary cap; stuck → fail loud). Dogfood `skill_layer_test` untouched — sha256 verified `1eb8d9fb…`
before and after. `--max-turns 40` (recorded, a stuck-detector not a work cap). Bootstrap seed/iters pinned.

---

## POWERED RUN ADDENDUM (N=40, 2026-06-20)

The N=10 loop returned a directionally-positive but UNDERPOWERED result
(`docs/assessments/2026-06-20-t15-selfseed-result.md`: X=0.60→Y=0.70, 1 flip, 0 regressions, sign p=1.0).
Per its own three-outcome rule that earns a **powered run**. Locked here BEFORE any N=40 solve:

- **N = 40 django instances** = `test_block_ordered[:40]` from `tests/fixtures/t15_swebench_split.json`
  (deterministic sha1(prereg_salt+id) order, **never observed**, **disjoint** from the N=10 set →
  no contamination): `16873, 14997, 15061, 15498, 15738, 15252, 15996, 14016, 15202, 14787, 13551,
  12700, 16041, 14534, 15320, 11620, 11630, 14667, 14580, 10914, 11910, 16408, 12983, 16820, 12308,
  13964, 14155, 12589, 16910, 13933, 11999, 11283, 13710, 13315, 14382, 15695, 16255, 11742, 14238, 12915`.
- **Same design + metrics + interpretation rule as above** (X→Y, McNemar primary, turns/tokens secondary,
  own-vs-cross attribution), unchanged. Fresh empty layer (`swebench_t15` dropped+recreated to 0 skills;
  Qdrant collection cleared; all prior scopes archived) → scope `swebench-django-n40`.
- **Power:** at the N=10 point estimate (base ≈0.60, ~+10pp, low discordance) N=40 still may not reach
  paired-McNemar significance — UNDERPOWERED remains a valid, reportable outcome (this is a directional
  product signal, not a clinical trial). The efficiency secondary gains power at N=40 (more resolved-by-both
  pairs). Bootstrap seed/iters pinned (`20260619` / 10000).
- **Robustness (added after the N=10 auth-expiry):** per-instance checkpoint (resumable); an invalid solve
  (claude 401/crash → `is_error`/empty `turns=1`) retries then FAILS LOUD + checkpoints — never banked as a
  fake not-resolved. ~80 serial solves, ~$40–60, ~10–20h (multi-session via checkpoint).
