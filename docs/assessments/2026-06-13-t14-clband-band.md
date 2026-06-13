# T14 CL Acquisition BAND — full 8-context run (T23, 2026-06-13)

**What this is.** The morning report for T23: T14's full 8-context CL acquisition band, run end-to-end,
fully automated and unattended, under the LOCKED auto-gate amendment. T14 owns the pre-registration and
the verdict; T23 owned instruments-at-scale, the orchestrator, the run, and this report.

**Gate mode (recorded verbatim):** `auto-accept-all (clband-* scopes only)` — every `.pending` draft
in a context's `clband-<name>` scope accepted via the REAL rename path behind a hard `clband-*` scope
guard. The production human gate and the 262 dogfood corpus were UNTOUCHED.
**Solver checkpoint:** `claude-code 2.1.175, --model sonnet`. **Dataset:** `tencent/CL-bench` sha
`b28a5832a09b0d96c0cf4c22e90d7c60ede25b80`. **Session:** `work-2026-06-12-t23-band-run` (commits
`9be303b` Unit 0, `bb14c03`+`032f040` Unit B, `dddeb82` Unit A, + this).

---

## VERDICT vs the LOCKED pre-registration

> *"ON wins ≥ 7 of 10 paired tasks by sign test, with no catastrophic regression on any single task."*

**Outcome: INSTRUMENT-FAILURE — the efficacy question is UNANSWERED (N_clean = 0 paired points).**
NOT PASS, NOT FAIL, NOT UNDERPOWERED. The pipeline/instruments blocked measurement before any clean
ON-vs-OFF datum could be produced. Per the pre-registered taxonomy this is INSTRUMENT-FAILURE, reported
honestly — the layer was neither shown to help nor shown not to help.

The binding constraint is **extraction fidelity**, exactly the T22 failure class — but now exposed on
the larger / harder band contexts the smoke (flywheel 4.2k, aether 33.5k) never covered.

---

## Per-context outcomes (all 8, Pass 1 build + Pass 2 measure)

| # | context | OFF pre-gate | teach | drafts | fidelity | classification |
|---|---|---|---|---|---|---|
| 1 | material-handler-sops | loss/loss (discriminates) | ok | 25 | **FAIL** | genuine extraction gap (`<1 megaohm` dropped) |
| 2 | source-integrity-agent | loss/loss | ok | 20 | **FAIL** | genuine gap (enum sets `MATCH\|MISMATCH\|MISSING`, risk levels dropped) |
| 3 | quartermaster-hold-inventory | loss/loss | ok | 14 | **FAIL** | **FALSE-NEGATIVE — verifier PASSES on the drafts; recoverable** |
| 4 | dartman-game | loss (discriminates) | **timeout (rc=-2)** | 20 | PASS → built | measured but **CONFOUNDED** (see below) |
| 5 | ezlang-language | **win (NON-discriminating)** | — | — | — | OFF pre-gate dropped it (within model competence) |
| 6 | drywave-3000-manual | loss | ok | 23 | **FAIL** | genuine gap (40% floor + 8 specifics dropped) |
| 7 | 123corp-hr-policy | loss | ok | 11 | **FAIL** | genuine gap (diagnosis/prognosis/medical-cert dropped) |
| 8 | dpms-agent-m | loss/loss | ok | 7 | **FAIL** | genuine gap (M-WARN-01 / POSTERIOR_DISTRIBUTION dropped) |

**Tally:** 5 genuine extraction gaps · 1 fidelity-gate false-negative (recoverable) · 1 non-discriminating
· 1 timeout-confounded. **0 clean efficacy data points.**

### The decisive analysis — verifier-against-drafts (the authoritative classifier)
For each fidelity-RED context I ran its committed **deterministic verifier** (the tolerant, authoritative
instrument) against the concatenation of its accepted drafts — i.e. exactly what Session B's ON arm would
receive. This separates *genuine extraction gaps* (verifier ALSO fails → ON would fail → correct exclusion)
from *strict-sentinel false-negatives* (verifier PASSES → the rule survived, the exact-substring sentinel
gate wrongly excluded a measurable context):
- **5 genuine gaps** (material-handler, source-integrity, drywave, 123corp, dpms): the verifier fails too,
  because extraction dropped the verifier-precise specific (a numeric value, an enum value-set, a code).
  Excluding these is CORRECT — ON could not have passed.
- **1 false-negative** (quartermaster): the verifier PASSES — the invented rule (500/100 hold constants,
  HOLD_OK/HOLD_LOW/HOLD_CRITICAL_LOW status codes, the hard-stop, the Inventory Status report) DID survive
  extraction. The gate excluded it only because the *reworded* sentinel `100 percent of requirement` didn't
  exact-substring-match. With a verifier-based fidelity gate, quartermaster would have been MEASURED.

### dartman-game — the one "measured" context, and why it yields no signal
- Teach session **timed out** (rc=-2, 539 KB transcript); both Pass-2 solves **timed out** at the 1200 s
  stuck-detector (`on_elapsed_s 1202.1`, `placebo_elapsed_s 1201.6`). A timed-out solve produces no
  passing answer → ON=loss / PLACEBO=loss for a reason unrelated to the layer.
- PLACEBO was **degenerate**: dartman was the only built context, so the cross-scope rotation fell back to
  dartman's OWN scope (`placebo_donor: dartman-game`) — not a real matched-mass control.
- Attribution shows the injection PATH worked: ON retrieved + injected 3 dartman skills (4887 chars,
  status `ok`). `instrument_failure_injection=true` fired by the heuristic, but the root cause is the
  solve **timeout**, not obedience. dartman provides **no interpretable efficacy or injection reading.**

---

## What the band POSITIVELY established (the harness is sound)
- **OFF pre-gate discriminates** on genuinely novel CL-bench rules: 7/8 contexts had OFF=loss (the bare
  agent cannot produce the invented specifics). Only ezlang (depth-4) was within competence.
- **The retrieval / injection / auto-gate / scope-isolation machinery works end-to-end**, live, on the
  real stack: dartman built a real isolated scope (`clband-dartman-game`, 20 accepted), the running
  mcp-server reloaded and retrieved its 3 skills *scoped and isolated* (`compile_context` returned only
  dartman's skills), and the throwaway **canary** earlier proved the full write→accept→retrieve→remove→
  restore-262 path. The auto-gate fired only once (dartman), scope-guarded, logged (`auto_gate.json`);
  every other context was blocked at fidelity *before* acceptance, so **the corpus was never touched.**
- **Dogfood isolation re-probe:** project corpus reads exactly **262**, **0** `clband-*` scopes remain
  after closeout; `/health` green. (See `closeout.json`.) The 262 dogfood corpus is pristine.

## The binding constraint — extraction fidelity (refines T22)
T22 made the smoke GREEN by teaching the prose extractor to capture taught knowledge; that holds for
**rules, procedures, and structure** (the band's drafts faithfully carry them — e.g. quartermaster's
constants + status codes; dpms's section structure; material-handler's bake-out / escort / single-lot
rules). But the band shows extraction **still drops the most-specific verifier-precise tokens**:
`<1 megaohm`, the `MATCH|MISMATCH|MISSING` enum set, the *below-40% RH* floor, `diagnosis`/`prognosis`,
`M-WARN-01`. Because the deterministic verifiers REQUIRE those exact specifics (that is what makes the
tasks OFF-hard and non-pretrained), dropping them makes ON unable to pass — so 5/8 contexts are
genuinely unmeasurable today. The smoke escaped this because its operative tokens (`next size up`,
`conduit`, `<<`) were short and reword-resistant; the broader, value-dense band did not.

---

## Pre-registered secondaries (stated explicitly)
- **Paired turns-to-solve / token cost:** not captured per-arm (the harness records wall-clock elapsed
  only; per-turn/token capture needs `--output-format json`, a follow-up). Wall-clock is in each
  `sessionB/.../result.json`; dartman's arms hit the 1200 s cap.
- **Judge-rubric score (secondary):** not run — Session B reached only dartman, which was confounded;
  no judge scoring is meaningful on a timed-out solve.
- **ON-vs-PLACEBO:** only dartman, and its placebo was degenerate (self-scope) → no comparison.

## Provenance & fences (all honored)
- **No planting:** every rule travelled context → genuine teach session → real extraction → `.pending` →
  auto-accept (clband scope) → scope rebuild. When extraction dropped a value, the result was *reported*,
  never hand-edited into a skill.
- **Auto-accept only under `clband-*`:** the scope guard asserted before every rename; **zero** non-clband
  paths touched (24 scope-guard unit tests green; `auto_gate.json` logs the one acceptance).
- **Production + dogfood gates untouched;** re-probe confirms 262.
- Measurement drove the **REAL mcp-server over HTTP**; no crate/ranking/floor changes; injection was the
  labeled focused inject-query mode; no fakes; **nothing deleted**; workspace gates green.

---

## Recommendations (owner decides — NO unilateral protocol change was made mid-run)
1. **Extraction value-preservation (the real blocker, T22-followup ticket).** Extraction must carry
   verifier-precise specifics — numeric thresholds, enum value-sets, invented codes — VERBATIM, not just
   the surrounding rule. The 5 genuine-gap contexts + their dropped tokens are ready acceptance fixtures
   (`<1 megaohm`, `MATCH|MISMATCH|MISSING`, the 40% floor, `diagnosis`/`prognosis`, `M-WARN-01`).
2. **Verifier-based fidelity gate.** Replace the exact-substring operative-sentinel check with running the
   committed verifier against the accepted drafts (the authoritative, tolerant instrument). This recovers
   false-negatives (quartermaster) and ties the gate to what Session B actually measures. Pre-registerable
   as an instrument amendment before a re-run.
3. **Task-design fixes.** Raise/remove the solve timeout for deep game tasks (dartman, depth-8) or drop
   dartman; replace ezlang's non-discriminating depth-4 sibling (OFF won) or drop ezlang. Author the 3
   alternate contexts' instruments so the substitution path is real.
4. **Re-run after (1)+(2).** The harness, instruments, auto-gate, and scope isolation are all proven; a
   re-run with value-preserving extraction + a verifier-based gate would actually measure efficacy.

## Artifacts (every number traces to one)
- `tests/e2e/reports/efficacy/clband-band/band_results.json` — verdict + the dartman row.
- `tests/e2e/reports/efficacy/clband-band/checkpoint.json` — per-(context, step) state.
- `tests/e2e/reports/efficacy/clband-band/<context>/` — per context: `offpregate/` (OFF solves + verifier
  reasons), `transcript.jsonl` + `teach_solution.md`, `extract.log`, `scope/.skills/**/*.pending` (the
  drafts), `fidelity_gate.txt`, and (dartman) `auto_gate.json` + `sessionB/`.
- `tests/e2e/reports/efficacy/clband-band/closeout.json` + `run.log`.
- `docs/execution-sessions/work-2026-06-12-t23-band-run/` — STATE + per-unit session files.
