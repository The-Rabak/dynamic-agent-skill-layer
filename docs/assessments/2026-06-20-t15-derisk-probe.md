# T15 de-risk probe — OFF + TREAT on 5 SEED-block django instances (2026-06-20)

Branch `feat/v-1-7`. Umbrella: `docs/plans/2026-06-19-t15-compounding-efficacy-phased-plan.md`.
Phase 2 record: `docs/assessments/2026-06-19-t15-phase2-measured-runner.md`. Ticket: `todos/285` (Phase 3).

## Purpose

A cheap (~$3–8) directional signal check **before** committing the ~$30–60 powered Phase-3 run. The
Phase-2 dry-run surfaced a real risk that the effect may be undetectable; this probe was the kill-or-
continue gate. It is **not** a powered verdict (N=5, 2 flip opportunities) — it is a heuristic.

## Setup (no-fakes, reuses everything built in Phase 2)

- **Arms:** OFF (no layer) + TREAT (`swebench-django` scope, the 3 gated Phase-0 django seeds), injected
  via the real `compile_context` with `trigger=session_start` (Priming floor). CTRL omitted (cost).
- **Instances:** 5 SEED-block django (`15789, 11099, 10924, 11564, 13321`). **Non-circular:** the 3 TREAT
  seeds derive from Phase-0 instances (14999/16046/13447), not from these 5. Held-out TEST block untouched.
- **Model:** Sonnet, `--max-turns 40`, one solve at a time (WSL2 serial rule). Every `resolved` bit from
  the official SWE-bench F2P/P2P oracle (per-instance `report.json`). Real instance containers, real patches.
- Report: `tests/e2e/reports/swebench/did_t15-derisk-probe.json`.

## Result

| instance | topic | OFF | TREAT | TREAT injection (status / seeds) | OFF vs TREAT patch |
|---|---|---|---|---|---|
| 15789 | `json_script()` encoder arg | ✅ pass | ✅ pass | ok / 1 seed | **byte-identical** (931 B) |
| 11099 | (resolved unaided) | ✅ pass | ✅ pass | ok / 2 seeds | **byte-identical** (901 B) |
| 10924 | (resolved unaided) | ✅ pass | ✅ pass | ok / 2 seeds | DIFFER (1.2 KB → **135 KB scope creep**) |
| 11564 | SCRIPT_NAME in STATIC/MEDIA_URL | ❌ fail | ❌ fail | **no_match / 0 seeds** | DIFFER (both fail) |
| 13321 | decode invalid session data crash | ❌ fail | ❌ fail | ok / **3 seeds** | **byte-identical** (720 B) |

- **OFF resolved-rate = 3/5 = 60%. TREAT = 3/5 = 60%. TREAT == OFF on every instance.**
- **Flips (OFF-fail → TREAT-pass): 0 / 2 opportunities.**
- On **4 of 5** instances the injected skill context **did not change the agent's solution**: 3 byte-
  identical patches, 1 harmful scope-creep (10924: 135 KB vs 1.2 KB, still resolved → no regression but
  pure noise). Only 11564 had no injection (no_match).

## The decisive instance — 13321

`django-13321` ("decoding invalid session data crashes"; fix in `sessions/backends/base.py`) is the one
clean test of the mechanism: an OFF-**failure** where TREAT injection **fired** (status=ok, all 3 django
seeds retrieved over the Priming floor). Outcome: TREAT produced a **byte-identical failing patch** to OFF.
The injected context altered nothing. The 3 seeds (migration-rename-noop, assert-migration-noop, empty-
string-index-guard) are **topically irrelevant** to session decoding — the empty-string guard was
vaguely adjacent enough to clear the floor, but offered no value to the actual fix.

## Honest reading (both halves stated)

**Pessimistic signal (real):**
1. **High OFF base-rate (60%).** Sonnet solves the majority unaided — limited headroom, exactly the
   T14/T23 rock and the work-prompt's prior ("OFF already solves the rocks").
2. **Injection is largely inert on solution content.** 3/5 byte-identical, 1/5 harmful noise. The
   resolved-rate metric is blind to compounding here: Sonnet solves (or fails) the same way with or
   without the seed. This is the work-prompt's "the metric may be blind to the benefit" confirmed.
3. **0/2 flips**, and on the single opportunity where seeds genuinely injected into a failure (13321),
   the patch was byte-identical → **no conversion**.

**Probe limitation (equally real — do not overclaim):**
- The probe reused only the **3 narrow Phase-0 seeds**. Both OFF-failures sampled (11564 routing, 13321
  session-decode) fall **outside** those seeds' topics, so the probe **never sampled a seed-covered
  OFF-failure** — the one condition under which TREAT could flip. 11564 correctly `no_match`ed; 13321's
  seeds were off-topic. The probe therefore confirms "high OFF + inert injection on uncovered topics"
  but **did not cleanly test** "does a *relevant* seed convert a failure."
- The powered design re-seeds **12 fresh pre-registered SEED instances** precisely to get topical
  coverage of the TEST set; the probe's narrow corpus is not design-faithful.

## Decision-rule outcome

Per `todos/285` / the work-prompt rule: OFF=60% (moderately high, near the ≥70%-underpowered band) with
**0 flips** → **do NOT spend the powered budget on the resolved-rate metric.** The de-risk did its job:
it caught a likely FAIL/UNDERPOWERED resolved-rate run for ~$5 before $30–60. **Phase 3 (resolved-rate)
is not justified as-is.** Pivot — owner decision recorded separately (see "Decision" below once chosen).

## Integrity (standing rules — verified)

- **Dogfood `skill_layer_test` byte-identical**: sha256 `1eb8d9fb…` == pre-run baseline. Untouched.
- **Isolated `swebench_t15` unchanged**: 8 skills (3 django / 5 sympy) — the probe only reads via
  `compile_context` and solves in throwaway containers; no extraction/gating ran.
- Stray swebench `make_run_report` summaries (10) relocated from repo root →
  `tests/e2e/reports/swebench/harness-summaries/`.
- Solve logs + patches retained under `/tmp/t15-swebench/solve/<iid>__<arm>/`.

## Cost

10 Sonnet solves @ max-turns 40 + 10 oracle verifications. Within the ~$3–8 de-risk budget.
