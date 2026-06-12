---
source_type: ticket-index
plan_file: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
ticket_index: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/20-institutionalize-262-instrument-e2e-gate.md
tickets_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/index.md
source_packet_ref: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/20-institutionalize-262-instrument-e2e-gate.md
brainstorm_ref: none
started: 2026-06-12T07:56:27
status: completed
execution_shape: vertical-slices
current_unit: 1
total_units: 2
session_id: work-2026-06-12-075627-T20
---

## WHY Linkage
- Canonical WHY source: docs/plans/2026-06-08-feat-v1-7-local-hybrid-skilldag-retrieval-plan.md
- Parent plan: same.
- This execution serves: ONE validated ruler (the T11 262 instrument) wired into the automated gate, so future corpus/fixture drift is caught by CI, not by the next assessment. T12/T14/T18 lean on this instrument; it must be the one CI sees.
- Success-criteria focus: 262 fixture is the gate; α=0 canary craters live; candidate-recall asserted; scripts promoted to ticket-agnostic home; latency artifact persisted; T11 report erratum appended; stale ruler retired loudly.

### Owner decision (2026-06-12)
- **262 gate mechanism = PROMOTE THE PYTHON INSTRUMENT AS THE GATE** (option A). The validated T11 instrument is `scripts/t11_*` driving the real mcp-server over HTTP on the live 262 corpus by anchor — that IS "measurement drives the real app". The Rust `test_retrieval_quality.rs` is a DIFFERENT instrument (seeds a synthetic corpus, measures compile_context); it cannot consume the anchor/strata/split 262 fixture. So: promote the Python instrument into a `--gate` mode + a thin Rust `#[ignore]` shell test; retire the stale fixture AND the superseded synthetic-seed Rust quality tests loudly. One validated ruler, no re-validation of a second 262 implementation.

### TDD Contract
- Effective mode: Ralph-driven, instrument/gate variant.
- Loop: RED (gate not wired; stale falsified ruler is what CI sees; 2/4 quality tests fail on stale fixture) → promote+wire the validated instrument → GREEN (`--gate` passes on live 262; α=0 craters live; `--self-test` green) → Post-Refactor Green (re-run self-test + gate after cleanup).
- Required evidence: `--self-test` of the promoted metrics (offline, deterministic); LIVE `--gate` exit 0 on the 262 fixture against the real server; LIVE α=0 canary crater (≥50% rel MRR drop); persisted raw latency artifact.
- Exceptions: the gate is anchor-only/deterministic (no LLM judge) for CI reproducibility — the judge-aug frozen 0.80/0.80/0.90 stays a T11 report claim, not a CI assertion (LLM-nondeterministic). Justified: a CI gate must be deterministic; anchor-only MRR/cand-recall/nDCG with the α=0 canary is the reproducible alignment ruler.

### Constitution Context
- STANDING RULE: measurement drives the REAL running mcp-server over HTTP end-to-end; NO in-process reconstruction. The promoted instrument already honors this.
- No-fakes: gate thresholds derive from T11-MEASURED numbers with recorded margins, never threshold-gamed to whatever the suite produces. Stale fixture retired loudly (no second silent load path).
- No production crate changes (harness/fixtures/scripts/docs only).

### Architecture Handoff
- Artifact: T11 instrument + report (tests/e2e/reports/t11/T11-VALIDATION-REPORT.md).
- T11-measured anchor-only numbers (dense_views-ON, the as-shipped default per 7fe8912): MRR@3 0.743, MRR@10 0.743, nDCG@3 0.755, hit@3 0.788, candidate-recall@50 0.796, no_match 0.92. Single-view dense (flag OFF) floor: 0.686/0.696/0.723. α=0 control: 0.000 (100% crater).
- Gate floors must sit BELOW the measured numbers (margins recorded) so real regressions fire and noise does not; robust to the dense_views flag state (set below the dense single-view numbers).

## Work Status
| # | Unit | Kind | Serves / Unlocks | Status | Attempts | Session File |
|---|------|------|------------------|--------|----------|--------------|
| 1 | Promote instrument + `--gate` + Rust shim + retire stale ruler + erratum (offline) | infra-packet | the validated ruler becomes the gate; T12/T14/T18 import the shared lib | completed | 1 | unit-01-promote-instrument-gate.md |
| 2 | Live gate run + α=0 canary crater + latency artifact (orchestrator-driven, real server) | infra-packet | GREEN evidence: the gate passes on the live 262 stack; canary craters | completed | 1 | unit-02-live-gate-run.md |

## GREEN evidence (live gate run t20-gate-20260612-081155, 2026-06-12)
- **GATE: PASS** — all 6 floor assertions green. dense_views_on: MRR@3 0.743 / MRR@10 0.743 / nDCG@3 0.755 / cand-recall@50 0.796 / no_match 0.92 (reproduces T11 §2 EXACTLY). alpha0_control: 0.000 across the board → 100% MRR crater (canary ≥50%).
- Real measured latency persisted (`latency_t20-gate-20260612-081155.json`, 137 queries): mean 282.7ms, p50 266.4ms, **p95 375.3ms** < 500ms SLO (sources the T11 §3 369ms claim). The script's original placeholder latency note was replaced with genuine wall-clock timing (no-fakes rule).
- Live stack restored to default env (RETRIEVAL_ALPHA/DENSE_VIEWS unset) + /health 200 + real query verified post-run (`prohibit-concurrent-cargo-ops-across-agents` 0.749). The α=0 arm leaves the server crippled if not restored — orchestrator restore step is mandatory.

## RED baseline (captured by orchestrator 2026-06-12)
- The validated 262 instrument lives only in `scripts/t11_*` — nothing gates on it.
- The automated e2e quality gate (`tests/e2e/quality/labeled_corpus.rs:82` → `test_retrieval_quality.rs`) loads the STALE fixture `retrieval_quality_labeled.json` (synthetic skills); its 2 live tests (`retrieval_quality_meets_thresholds_on_live_stack`, `semantic_retrieval_beats_lexical_baseline_on_disjoint`) FAILED 2/4 in the 2026-06-11 run (corpus drift). Two rulers: a validated one nothing gates on, a falsified one wired in.
- Live stack: UP 9h healthy, mcp-server :3001, collection skills__qwen3-embedding-4b, graph_version 16, RETRIEVAL_DENSE_VIEWS unset (compiled default).

## Learnings Brief
_No learnings yet._
