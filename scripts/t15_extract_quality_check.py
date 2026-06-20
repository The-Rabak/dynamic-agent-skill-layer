#!/usr/bin/env python3
"""T15 — extraction-quality pre-check (no solves; reuses existing OFF transcripts).

The self-seed smoke (2026-06-20) showed a feature-add solve session extracted a
USELESS `preference` skill echoing the problem statement, not the fix. Before
spending the full N=10 self-seed run, this checks the extraction-quality
DISTRIBUTION cheaply: ingest the already-on-disk probe OFF transcripts into a
throwaway scope, drain via the real frontier worker, and report each extracted
skill's type + name so we can judge whether the loop will have useful skills to
inject. No fakes — real worker, real isolated DB, dogfood untouched.

Usage: t15_extract_quality_check.py --solve-dirs <iid__off>,<iid__off>,...
"""
import argparse
import json
import sys
from pathlib import Path

_SCRIPTS_DIR = Path(__file__).parent.resolve()
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

import t15_selfseed_loop as loop      # noqa: E402  — reuse ingest/drain/gate
import t15_swebench_seed as seedmod   # noqa: E402  — psql

SOLVE_ROOT = Path("/tmp/t15-swebench/solve")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--run-id", default="qualcheck")
    ap.add_argument("--scope-name", default="swebench-django-qualcheck")
    ap.add_argument("--solve-dirs", required=True,
                    help="comma list of <iid>__off solve dirs under /tmp/t15-swebench/solve")
    ap.add_argument("--drain-timeout", type=int, default=3600)
    args = ap.parse_args()

    scope_dir = loop.PROJECT_ROOT / args.scope_name
    (scope_dir / ".skills").mkdir(parents=True, exist_ok=True)
    (scope_dir / "global").mkdir(parents=True, exist_ok=True)
    log_dir = seedmod.REPO_ROOT / "logs/t15-selfseed" / args.run_id
    log_dir.mkdir(parents=True, exist_ok=True)

    dirs = [d.strip() for d in args.solve_dirs.split(",") if d.strip()]
    print(f"=== extraction quality check ({args.run_id}) — {len(dirs)} transcripts ===", flush=True)
    ingested = 0
    for d in dirs:
        ws = SOLVE_ROOT / d
        t = loop.locate_transcript(ws)
        if not t:
            print(f"  [skip] {d}: no transcript on disk", flush=True)
            continue
        iid = d[:-5] if d.endswith("__off") else d
        status = loop.ingest_transcript(f"{args.run_id}-off-{iid}", t.read_text(errors="replace"), scope_dir)
        ingested += 1
        print(f"  [ingest] {d} ({t.stat().st_size}B) -> {status}", flush=True)

    drain = loop.drain_until_empty(scope_dir, log_dir, args.drain_timeout)
    gated = loop.gate_drafts(scope_dir)
    print(f"\n=== extracted {len(gated)} skill(s) from {ingested} transcript(s) "
          f"({drain['passes']} drain pass(es)) ===", flush=True)
    skills_dir = scope_dir / ".skills"
    rows = []
    for g in gated:
        skill_md = skills_dir / g["dir"] / "SKILL.md"
        stype = ""
        for line in skill_md.read_text(errors="replace").splitlines():
            s = line.strip()
            if s.startswith("type:"):
                stype = s[len("type:"):].strip()
                break
        rows.append({"name": g["name"], "type": stype, "source": g["source_session_id"]})
        print(f"  • [{stype or '?':14s}] {g['name']}  (from {g['source_session_id']})", flush=True)
    # type histogram — preference/convention = low value here; failure_fix/best_practice/diagnostic = useful
    hist: dict[str, int] = {}
    for r in rows:
        hist[r["type"]] = hist.get(r["type"], 0) + 1
    print(f"\ntype histogram: {hist}", flush=True)
    useful = sum(v for k, v in hist.items() if k in ("failure_fix", "best_practice", "diagnostic", "anti_pattern"))
    print(f"useful (fix/best_practice/diagnostic/anti_pattern): {useful}/{len(rows)}", flush=True)
    out = seedmod.REPO_ROOT / "tests/e2e/reports/swebench" / f"qualcheck_{args.run_id}.json"
    out.write_text(json.dumps({"scope": str(scope_dir), "ingested": ingested,
                               "skills": rows, "type_histogram": hist, "useful": useful}, indent=2) + "\n")
    print(f"[report] {out}", flush=True)


if __name__ == "__main__":
    main()
