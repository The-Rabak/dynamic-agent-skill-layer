# Next-session work prompt — T15 is DONE (efficacy proven); commit + decide what's next

Branch `feat/v-1-7`. This hands off a **completed** T15 (the primary efficacy gate). Nothing is committed
yet — that's the top recommended action.

## TL;DR — what happened

T15 asked: *does the skill layer compound — learn from its own sessions and make future sessions
measurably better?* Answered with a **same-set self-improvement loop** on real SWE-bench django (solve a
bench OFF → score X, extract skills from those sessions into a clean layer, re-solve the SAME bench ON →
score Y), verified by the deterministic SWE-bench F2P/P2P oracle. No fakes.

**Verdict (powered N=40): COMPOUNDS on EFFICIENCY (statistically significant), NULL on resolved-rate.**
The project's first significant, pre-registered, no-fakes efficacy win.

- Resolved-rate: X=0.600 → Y=0.625 (+0.025), McNemar gained 2/lost 1, p=1.0 → **NULL** (14/16 OFF-failures
  held even with the exact relevant skill injected → the gap is *execution*, not knowledge).
- Efficiency (n=23 resolved-by-both, TREAT−OFF): **output_tokens Δ=−3,584, 95% bootstrap CI [−7177,−593]
  EXCLUDES ZERO**; **duration Δ=−55.7s, CI [−106570,−12050] EXCLUDES ZERO**; num_turns (−1.4) and cost
  (−$0.077) same direction but CIs span 0. All four deltas negative. By the pre-registered rule (efficiency
  mean-delta<0 + bootstrap CI excludes 0 on ≥1 metric) → **COMPOUNDS**.
- Meaning: the layer doesn't make Sonnet solve *more* instances, it makes it solve the *same* ones
  **measurably cheaper** — it stops re-deriving repo conventions each session. This is exactly the
  hypothesis the de-risk probe predicted ("the benefit shows as saved exploration, not fails→passes").

Full verdict: `docs/assessments/2026-06-20-t15-selfseed-n40-VERDICT.md`. Read this first.

## Read order for context

1. `docs/assessments/2026-06-20-t15-selfseed-n40-VERDICT.md` — the powered result (authoritative).
2. `docs/assessments/2026-06-20-t15-selfseed-precommit.md` — the pre-registration (+ N=40 addendum).
3. `docs/assessments/2026-06-20-t15-selfseed-result.md` — N=10 underpowered precursor.
4. `docs/assessments/2026-06-20-t15-derisk-probe.md` — why the pivot to the same-set / efficiency design.
5. `docs/plans/2026-06-19-t15-compounding-efficacy-phased-plan.md` — umbrella plan + HARD INVARIANTS.

## Live state (verified end of session 2026-06-20)

- **Isolated `dast15` stack UP**: `dast15-mcp-server` (:3002), `dast15-graph-builder` (:8081), DB
  `swebench_t15` = **56 skills** (self-mined from the N=40 OFF transcripts), Qdrant `skills__t15_swebench`,
  scope `/tmp/t15-swebench/project/swebench-django-n40/.skills`. Teardown:
  `docker compose -f docker-compose.t15.yml -p dast15 down`.
- **Dogfood production UNTOUCHED**: :3001, DB `skill_layer_test`, 277 skills, ollama/qwen3-embedding:4b,
  sha256 of names = `1eb8d9fb…` (baseline `/tmp/t15-dogfood-names-before.txt`). Verified byte-identical
  before/after every phase.
- **50 SWE-bench django images cached (~70GB)**; disk 190G free (21% used).
- Problem-statement cache: `/tmp/t15-swebench/_cache/swebench_lite_problems.json` (300 rows, one-shot).
- Archived prior scopes/solve dirs: `/tmp/t15-swebench/_archive/` (kept, not deleted).
- `claude` CLI 2.1.181 on PATH (own auth). swebench venv `/tmp/t15-venv-swebench` (swebench 4.1.0).

## Reports & key data

- `tests/e2e/reports/swebench/selfseed_selfseed-django-n40.json` — the powered result (X/Y, per-instance,
  attribution, efficiency aggregate). Plus `…-n10.json` / `…-n10-r2.json` (N=10) and
  `…-n10.TAINTED-auth401.json` (the auth-tainted R2, preserved for the record).
- `did_t15-derisk-probe.json` — the de-risk probe.
- Logs: `logs/t15-n40/` (run.log, complete-seed.log, image-pull.log), `logs/t15-selfseed/<run-id>/`
  (per-instance **checkpoint.json** + drain-pass logs).
- Per-instance solve patches/logs: `/tmp/t15-swebench/solve/<iid>__{off,treat}/`.

## NOTHING IS COMMITTED — recommended action #1

All of this is uncommitted on `feat/v-1-7`. New (untracked): the `scripts/t15_*.py` suite
(`t15_selfseed_loop.py`, `t15_swebench_runner.py`, `t15_swebench_seed.py`, `t15_complete_seed.py`,
`t15_extract_quality_check.py`, `t15_build_split_fixture.py`, phase0 scripts), all `docs/assessments/
2026-06-{19,20}-t15-*.md`, `docker-compose.t15.yml`, `tests/fixtures/t15_swebench_split.json`, the reports.
Modified (tracked): `scripts/efficacy_ab.py` (T15 DiD + efficiency aggregator + self-tests),
`crates/infrastructure/src/extraction/merge_verifier.rs` (E3 degrade fix). Self-tests:
`python3 scripts/efficacy_ab.py --self-test`, `…/t15_swebench_runner.py --self-test`,
`…/t15_selfseed_loop.py --self-test` — all green.

→ **Commit the T15 work** (suite + aggregator + verdict docs). Suggest one focused commit for the harness
and one for the assessment docs, or a single `feat(v1.7): T15 compounding efficacy — COMPOUNDS on efficiency`.

## Recommended next actions (in priority order)

1. **Commit the T15 work** (above). Biggest pending item.
2. **The ship decision for v1.7.** T15 was the gate the whole release rested on. It now has a real,
   honest, scoped answer: the layer compounds by cutting exploration cost (~3.6k tokens / ~56s per recurring
   task), not by raising pass-rate. Decide whether that clears the bar to ship qwen3-default + the layer, and
   update the v1.7 thesis/README accordingly. This is an owner call — surface the verdict, don't assume.
3. **(Optional, strongest follow-up) Replicate on a 2nd repo to show the efficiency win generalizes.**
   Re-run the same loop on **sympy** (77 Lite instances; shorter problem statements, cheaper). Same
   `t15_selfseed_loop.py --scope-name swebench-sympy-n40 --instances <sympy ids>`. If output-tokens Δ<0
   with CI excluding zero replicates, the compounding claim is repo-general, not django-specific. ~$40-60.
4. **(Optional) Add the CTRL arm to subtract "generic context helps".** The same-set loop is TREAT-vs-OFF;
   a foreign-repo-seed CTRL (the original DiD design, already supported by `t15_swebench_runner.py`) would
   prove the *token savings are repo-specific*, not "any injected context shortens solves." Cheaper variant:
   re-run a subset of the 23 resolved-by-both under a CTRL (sympy-seeded) scope and compare the token delta.
5. **(Optional) Investigate the 1 regression** (`13551`, ✅→❌): inspect `/tmp/t15-swebench/solve/
   django__django-13551__treat/` — did an injected skill mislead it? One data point, but the
   injection-distraction failure mode is worth understanding before any production SessionStart wiring.
6. **(Optional) Tighten the efficiency stat.** The pre-reg "≥1-of-4 metrics" rule has multiple-comparison
   risk; the result survives because the *bootstrap CI* (not the sign-test p) is the gate and clears
   comfortably. For a publishable claim, pre-register output_tokens as the single primary efficiency metric
   and re-confirm on a fresh bench (repo #2 doubles as this).

## Cleanup the owner may want (ask first)

- Tear down `dast15` + reclaim disk: the **50 swebench django images ≈ 70GB**. The stack itself is light.
  `docker compose -f docker-compose.t15.yml -p dast15 down` + optionally
  `docker rmi $(docker images -q 'swebench/sweb.eval.x86_64.django_1776_*')`. **Keep the reports/logs/
  /tmp/t15-swebench/solve patches** (freshly-generated outputs — never auto-delete).
- Production defaults already intact (dogfood on :3001 / ollama / `skill_layer_test`). Nothing to restore.

## Standing rules (carry forward)

Measurement drives the REAL server + REAL instance containers + REAL oracle — no fakes / fail loud. Serial
heavy actions (one solve / one build at a time — WSL2 crash rule). **Drain large-transcript batches in
SMALL WAVES, not 40-at-once** (frontier extraction spawns many claude subprocesses → overloaded WSL2 this
session; `drain_until_empty` failed loud, recovered via `t15_complete_seed.py`; keep a memory watchdog).
Never delete freshly-generated outputs. Cost-estimate before cloud-LLM runs (N=40 cost ~$70 solve + ~$10
extraction; harder `test_block` instances ran pricier than estimated). After every run, verify dogfood
integrity (`1eb8d9fb…`). Auto-gate ONLY into isolated `swebench-*` scopes; dogfood human gate untouchable.

## Gotchas that bit this session (heed them)

- **Seed-hit attribution must key on the frontmatter `name:`**, not the slug dir name (graph-builder +
  compile_context use `name:`; preference-type skills slug-differ wildly). Fixed in
  `runner.seed_skill_names` + `loop.gate_drafts`.
- **`is_error=True` is ambiguous**: it marks BOTH a real auth-401 AND a legitimate `--max-turns` cap-hit
  (e.g. django-15738: 40 turns, $1.31, 0-byte patch, `terminal_reason=max_turns`). The final guard
  (`loop._solve_is_invalid`) retries ONLY on a true crash (no JSON) or explicit auth (`api_error_status`
  401 / "failed to authenticate" / "invalid authentication credentials") — a max-turns empty-patch solve
  is a REAL not-resolved, banked, never retried. Never bank an auth-empty solve as a fake not-resolved.
- **HF datasets-server rate-limits** a burst of per-instance fetches (429). Use the bulk one-shot
  `build_problem_map()` cache (already in `t15_selfseed_loop.py`).
- **claude `--disallowed-tools "…,LS,MultiEdit"`** prints "matches no known tool" to stderr but **exits 0**
  — harmless noise in `claude_code.rs:65`, NOT a failure cause (verify before chasing it again).
- **graph-builder reconciles a NEW scope dir only after a restart** (file-watch set at boot). `seedmod.
  reconcile_and_rebuild` restarts mcp-server-t15; relocate/remove scopes + restart graph-builder to reset.
- **swebench `make_run_report` writes `<model>.<run_id>.json` to CWD** (repo root) — relocate to
  `tests/e2e/reports/swebench/harness-summaries/` (done this session; will recur on future runs).
- **Per-instance checkpointing** makes any interruption a clean resume: re-run the EXACT same loop command;
  it re-solves only what's missing. Problem statements are re-fetched (cached), not checkpointed.

## How to reproduce / resume the N=40 (if needed)

```bash
cd /home/rabak/projects/dynamic-agent-skill-layer
# stack up?  curl -s http://127.0.0.1:3002/health
python3 scripts/t15_selfseed_loop.py \
  --run-id selfseed-django-n40 --scope-name swebench-django-n40 \
  --instances <test_block_ordered[:40], see split fixture> --model sonnet --max-turns 40
# resumes from logs/t15-selfseed/selfseed-django-n40/checkpoint.json (now fully complete)
```
