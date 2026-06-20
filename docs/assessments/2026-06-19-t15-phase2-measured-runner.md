# T15 Phase 2 — measured SWE-bench compounding runner: BUILD + dry-run (2026-06-19)

Branch `feat/v-1-7`. Umbrella: `docs/plans/2026-06-19-t15-compounding-efficacy-phased-plan.md`.
Ticket: `todos/284`. Phase 0 (GREEN): `docs/assessments/2026-06-19-t15-phase0-mechanism-derisk.md`.

## VERDICT: **BUILT + dry-run GREEN** — the full chain runs end-to-end with no fakes.

Phase 2 turns the validated SWE-bench oracle + the de-risked seed→extract pipeline into a measured,
no-fakes DiD runner. This is **build + a 1-instance dry-run** proving the whole chain — NOT the powered
experiment (that is Phase 3, after N is locked).

## What was built (each piece no-fakes / fail-loud)

| Piece | Deliverable | Status |
|---|---|---|
| **A3** | Isolated clean data layer | `docker-compose.t15.yml`: `mcp-server-t15` (:3002) + `graph-builder-t15` (:8081) on `swebench_t15` DB + distinct Qdrant collection `skills__t15_swebench`, real `qwen3-embedding:4b`, `SKILL_PROJECT_MARKER=.skills`. **Boots at 0 skills**; dogfood `skill_layer_test` (277) never in pool. |
| **A** | SWE-bench oracle verifier | `t15_swebench_runner.py` wraps `swebench.harness.run_evaluation` → per-instance `report.json` → `resolved` bit + F2P/P2P counts. Missing report on a non-empty patch = HARD error. |
| **B** | Patch extraction | `git -C /testbed add -A && git diff --cached` → predictions.jsonl row `{instance_id, model_name_or_path, model_patch}`. Empty patch ⇒ recorded not-resolved (no harness run). |
| **D** | 3 arm wirings | OFF (no injection) / CTRL (foreign sympy scope) / TREAT (same django scope), byte-identical except the harness-mediated `compile_context` injection (`trigger=session_start` Priming). |
| **E** | Per-instance attribution | injected skill names/ids + which were SEED skills, per arm, recorded in the report. |
| **E2** | Live retrieval-of-seeds + separation | `t15_swebench_seed.py separation-test` — **PASS, 0 foreign leaks** (below). |
| **E3** | Merge-verifier JSON-miss guard | `crates/infrastructure/.../merge_verifier.rs` — a malformed `{equivalent, rationale}` body degrades to a conservative not-equivalent (logged), never crashes the worker. Worker rebuilt; 7/7 unit tests green. |
| **F** | DiD aggregator | `efficacy_ab.py` extended: `compute_did`, `bootstrap_ci_did`, `mcnemar_treat_vs_off`, `classify_did_verdict` (PASS/FAIL/UNDERPOWERED/INSTRUMENT-FAILURE) + `--self-test`. |
| **A0** | graph-builder vendored-SKILL.md hardening | filed `todos/287` (ignore `.venv*`/`site-packages`/`node_modules`). |
| Phase-1 | Deterministic split fixture | `tests/fixtures/t15_swebench_split.json` (django pool=111, SEED=12, TEST block=99; N unlocked). |

## Self-tests (no solves spent)

- `efficacy_ab.py --self-test` → **ALL PASSED (T14 sign-test + T15 DiD)**: DiD arithmetic, seeded
  bootstrap determinism, FAIL on DiD≤0, UNDERPOWERED on CI-spans-zero, INSTRUMENT-FAILURE on a dead
  injection path (0 seed injections) and on explicit attribution-confirmed regressions, McNemar counts.
- `t15_swebench_runner.py --self-test` → **ALL PASSED**: id/image parsing, predictions row, report-path
  layout, resolved parsing (true/false), **malformed report fails loud (no fabricated resolved)**, empty
  patch → not-resolved-without-harness, arm→scope mapping.

## Verifier live-validated (real harness, no solve)

The runner's oracle wrapper was proven end-to-end against the official harness with the **gold patch**:
`verify django__django-14999` → `resolved: true`, **F2P 1/1 success, P2P 113/113 success**,
`patch_applied: true`, `harness_rc 0`. The deterministic-oracle seam is real.

## A3 isolation evidence

- `/health` on :3002 → `embedding_arm: model=qwen3-embedding:4b dim=2560 collection=skills__t15_swebench`,
  `retrieval_backend: snapshot_dense`.
- `swebench_t15`: all migration tables present (`schema_migrations`, `skill_embeddings`, …); **skills=0
  at boot**, embeddings=0.
- Distinct Qdrant collection `skills__t15_swebench` created (alongside the untouched dogfood collections).
- Dogfood `skill_layer_test` = **277 skills, untouched**.

## C — seed → gate → reconcile into the clean layer

The 8 Phase-0 drafts were auto-gated into the clean scopes via the **real rename path**
(`.pending → SKILL.md`, T23 precedent): 3 django → `swebench-django`, 5 sympy → `swebench-sympy`.
`graph-builder-t15` reconciled → **PG skills=8, embeddings=8**; `mcp-server-t15` rebuilt its snapshot.
Per-scope inventory snapshotted (proves no foreign skill present):

- `swebench-django` (3): `assert-migration-noop-with-assertnumqueries-zero`,
  `django-migration-rename-noop-guard`, `guard-empty-string-before-index-access-python`.
- `swebench-sympy` (5): `slots-hierarchy-full-audit-principle`, `slots-mixin-missing-slots-restores-dict`,
  `symbolic-float-precision-mismatch-prevents-cancellation`, `sympy-codeprinter-unsupported-fn-via-piecewise`,
  `treat-missing-slots-in-a-parent-class-as-preference`.

## E2 — live retrieval-of-seeds + semantic separation (deferred from Phase 0): **PASS**

Fired the pre-registered aligned probes via the REAL `compile_context` (:3002, `trigger=session_start`),
each scoped to its own repo:

| probe | scope | own-seed hits | foreign leaks | result |
|---|---|---|---|---|
| `django-12708` | django | 3 django seeds | 0 | PASS |
| `django-16820` | django | 3 django seeds | 0 | PASS |
| `sympy-12171` | sympy | 3 sympy seeds | 0 | PASS |
| `sympy-14817` | sympy | 4 sympy seeds | 0 | PASS |

**all_pass=True, foreign_leaks=0.** The dual-scope separation (django queries → django seeds, sympy →
sympy) holds on the live server. Report: `tests/e2e/reports/swebench/e2_separation.json`.

**Key mechanism finding (carried to Phase 3):** the injection MUST use `trigger=session_start`
(`RetrievalIntent::Priming`, lower floor + query-side multi-view) — the production SessionStart priming
path. Task-intent `compile_context` no_matches verbose SWE-bench problem statements against a small seed
corpus (Risk #1 resurfaces at 3-skill scale); the Priming floor retrieves correctly. This is the correct
production-faithful intent for a session-start injection (Task intent is for mid-session focused prompts).

## SEED arc into the isolated layer (worker `DATABASE_URL=swebench_t15` + E3 live)

`t15_swebench_seed.py seed-drain`: re-ingested a real Phase-0 django seed-solve transcript to the clean
server (:3002 → `swebench_t15` queue), then drained via the **host frontier worker**
(`EXTRACT_SESSION_PROVIDER=claude-code`, `DATABASE_URL=swebench_t15`):

- worker **rc=0**, queue drained 1→0, **1 draft** produced (`guard-string-index-access-against-empty`).
- **dogfood `skill_layer_test` = 277, untouched** — extraction + its maintenance passes never read or
  mutated the dogfood DB (the HARD INVARIANT extraction clause, proven live).
- E3: the worker did **not** crash. The merge-verifier degrade path is in place (7/7 unit tests) and
  was a no-op this run (the recurrence pass produced no malformed-JSON candidate to degrade).

## Dry-run — 1 TEST instance × 3 arms, end-to-end (`did_t15-p2-dryrun.json`)

Plumbing instance `django__django-14999` (cached image; **pool-excluded** Phase-0 seed, chosen so no
held-out TEST instance is burned before the Phase-3 N-lock, and so the TREAT attribution path is
exercised). Full chain ran with NO fakes:

| arm | injection (status) | seed_hits | patch | oracle `resolved` |
|---|---|---|---|---|
| OFF | none | — | 3045 B | **True** |
| CTRL (sympy scope) | `no_match` | none | 2646 B | **True** |
| TREAT (django scope) | `ok` | `django-migration-rename-noop-guard`, `assert-migration-noop-with-assertnumqueries-zero` | 2289 B | **True** |

Every `resolved` bit came from the **official swebench oracle** (per-instance F2P/P2P `report.json`), not
inference; each arm solved a fresh instance container, the patch was the real `git diff`, and attribution
recorded exactly which seeds injected.

**Aggregator verdict: FAIL** — `DiD = TREAT(1.000) − CTRL(1.000) = +0.000`, CI [+0.000, +0.000]. This is
the classifier behaving **correctly** (DiD ≤ 0 → FAIL), and is **meaningless as efficacy**: N=1, the
instance is within Sonnet's competence so all three arms trivially resolve, and the instance is circular
(its own skill is a TREAT seed). The dry-run proves the *chain*, not a verdict (the runner prints this
caveat). The aggregator's PASS/UNDERPOWERED/INSTRUMENT-FAILURE branches are covered by `--self-test`.

### Two real findings carried to Phase 3

1. **CTRL frequently no_matches on cross-domain seeds.** The sympy foreign-seed corpus did not clear even
   the Priming floor for a django problem, so CTRL injected nothing (CTRL ≈ OFF here). If this holds at
   scale, the headline `DiD = TREAT − CTRL` collapses toward `TREAT − OFF` and the McNemar TREAT-vs-OFF
   becomes the operative test. Pre-registration should either accept this (report all three raw rates;
   the "generic-injection" confound is empirically ~0 for cross-domain seeds) or pick a topically-closer
   foreign corpus. **Not a blocker** — it is informative and the runner records it per-instance.
2. **Injection requires `trigger=session_start` (Priming).** Task-intent `compile_context` no_matches
   verbose SWE-bench problem statements on a small seed corpus; the SessionStart Priming floor retrieves
   correctly. The arms now use Priming (the production session-start path). Worth also making the
   production `settings-swebench.json` SessionStart hook pass `trigger=session_start` explicitly.

## Corpus integrity (HARD INVARIANT) — PROVEN

- **Dogfood `skill_layer_test`: byte-for-byte identical** before vs after the entire Phase-2 dry-run —
  277 names, sha256 `1eb8d9fb…` unchanged. Extraction, gating, and all three arm solves never mutated it.
- **Auto-gate scope guard**: the 8 experiment skills exist ONLY under
  `/tmp/t15-swebench/project/swebench-{django,sympy}/.skills/`. No experiment `SKILL.md` was written under
  the repo root or the dogfood corpus.
- **Production server (:3001) intact**: healthy, ollama / qwen3-embedding:4b / snapshot_dense /
  `skill_layer_test` / 277 skills. Production defaults never changed.

## Acceptance (todo 284) — all met

- [x] `--self-test` green — `efficacy_ab.py` (T14 sign-test + T15 DiD) and `t15_swebench_runner.py`.
- [x] Dry-run on 1 SEED (isolated-DB drain) + 1 TEST (3-arm, oracle-verified) end-to-end, no fakes.
- [x] Auto-gate writes only into `swebench-*` scopes; production corpus byte-identical (sha256 match).
- [x] Runner consumes the Phase-1 split fixture deterministically (`run --list`).

## Artifacts

- Runner: `scripts/t15_swebench_runner.py` (verify / solve-arm / run / --self-test).
- Seed + separation: `scripts/t15_swebench_seed.py` (gate-existing / seed-drain / separation-test / inventory).
- Aggregator: `scripts/efficacy_ab.py` (T15 DiD section + extended `--self-test`).
- Isolated stack: `docker-compose.t15.yml` (`dast15` project; :3002 / :8081).
- Split fixture: `tests/fixtures/t15_swebench_split.json`. Builder: `scripts/t15_build_split_fixture.py`.
- E3 fix: `crates/infrastructure/src/extraction/merge_verifier.rs` (+ 2 unit tests). Worker rebuilt.
- Reports: `tests/e2e/reports/swebench/{did_t15-p2-dryrun.json, e2_separation.json}`.
- Hardening todo: `todos/287` (graph-builder vendored-SKILL.md ignore).

## State left for Phase 3

The isolated `dast15` stack is **left running** (`mcp-server-t15` :3002, `graph-builder-t15` :8081) with
the 8 gated seeds in `swebench_t15`, ready for the powered run. Teardown when finished:
`docker compose -f docker-compose.t15.yml -p dast15 down` (the dogfood stack is independent). Production
defaults (dogfood on :3001 / ollama / `skill_layer_test`) were never altered.

## Phase 3 readiness

Everything Phase 3 needs is built and proven: isolated layer, deterministic oracle, 3-arm runner with
attribution, DiD aggregator, seed pipeline, separation test. Phase 3 = lock N (top of the test block),
seed the 12 pre-registered SEED instances into a fresh `swebench-django` scope, and run
`t15_swebench_runner.py run --n-test N`. Heed the two findings above (CTRL cross-domain no_match; Priming
intent) when finalizing the pre-registration.

