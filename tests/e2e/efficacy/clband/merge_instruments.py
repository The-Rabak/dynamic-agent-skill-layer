#!/usr/bin/env python3
"""T23 Unit A→B bridge: orchestrator-owned merge + INDEPENDENT instrument verification gate.

The 8 per-context Unit-A agents each authored `instruments/<name>.json` (operative + document
sentinels, measured siblings) and self-tested their verifier. This script is the AUTHORITATIVE gate
the orchestrator runs before the band: it does NOT trust the agents' self-reports. For every full
context it:

  1. RE-VERIFIES every operative sentinel appears VERBATIM (case-insensitive substring) in the COMMON
     context text (union of contexts/<name>/system.md + context.md). A sentinel not present is
     HALLUCINATED (the M-WARN-01 / LMI-2025 lesson) — FAIL LOUD.
  2. RE-RUNS the verifier on its good/bad fixtures and asserts good→exit 0, bad→exit non-zero (the
     Ralph RED/GREEN, independently re-checked).
  3. Confirms every measured sibling's task spec + judge prompt + the teach workspace exist.
  4. MERGES sentinels_operative + sentinels_document into manifest.json so fidelity_gate.sh reads
     them by the context `short`.

Exit 0 only if EVERY full context passes. Exit non-zero (and write nothing to the manifest) otherwise.

Usage: merge_instruments.py [--write]    (default: verify only; --write updates manifest.json)
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

CLBAND = Path(__file__).resolve().parent
MANIFEST_PATH = CLBAND / "manifest.json"


def common_context_text(name: str) -> str:
    """The COMMON context text a sentinel must appear in: union of system.md + context.md, lowercased."""
    parts = []
    for fn in ("system.md", "context.md"):
        p = CLBAND / "contexts" / name / fn
        if p.exists():
            parts.append(p.read_text(errors="replace"))
    return "\n".join(parts).lower()


def verify_context(name: str, manifest_shorts: dict) -> tuple[bool, list[str], dict]:
    """Run all four checks for one full context. Returns (ok, problems, instr_dict)."""
    problems: list[str] = []
    meta_path = CLBAND / "instruments" / f"{name}.json"
    if not meta_path.exists():
        return False, [f"missing instruments/{name}.json"], {}
    instr = json.loads(meta_path.read_text())

    # 1. operative sentinels present verbatim in the common context text
    text = common_context_text(name)
    operative = instr.get("sentinels_operative", [])
    if not operative:
        problems.append("no sentinels_operative authored")
    for s in operative:
        if s.lower() not in text:
            problems.append(f"HALLUCINATED operative sentinel (not in context text): {s!r}")

    # 2. verifier good/bad fixtures
    verifier = CLBAND / "verifiers" / f"{name}.sh"
    good = CLBAND / "fixtures" / f"{name}-good"
    bad = CLBAND / "fixtures" / f"{name}-bad"
    if not verifier.exists():
        problems.append(f"missing verifier verifiers/{name}.sh")
    else:
        for fx, want_zero in ((good, True), (bad, False)):
            if not fx.exists():
                problems.append(f"missing fixture {fx.name}")
                continue
            rc = subprocess.run(["bash", str(verifier), str(fx)], capture_output=True, text=True).returncode
            if want_zero and rc != 0:
                problems.append(f"verifier should PASS good fixture but exit={rc}")
            if not want_zero and rc == 0:
                problems.append(f"verifier should FAIL bad fixture but exit=0 (non-discriminating)")

    # 3. measured-sibling task specs + judge prompts + ≥5 verifier checks + teach workspace
    sibs = instr.get("measured_siblings", [])
    if not sibs:
        problems.append("no measured_siblings authored")
    for sib in sibs:
        slug = sib.get("slug", "")
        if not (CLBAND / "tasks" / f"{slug}.json").exists():
            problems.append(f"missing tasks/{slug}.json")
        if not (CLBAND / "judge" / f"{slug}.md").exists():
            problems.append(f"missing judge/{slug}.md")
    if verifier.exists():
        # crude ">=5 deterministic checks" proxy: count fail()/grep checks
        body = verifier.read_text()
        n_fail = body.count("fail ") + body.count("fail(")
        if n_fail < 5:
            problems.append(f"verifier has <5 deterministic checks (found ~{n_fail} fail-points)")
    if not (CLBAND / "teach" / name / "prompt.txt").exists():
        problems.append(f"missing teach/{name}/prompt.txt")

    # 4. short matches the manifest
    short = instr.get("short")
    if short and manifest_shorts.get(name) and short != manifest_shorts[name]:
        problems.append(f"instruments short {short} != manifest short {manifest_shorts[name]}")

    return (len(problems) == 0), problems, instr


def main() -> int:
    write = "--write" in sys.argv[1:]
    manifest = json.loads(MANIFEST_PATH.read_text())
    by_name = {c["name"]: c for c in manifest["contexts"]}
    shorts = {n: c["short"] for n, c in by_name.items()}
    full = [c["name"] for c in manifest["contexts"] if c["role"] == "full"]

    print(f"=== merge_instruments: verifying {len(full)} full contexts (write={write}) ===")
    all_ok = True
    merged = 0
    for name in full:
        ok, problems, instr = verify_context(name, shorts)
        status = "OK" if ok else "FAIL"
        n_op = len(instr.get("sentinels_operative", []))
        n_sib = len(instr.get("measured_siblings", []))
        print(f"  [{status}] {name}: operative_sentinels={n_op} siblings={n_sib}")
        for p in problems:
            print(f"        - {p}")
        if not ok:
            all_ok = False
            continue
        # stage the merge into the manifest entry
        if write:
            entry = by_name[name]
            entry["sentinels_operative"] = instr["sentinels_operative"]
            entry["sentinels_document"] = instr.get("sentinels_document", [])
            entry["_clband_instruments"] = {
                "doc_file": instr.get("doc_file"),
                "teach_sibling_id": instr.get("teach_sibling_id"),
                "measured_siblings": [s["slug"] for s in instr.get("measured_siblings", [])],
                "verifier": f"verifiers/{name}.sh",
            }
            merged += 1

    if not all_ok:
        print("\n*** GATE FAILED — manifest NOT written. Fix the offending instruments first. ***")
        return 1
    if write:
        MANIFEST_PATH.write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"\nmanifest.json updated: merged sentinels for {merged} full contexts.")
    else:
        print("\nAll full contexts pass. Re-run with --write to merge sentinels into manifest.json.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
