---
unit: "Unit B — Harness document delivery"
unit_number: 2
unit_kind: infra-packet
serves: "Make the knowledge document verifiably reach the prose-extraction windows (no extractor change)"
status: completed
attempt_count: 1
domains: [extraction-harness, clband, python]
ticket_file: docs/tickets/2026-06-08-v1-7-local-hybrid-skilldag-retrieval/22-teach-path-extraction.md
session_id: work-2026-06-12-t22-teach-path
---

## What Was Implemented
`tests/e2e/efficacy/clband/teach_delivery.py` — `materialize(context, raw_jsonl)` prepends the
context's knowledge document (flywheel→system.md, aether→context.md, matching setup_teach_workspaces)
as a leading **user** turn so the prose extractor (which keeps only user+assistant message text) sees
the verbatim rules. Wired into `clband_extract.py` before ingest (toggle CLBAND_TEACH_DELIVERY,
default on; no-op for unknown contexts). Unit tests: `test_teach_delivery.py` (4/4).

## Files Changed
- `tests/e2e/efficacy/clband/teach_delivery.py` — created
- `tests/e2e/efficacy/clband/test_teach_delivery.py` — created
- `tests/e2e/efficacy/clband/clband_extract.py` — wire materialize() before ingest
- `tests/e2e/reports/efficacy/clband-smoke/visibility/{*-materialized.json, materialized/*.jsonl}`

## Fences honored
- Does NOT weaken the suspicious-speaker injection filter: doc delivered as ordinary role:"user"
  content, fenced in <transcript> like all transcript data (test_delivery_does_not_use_system_speaker).
- No extractor changes (harness-side only).

## Evidence (deterministic replay; no live claude burn)
Visibility map re-run on materialized transcripts:
- flywheel: doc-tier 3/4→4/4, operative 8/9→9/9 (prose-visible 6266→10668 chars)
- aether: doc-tier 2/4→3/4, operative 4/8→6/8 (prose-visible 1338→35106 chars); Cause + outer now visible.
- Remaining aether invisibles (Turbulence Alert, Corrected Code) are full=false (different sibling) →
  Unit D operative tier must be derived from the TAUGHT sibling (translate-spec rules).

## Test Results
- `python3 tests/e2e/efficacy/clband/test_teach_delivery.py` → 4/4 PASS
- Visibility re-map → document text demonstrably reaches windows. Attempts: 1
