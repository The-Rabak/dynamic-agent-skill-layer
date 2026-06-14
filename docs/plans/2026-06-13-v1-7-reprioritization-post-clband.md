# V1.7 reprioritization — post-CL-band strategic pivot (2026-06-13)

**Owner decision (2026-06-13, after the T23 band returned INSTRUMENT-FAILURE):** CL-bench was the
wrong *primary* efficacy gate. Re-center the remaining V1.7 work on what real usage actually
exercises — **compounding skill retrieval into real sessions through the production priming path** —
and demote CL-bench to one optional adversarial stressor. This doc records the re-weighting of every
open ticket and the reasoning, so the index frontmatter `repriority_note` has a full backing.

## Why CL-bench is the wrong primary gate

1. **Regime mismatch.** CL-bench is purpose-built around an injected reference document, a single
   teach→test cycle, and an answer that hinges on one specific *buried* value the model couldn't
   pretrain. Real taught knowledge in coding is smaller, *operative* (you state a convention because
   you're about to apply it, so it lands in the session and gets captured), and **compounding** (a
   value that matters gets operationalized in some later session and captured then). CL-bench is the
   adversarial worst case for a compounding system, not the common case.
2. **Root cause, verified in the band drafts.** Extraction is session-distillation: it preserves the
   literals the capturing session *operationalized* and drops the ones it merely *referenced*.
   material-handler kept `50 lb (Rule 7)` verbatim (the teach solve did weight arithmetic) but
   dropped `<1 megaohm` (a checkbox the solve never computed with) — zero occurrences across 25
   drafts. That is "distill the session" working as designed, not a tunable bug; a prompt instruction
   to keep specifics (T22's abstraction-exception clause) was active and still didn't save it, because
   the loss is at *selection*, upstream of placeholdering.
3. **Structurally unwinnable as built.** The per-context pipeline is a serial AND of four gates
   (OFF-discriminates · teach-completes · extraction-fidelity · solve-completes). With fidelity
   passing ~25% of contexts under a "need ≥7/10" bar, the expected clean N was ~0–2. INSTRUMENT-FAILURE
   was the arithmetic, not bad luck.
4. **The same logic re-weights the benchmark, not just the fix.** If document-grounded one-shot
   teaching isn't the real use case, a benchmark whose central difficulty *is* document-grounded
   one-shot teaching shouldn't be the gate. The risk to avoid is the worst-of-both: de-prioritize the
   doc-fidelity fix *and* keep chasing the benchmark that needs it.

## The reframed thesis and the honest critical path

The product thesis is **"the layer compounds: it learns from real sessions and makes future sessions
measurably better."** What serves that, in dependency order:

- **The production priming path must actually work.** `compile_context` (the SessionStart injection
  surface) returns `no_match` for realistic verbose prompts. Every efficacy run so far has *worked
  around* this with focused inject-query mode rather than measuring the real path. So the honest
  critical path is: **instrument the real priming path (T18) → fix it (T12) → measure compounding
  through the fixed path (T15).** Measuring efficacy through a broken priming path would repeat the
  CL band's core mistake.
- **Extraction should keep contract-bearing literals** (enums, status/error codes, thresholds, pins)
  — a *general* quality win for real code, sized as a lightweight additional step (T24), explicitly
  not a document-ingestion mode and not the focus.
- **CL-bench survives as a clean optional stressor** (T25), not the gate.

## Re-weighted open tickets (new batch order)

| Batch | Ticket | Why here |
|---|---|---|
| 19 | **T18** priming instrument | READY now (T10/T11/T20 done). The measuring stick for the real injection path, incl. the verbose `no_match` substratum. First because it's ready and it gates the highest-value fix. |
| 20 | **T12** priming mechanism fix | The production `compile_context` verbose-prompt fix IS the top real-usage retrieval fix. Blocks T15. "After T14 for attribution" is discharged (band gave ~no attribution). |
| 21 | **T24** extraction literal-retention (NEW) | Lightweight, general; keep operative literals verbatim in selected skills. Pre-T15 synergy (better corpus), but not blocking. Not the focus. |
| 22 | **T15** SWE-bench compounding | PROMOTED to the **primary efficacy gate**. Measures compounding on a realistic code-task distribution *through the fixed priming path* (new hard dep on T12) + #217. |
| 23 | **T25** CL → clean secondary stressor (NEW) | LOW priority / conditional. Verifier-based fidelity gate (recovers quartermaster), task-design fixes, placebo robustness, circuit breaker. Only when a CL re-run is wanted (T15's optional arm). |
| 24 | **T16** maintenance robustness | Independent hardening, ready, parallel-safe; low strategic weight; opportunistic win. |
| — | **T19** cross-project recurrence | Still deferred (needs ≥2 project corpora). |

T14 stays the efficacy-chapter owner; its **instrument of record is now T15**, and the CL band (T23)
is recorded as INSTRUMENT-FAILURE with the verdict reassigned, not spun.

## Where we go from here, in one line

**Next action = T18** (ready): author the session-start stratum with the verbose-opening substratum,
pre-register the priming metrics + negative control, and measure the baseline prime *through
`compile_context`* — quantifying the real `no_match` failure. That hands T12 an honest before-number
for the fix that finally makes the production priming path measurable, which is the prerequisite for
T15 to answer the efficacy question on the distribution we actually care about.

## What did NOT change (guardrails intact)

Pre-registration discipline, the untouchable production human gate, measurement-drives-the-real-server,
no-fakes/fail-loud, heavy-action serialization, the pristine 262 dogfood corpus. The pivot is about
*what we measure and in what order*, not a loosening of how we measure.
