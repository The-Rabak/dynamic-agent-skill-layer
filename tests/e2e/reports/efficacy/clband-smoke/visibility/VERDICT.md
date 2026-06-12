# T22 Unit A — extraction-input visibility map (forensics, no fixes)

**Method.** A diagnostic example (`crates/session-extractor/examples/clband_visibility_map.rs`) drives
the REAL pipeline functions end-to-end over the two CAPTURED smoke transcripts
(`tests/e2e/reports/efficacy/clband-smoke/transcripts/*.jsonl`):
`parse_session_events` → `mine_preamble` → `segment_session` → `events_to_transcript` →
`render_sanitized_transcript_lines`. What it reports is byte-for-byte what the orchestrated prose
extractor receives per window. Raw artifacts: `flywheel-visibility.json`, `aether-visibility.json`.

Each sentinel is checked for presence in (a) the **full-session haystack** = every event's
`grounding_text()` (INCLUDES `ToolResult.output` and `ToolCall` input), vs (b) the **prose-visible
text** = exactly what `events_to_transcript` + the sanitizer keep (UserMessage + AssistantMessage).
A sentinel present in (a) but not (b) is **invisible** to the prose extractor.

## Headline: the addendum's "flywheel document never seen" hypothesis is REFUTED for flywheel and CONFIRMED for aether

| context | events | windows | event types | full haystack | prose-visible | doc-tier visible | operative-tier visible |
|---|---|---|---|---|---|---|---|
| flywheel | 20 | 1 | 1 user / 3 asst / 2 toolcall / 2 toolresult / 1 fileedit / 11 meta | 15 224 ch | 6 266 ch | **3/4** | **8/9** |
| aether | 20 | 1 | 1 user / 3 asst / 2 toolcall / 2 toolresult / 1 fileedit / 11 meta | 38 826 ch | **1 338 ch** | 2/4 | 4/8 |

### Three separable losses, now measured

1. **LOSS 1 — document invisibility (the real plumbing fact).** `SessionEvent::as_transcript_entry()`
   returns `None` for `ToolResult`/`ToolCall`/`FileEdit` (`crates/domain/src/types.rs:420`), so the
   prose extractor's flat transcript only ever contains user + assistant **message** text. Content
   that the agent **Read** (the spec/SOP → `ToolResult`) or **Wrote** (the answer → `FileEdit`) is
   discarded before extraction.
   - **flywheel:** mostly NOT lost — the agent *narrated* its applied rules in assistant prose, so
     8/9 operative rules (`next size up`, `firm shake`, `retest`, `spin test`, `Validation Engineer`,
     `Agent C`, `Forklift`, `Agent D`) were visible. Only `extra torque` and the persona name
     `Scatterbrained Improviser` were lost. **Flywheel's failure is NOT primarily a visibility problem.**
   - **aether:** SEVERELY lost — prose-visible is only **1 338 chars** of a 38 826-char session. The
     agent read the spec (ToolResult, invisible) and wrote its answer to `solution.md` (FileEdit,
     invisible). Spec-operative tokens `outer` and `Cause` are present in the haystack but **invisible**
     to the prose extractor. **Aether's failure IS substantially a visibility problem.**

2. **LOSS 2 — preamble eaten (real bug, but NOT causal here).** The orchestrator prepends the mined
   preamble as `speaker:"system"` (`orchestrator.rs:703`); the suspicious-speaker injection filter
   (`prompt_contract.rs:252`) drops every `*system*` speaker → the preamble is dropped on **1/1**
   windows of **both** contexts. This is the literal worker-log line
   (`transcript entry dropped: ... system impersonation`). **However**, the mined preamble carries
   **zero** document/operative sentinels in both contexts (`carries_*_sentinel=false`), so this drop
   did **not** cause the sentinel loss. It is a genuine defect — the orchestrator's own trusted
   preamble is silently 100% filtered, so the "carry global facts into every window" feature is dead
   weight — but it is a **side-finding**, filed as a cleanup, not on the critical path to GO.

3. **WORLDVIEW (the dominant cause, esp. flywheel).** The prose extractor *saw* 8/9 flywheel operative
   rules and still returned **0 candidates 3× with reasoned refusals** ("every 'lesson' is explicitly
   embedded in the task instructions… nothing durable", verbatim in
   `logs/worker-flywheel-assembly-agent-200451-s1.log`). The capture that survived (11 drafts) came
   via the preference/convention channel, not the prose channel. Visibility was adequate; the
   extractor's value system (durable/reusable/"future, different task" + the verbatim-literals
   anti-pattern) rejected one-shot taught rules. **This is the core T22 Unit C gap.**

## Apportionment (the point of Unit A)

| failure component | flywheel | aether | owning unit |
|---|---|---|---|
| document reaches prose extractor | mostly yes (8/9 op) | mostly no (4/8 op) | **Unit B** (delivery) — load-bearing for aether |
| extractor *willing* to capture taught rules verbatim | **NO** | **NO** | **Unit C** (worldview + retry) — load-bearing for both |
| preamble survives sanitizer | no (but carries nothing) | no (but carries nothing) | cleanup note (off critical path) |

**Verdict:** The path to GO needs BOTH (B) document delivery — decisive for aether, where the spec
and the answer are lost in tool/file events — AND (C) the taught-knowledge worldview fix — decisive
for both, and the *sole* remaining blocker for flywheel, whose rules were already visible. This does
**not** trigger decision-point #4 (it is not "Unit A refutes visibility AND C-alone still fails"):
the data assigns clear, independent work to B and C, both product-justified.

## Unit B — document delivery applied (replay proof, deterministic)

The harness (`tests/e2e/efficacy/clband/teach_delivery.py`) prepends the knowledge document as a
leading **user** turn before ingest (no extractor change, no filter weakening). Re-running the
visibility map on the materialized (replayed) transcripts:

| context | doc-tier visible (raw → materialized) | operative-tier visible (raw → materialized) | prose-visible chars |
|---|---|---|---|
| flywheel | 3/4 → **4/4** | 8/9 → **9/9** | 6 266 → 10 668 |
| aether | 2/4 → **3/4** | 4/8 → **6/8** | 1 338 → **35 106** |

`Cause` and `outer` (aether spec-operative tokens previously lost in `ToolResult`) are now visible.
The two remaining aether invisibles (`Turbulence Alert`, `Corrected Code`) are `full=false` — absent
from the session entirely because the aether teach session is the **translate** sibling, not the
turbulence-review sibling. → Unit D must derive the operative sentinel tier from the sibling actually
taught (translate-spec rules), not from a different sibling's verifier. Raw artifacts:
`{flywheel,aether}-visibility-materialized.json`, `materialized/*.jsonl`.

**Unit B AC met:** re-extracted (replayed) window contents demonstrably contain the document text;
delivery is fixed harness-side without touching the injection defense. Unit tests:
`test_teach_delivery.py` 4/4 green.
