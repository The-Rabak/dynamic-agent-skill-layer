#!/usr/bin/env python3
"""Apply confidence-gated, grounded multi-view fields onto the live target SKILL.md files.

INPUT: /tmp/backfill_match_candidates.json — the GATED proposals from
backfill_match.py (target -> {file, from_draft, cosine, fields}), each already
verified same-skill (cosine + mutual-best + margin + same-source) and, for the
accepted set, eyeball-confirmed by the orchestrator.

RULES (honesty — machine-wide no-fakes mandate):
  - Set a field ONLY when the LIVE skill currently lacks it (empty) AND the proposal
    carries a NON-EMPTY grounded value. Never overwrite existing fields.
  - Only use_when / requires / avoid_when / invariants (e_task/e_needs/e_negative inputs).
  - Back up each live SKILL.md before editing. Dry-run unless --apply.

Usage: scripts/backfill_apply.py [--apply]
"""
import json
import shutil
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
FIELDS = ["use_when", "requires", "avoid_when", "invariants"]


def split_fm(text):
    if text.startswith("---\n") and "\n---\n" in text[4:]:
        raw, body = text[4:].split("\n---\n", 1)
        return yaml.safe_load(raw) or {}, body
    return None, None


def dump_fm(fm, body):
    return "---\n" + yaml.safe_dump(fm, sort_keys=False, allow_unicode=True, default_flow_style=False) + "---\n" + body


def main():
    apply = "--apply" in sys.argv
    proposals = json.load(open("/tmp/backfill_match_candidates.json"))
    backup_dir = ROOT / "tests/e2e/reports/retrieval/backfill_skillmd_backups"
    ledger = {"backfilled": [], "skipped_field_present": [], "no_grounded_field": [], "no_file": []}

    for target, p in proposals.items():
        live = ROOT / p["file"]
        if not live.exists():
            ledger["no_file"].append(target)
            continue
        fm, body = split_fm(live.read_text())
        if fm is None:
            ledger["no_file"].append(f"{target} (unparseable)")
            continue
        staged, present = {}, []
        for f in FIELDS:
            proposed = p["fields"].get(f) or []
            if not proposed:
                continue
            if fm.get(f):  # live already has it — never overwrite
                present.append(f)
                continue
            staged[f] = proposed
        if not staged:
            (ledger["skipped_field_present"] if present else ledger["no_grounded_field"]).append(target)
            continue
        ledger["backfilled"].append(dict(target=target, from_draft=p["from_draft"], cosine=p["cosine"],
                                         fields={k: v for k, v in staged.items()},
                                         file=p["file"]))
        if apply:
            backup_dir.mkdir(parents=True, exist_ok=True)
            shutil.copy2(live, backup_dir / f"{target}__SKILL.md.bak")
            for f, v in staged.items():
                fm[f] = v
            live.write_text(dump_fm(fm, body))

    print(f"\n=== backfill apply ({'APPLIED' if apply else 'DRY-RUN'}) ===")
    print(f"backfilled            : {len(ledger['backfilled'])}")
    for b in ledger["backfilled"]:
        flds = ", ".join(f"{k}(+{len(v)})" for k, v in b["fields"].items())
        print(f"   {b['target']:<46} cos={b['cosine']}  {flds}   <- {b['from_draft']}")
    print(f"skipped_field_present : {len(ledger['skipped_field_present'])}")
    print(f"no_grounded_field     : {len(ledger['no_grounded_field'])}")
    if ledger["no_file"]:
        print(f"no_file               : {ledger['no_file']}")
    out = ROOT / f"tests/e2e/reports/retrieval/backfill_apply_ledger{'_applied' if apply else '_dryrun'}.json"
    out.write_text(json.dumps(ledger, indent=2))
    print(f"ledger: {out}")


if __name__ == "__main__":
    main()
