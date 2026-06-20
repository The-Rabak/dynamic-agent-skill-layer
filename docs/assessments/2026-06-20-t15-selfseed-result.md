# T15 self-seeding same-set compounding loop — RESULT (2026-06-20)

Branch `feat/v-1-7`. Pre-commitment: `docs/assessments/2026-06-20-t15-selfseed-precommit.md` (written
before Y was observed). Design: run a bench OFF → **X**, extract skills from those sessions into a fresh
empty layer, re-run the **same** bench ON → **Y**; compounding = Y > X. N=10 django SWE-bench Lite.

## VERDICT: directionally POSITIVE but UNDERPOWERED (not a confirmed compounding claim)

**X (Round 1, OFF) = 0.60 → Y (Round 2, TREAT, self-seeded) = 0.70. Δ = +0.10 (one instance flipped,
zero regressions).** By the pre-registered rule this is **NULL / UNDERPOWERED leaning positive**: the
point estimate moved the right way with no harm, but with a single discordant pair the sign test cannot
reach significance (p=1.0) and no efficiency CI excludes zero. It is a real, no-fakes proof-of-concept —
the mechanism demonstrably fired once — that justifies, but does not substitute for, a powered run.

## What ran (no fakes, real oracle)

10 django instances, each solved twice (independent fresh solves differing ONLY in the layer injection):
`14999, 16046, 13447, 15789, 11099, 10924, 11564, 13321, 11133, 13660`. Every `resolved` bit from the
official SWE-bench `FAIL_TO_PASS`/`PASS_TO_PASS` oracle. Between rounds: all 10 OFF transcripts were
extracted by the real host frontier worker (`claude-code`) into a fresh empty scope → **15 skills gated**
(covering 9/10 instances) → reconciled. Round 2 injected via the real `compile_context`
(`trigger=session_start`), **a seed on 10/10 instances** (injection path healthy).

> **Auth-failure recovery (honest disclosure).** The first Round-2 pass was tainted: the `claude` CLI
> auth token expired mid-run (`401`), so 9/10 TREAT solves returned empty patches (`turns=1, $0`). That
> tainted run is preserved as `selfseed_..._n10.TAINTED-auth401.json` and is NOT the result. Round 2 was
> re-run cleanly (new `--resume-round2` mode) against the **unchanged** seeded scope; Round 1 (X) and the
> 15-skill corpus were reused as-is. The numbers below are the clean re-run.

## Per-instance (X → Y, own-skill injected, turns)

| instance | OFF | TREAT | flip | turns OFF→TREAT | own-skill injected |
|---|---|---|---|---|---|
| **14999** | ❌ | ✅ | **FLIP+** | 41 → 32 | `django-renamemodel-dbtable-noop-guard` |
| 16046 | ✅ | ✅ | · | 11 → 15 | guard-string-index-before-access |
| 13447 | ✅ | ✅ | · | 22 → 25 | (none; cross-skills only) |
| 15789 | ✅ | ✅ | · | 14 → 12 | (preference echo) |
| 11099 | ✅ | ✅ | · | 16 → **10** | python-regex-anchors |
| 10924 | ✅ | ✅ | · | 26 → **36** | django-field-callable-argument-pattern |
| 11564 | ❌ | ❌ | · | 45 → 46 | django-script-name-prefix-relative-urls |
| 13321 | ❌ | ❌ | · | 17 → 11 | ensure-all-decode-ops-inside-try-except |
| 11133 | ✅ | ✅ | · | 21 → **12** | handle-memoryview-before-str-fallback |
| 13660 | ❌ | ❌ | · | 17 → 15 | exec-with-globals-dict-for-function-visibility |

**McNemar TREAT-vs-OFF: gained 1, lost 0** (monotone — no OFF-pass regressed under the layer).

## The one flip is the gold mechanism (and its caveat)

`django-14999` failed OFF, then resolved under TREAT with its **own** self-mined skill
`django-renamemodel-dbtable-noop-guard` injected (attribution-confirmed own-hit). The compounding twist:
**that skill was extracted from 14999's own FAILED Round-1 session** — the extractor mined a correct
RenameModel db_table noop-guard from a session that did not complete the fix in 40 turns, and on retry
with that skill in context the agent finished it in fewer turns (41→32). That is precisely "the system
wrote down what it figured out and reapplied it."

**Honest caveat:** 14999 is a borderline/variance-prone instance — it resolved OFF in the Phase-2 dry-run,
failed OFF here, resolved TREAT here. With a single discordant pair we **cannot** cleanly separate
"the skill caused the flip" from run-to-run variance. This is the core reason the result is underpowered,
not conclusive.

## Efficiency (turns/tokens-to-resolve) — net wash at N

On the 6 instances resolved by **both** rounds, all four metrics' bootstrap CIs span zero:
`num_turns` mean Δ = 0.0 (3/6 cheaper), `output_tokens` Δ = +178 (3/6), `total_cost_usd` Δ = +$0.025
(2/6), `duration_ms` Δ = −1116 (3/6). The per-instance turn deltas are **bimodal**: clear wins
(11099 16→10, 11133 21→12, 15789 14→12) cancelled by losses (10924 26→36, 16046 11→15) — the injected
cross-instance skills sometimes add scope rather than focus. No efficiency signal survives at n=6.

## The 3 OFF-failures that did NOT flip

`11564` (SCRIPT_NAME-in-URL feature — genuinely hard; 46-turn cap with its own skill), `13321` (decode
crash — its own skill steered it to a *confident wrong/incomplete* fix, terminating faster at 11 turns
but not matching the gold patch), `13660` (exec-globals). So the mechanism converted **1 of 4**
OFF-failures; 3/4 had a relevant own-skill injected yet still missed — the skill named the area but didn't
carry the exact gold fix.

## Honest bottom line

- **Real, no-fakes, oracle-verified.** The layer **did** compound on its own work at least once, by the
  intended mechanism (mine a fix from a session — even a failed one — and reapply it on retry), with
  **zero regressions** across 10 instances.
- **But underpowered and not significant.** +10pp / 1 flip / sign-p=1.0 / efficiency net-zero, and the one
  flip sits on a variance-prone instance. By the pre-registered three-outcome rule this is
  **UNDERPOWERED**, not "COMPOUNDS."
- This is the **first positive efficacy signal** in the project (vs T14 non-discriminating, T23
  instrument-failure, the 2026-06-20 cross-instance probe's flat 0/2). It earns a **powered run**
  (larger N to resolve significance + separate skill-effect from variance), not a victory lap.

## Cost & integrity

Clean Round 1 + Round 2 ≈ **$9.6** (R1 $5.46 + R2 $4.13); with the de-risk probe, smoke, extraction, and
tainted-R2 overhead, total session ≈ **$20–25** — within the Path-B ~$15–30 envelope. **Dogfood
`skill_layer_test` byte-identical** before and after (sha256 `1eb8d9fb…`); the isolated `swebench_t15`
layer holds the 8 base seeds + 15 self-seeded skills; production defaults (`:3001` / ollama) untouched.

## Artifacts

- Clean result: `tests/e2e/reports/swebench/selfseed_selfseed-django-n10-r2.json` (Round-2 re-run;
  reuses Round 1 from `selfseed_selfseed-django-n10.json`). Tainted run preserved:
  `selfseed_selfseed-django-n10.TAINTED-auth401.json`.
- Orchestrator: `scripts/t15_selfseed_loop.py` (+ `--resume-round2`). Efficiency instrument:
  `scripts/t15_swebench_runner.py` (`--output-format json` capture) + `efficacy_ab.aggregate_efficiency`.
- Extraction-quality pre-check: `scripts/t15_extract_quality_check.py` (4/6 useful on a 5-transcript probe).
