# T15 Phase 0 — compounding-mechanism de-risk result (2026-06-19)

Branch `feat/v-1-7`. Gates the powered T15 experiment (`todos/283-286`). Umbrella:
`docs/plans/2026-06-19-t15-compounding-efficacy-phased-plan.md`. Ticket: `todos/282`.

## VERDICT: **GREEN** — proceed to Phase 1 (pre-registration) + the powered DiD.

Both repos clear the gate. SWE-bench solves yield **reusable, typed, repo-specific, multi-view**
drafts — not episodic junk — so the compounding thesis survives at the mechanism level. Recommended
powered-run repo: **django** (most instances → best power; the migration-convention skill is the
archetypal recurring-procedure knowledge), with a Phase-1 caveat on topic coverage (below).

## What ran (real pipeline, no fakes)

- **6 seed solves**, frontier provider (`EXTRACT_SESSION_PROVIDER=claude-code`, sonnet-4-6) through the
  gated host worker (`target/debug/maintenance-worker` rebuilt 13:25 on commit `50f5dee`):
  - django: `14999` (RenameModel noop), `16046` (numberformat), `13447` (adminsite) — all rc=0, real
    source edits, 786-test suite green on 14999.
  - sympy: `20590` (`__slots__` hierarchy), `13146` (`Basic.__slots__`/`_mixin`), `11400` (ccode
    Piecewise) — all rc=0, real source edits.
- Two isolated workspace scopes (`/tmp/swebench-phase0-{django,sympy}`); transcripts captured per
  scope; ingested with `repo_path=<workspace>` and drained per scope (`scripts/t15_phase0_seed_extract.py`).
- Served 277-corpus **byte-count intact (277 before/after)**; global scope untouched (0); queue drained.

## Risk #2 (extraction fidelity) — **ANSWERED: YES.** 6 solves → 8 drafts.

### django (3 drafts; 2/3 repo-specific reusable)
| draft | type | reusable? |
|---|---|---|
| `assert-migration-noop-with-assertnumqueries-zero` | `best_practice` | **YES — broad django convention.** `assertNumQueries(0)` to prove a migration op emits no SQL; `use_when`/`requires`/`produces`/`invariants` all populated; invariant captures the no-vacuous-pass discipline. Plausibly helps many of django's 114 instances. |
| `django-migration-rename-noop-guard` | `failure_fix` | **YES — same neighborhood.** Real APIs (`._meta.db_table`, `database_forwards`), real paths (`db/migrations/operations/models.py`). |
| `guard-empty-string-before-index-access-python` | `failure_fix` | general Python idiom (not repo-specific, still reusable). |

### sympy (5 drafts; 4/5 typed repo-specific reusable)
| draft | type | reusable? |
|---|---|---|
| `slots-mixin-missing-slots-restores-dict` | `failure_fix` | **YES — sympy core class-hierarchy gotcha.** |
| `symbolic-float-precision-mismatch-prevents-cancellation` | `failure_fix` | **YES — sympy symbolic diagnostic.** |
| `sympy-codeprinter-unsupported-fn-via-piecewise` | `best_practice` | **YES — sympy codegen convention** (`_print_{Fn}` via Piecewise). |
| `slots-hierarchy-full-audit-principle` | `principle` | **YES — reusable architecture principle.** |
| `Treat missing __slots__ in a parent class as …` | *(type-less)* | preference/episodic tier (the low-value class — 1/8 overall). |

**Yield: django 2/3, sympy 4/5 repo-specific reusable — both a clear majority.** 0 skeleton-mined
drafts (the `50f5dee` gate is live on the frontier tier). One django solve (13447) and the merge step
produced no kept draft — expected; the typing/grounding gate is selective.

## Risk #1 (verbose `compile_context` dilution) — **RETIRED (free, pre-Phase-0).**

`scripts/t15_phase0_verbose_retrieval_probe.py`: 4/4 verbose multi-paragraph issue statements →
`status=ok` with topically-relevant rich skills from the live 277-corpus. T18's verbose coverage@3
0.027 was the fixture-gold artifact, not a retrieval failure.

## Project separation — demonstrated structurally; live retrieval test → Phase 2.

The two repos extracted into **fully separate scopes with zero cross-contamination**: every django
draft is django/migrations/python-topic, every sympy draft is sympy/slots/symbolic/codegen-topic, each
tagged with its own `source_session_id` (`t15p0-django-*` vs `t15p0-sympy-*`). No draft crossed scopes.

The **live `compile_context` retrieval-of-seeds + semantic-separation test is deferred to Phase 2**, by
two honest constraints: (a) making the freshly-seeded /tmp skills retrievable on the real server needs
the workspace wired as a watched scope (Phase-2 work) — the global-scope shortcut was correctly blocked
by the isolation guardrail (production/global must stay untouched); (b) a hand-rolled offline
cosine check is forbidden by the standing "measurement drives the real server, no in-process
reconstruction" rule. Aligned held-out probes are selected and recorded for Phase 2: django `12708`,
`16820` (migration neighborhood); sympy `12171`, `14817` (codeprinter neighborhood).

## Found issue (flag for Phase 2 — does NOT block Phase 0)

The post-extraction **recurrence/promotion maintenance pass crashes** (`worker rc=1`): the ollama
merge-verifier returned malformed JSON (`missing field 'rationale'`) during recurrence clustering.
Drafts are written *before* this pass, so extraction is unaffected — but the powered run drains
repeatedly and the worker should not hard-fail on a merge-verifier JSON miss. Guard/fix it in Phase 2
(`maintenance` recurrence pass; same ollama-JSON class as prior `think:false` fixes). Positive
corollary: the grounding validator correctly **dropped an ungrounded merge candidate** ("fabricated
evidence") while keeping the two grounded drafts — the no-fakes discipline working.

## Phase-1 design input (carry forward)

- **Topic coverage matters.** django seeds clustered on migrations; sympy spread wider (slots /
  symbolic / codegen). A random seed/test split risks diluting the DiD if seed skills are topic-narrow
  relative to the test set. Phase 1 should either (a) pick the broadly-reusable convention skills
  (test-running, assertion idioms) as the compounding lever, or (b) stratify/topic-match the
  seed↔test split, and pre-register which.
- Powered repo = **django** (N=114 power + archetypal recurring-procedure skill); sympy is a viable
  alternative (higher drafts/solve, more diverse) and is already the natural CTRL foreign-seed corpus.

## Artifacts (kept)

- Drafts: `/tmp/swebench-phase0-{django,sympy}/.skills/*/SKILL.md.pending` (8 total).
- Solve logs + transcripts: `/tmp/swebench-phase0-*/solve-*.log`, `~/.claude/projects/-tmp-swebench-phase0-*/`.
- Extraction logs: `/tmp/swebench-phase0-*/extract-*.log`, `.../logs/worker-s*.log`.
- Scripts: `scripts/t15_phase0_{verbose_retrieval_probe.py,seed_extract.py,solve.sh}`.
- Corpus snapshot (safety net): `/tmp/t15-pre-phase0-skill_layer_test.sql`.
