#!/usr/bin/env python3
"""Draft-acceptance-rate scorer for real captured .pending skill drafts.

WHY this module exists
----------------------
T14 AC: measure the rate at which human reviewers accept skill drafts extracted
from real agent sessions.  "Accepted" means the draft was renamed from
``SKILL.md.pending`` → ``SKILL.md`` by a human (not automated) — the only
signal that indicates the draft was judged useful enough to ship.

This is a SEPARATE metric from the A/B task-outcome run, reported alongside it
in the T14 efficacy package.

Standing rules (from CONTRACT.md and project memory):
  - Synthetic drafts are rejected.  A "real" draft is any .pending file from a
    genuine captured session (not hand-written for testing).
  - The scorer FAILS LOUD (exit non-zero) if handed fewer than 10 real drafts.
    Reporting a rate on <10 items would be statistically meaningless and risks
    producing a fake-looking "result." It is better to fail loudly.
  - Acceptance is determined structurally: a .pending file is "accepted" iff the
    sibling .md file (same stem, no .pending extension) exists in the same directory.

Usage
-----
  # Check a directory of .pending files:
  python3 scripts/efficacy_draft_acceptance.py <dir>

  # Check specific files:
  python3 scripts/efficacy_draft_acceptance.py <file1.pending> <file2.pending> ...

  # Run self-tests:
  python3 scripts/efficacy_draft_acceptance.py --self-test
"""
import argparse
import sys
from pathlib import Path


# Minimum number of real .pending drafts required to compute a rate.
# Fewer than this is statistically meaningless; fail loud instead of fabricating.
MIN_DRAFTS_REQUIRED = 10


def compute_draft_acceptance_rate(pending_files: list[Path]) -> dict:
    """Compute the accepted/total rate over real .pending skill drafts.

    "Accepted" means the sibling .md file (same stem without .pending extension)
    exists in the same directory as the .pending file.  This is the only signal
    that a human reviewer judged the draft good enough to ship.

    FAILS LOUD: exits non-zero if fewer than MIN_DRAFTS_REQUIRED (10) real
    .pending files are provided.  This prevents reporting a fake-looking rate on
    a statistically insufficient sample.

    Args:
        pending_files: list of Path objects pointing to .pending draft files.

    Returns:
        Dict with keys:
          total          — total .pending files examined
          accepted       — count whose sibling .md exists (human-accepted)
          rejected       — total - accepted
          accepted_rate  — float in [0.0, 1.0]
          per_file       — list of per-file dicts (path, accepted bool)
    """
    if len(pending_files) < MIN_DRAFTS_REQUIRED:
        print(
            f"ERROR: draft-acceptance scorer requires at least {MIN_DRAFTS_REQUIRED} real "
            f".pending drafts; got {len(pending_files)}. "
            f"Reporting a rate on fewer items is statistically meaningless. "
            f"Source more real drafts from genuine captured sessions.",
            file=sys.stderr,
        )
        sys.exit(1)

    per_file: list[dict] = []
    accepted_count = 0

    for pending_path in pending_files:
        # Derive the sibling .md path: remove the .pending suffix.
        # Example: SKILL.md.pending → SKILL.md (in the same directory).
        stem_without_pending = pending_path.name
        if stem_without_pending.endswith(".pending"):
            stem_without_pending = stem_without_pending[: -len(".pending")]
        sibling_md = pending_path.parent / stem_without_pending
        is_accepted = sibling_md.exists()
        if is_accepted:
            accepted_count += 1
        per_file.append({
            "path": str(pending_path),
            "accepted": is_accepted,
            "sibling_md": str(sibling_md),
        })

    total = len(pending_files)
    return {
        "total": total,
        "accepted": accepted_count,
        "rejected": total - accepted_count,
        "accepted_rate": accepted_count / total,
        "per_file": per_file,
    }


def _self_test() -> int:
    """Run module self-tests.  Returns 0 if all pass, 1 if any fail."""
    import tempfile
    print("=== efficacy_draft_acceptance self-test ===")
    failures = 0

    def _assert(cond: bool, label: str, detail: str = "") -> bool:
        status = "PASS" if cond else "FAIL"
        suffix = f"  [{detail}]" if detail else ""
        print(f"  {status}  {label}{suffix}")
        return cond

    # ── fail-loud on < 10 drafts ───────────────────────────────────────────
    print("\n-- fail-loud on < 10 drafts --")

    with tempfile.TemporaryDirectory() as tmpdir:
        d = Path(tmpdir)
        # Write only 5 .pending files.
        files = []
        for i in range(5):
            p = d / f"SKILL-{i}.md.pending"
            p.write_text(f"draft {i}")
            files.append(p)

        try:
            compute_draft_acceptance_rate(files)
            ok = _assert(False, "5 drafts: expected SystemExit but none raised")
            failures += 0 if ok else 1
        except SystemExit as exc:
            ok = _assert(exc.code != 0, "5 drafts → SystemExit non-zero", f"code={exc.code}")
            failures += 0 if ok else 1

    # ── rate with exactly 10 drafts, 4 accepted ────────────────────────────
    print("\n-- rate with 10 drafts, 4 accepted --")

    with tempfile.TemporaryDirectory() as tmpdir:
        d = Path(tmpdir)
        files = []
        for i in range(10):
            pending = d / f"SKILL-{i}.md.pending"
            pending.write_text(f"draft {i}")
            files.append(pending)
            if i < 4:
                # Create accepted sibling.
                (d / f"SKILL-{i}.md").write_text(f"accepted {i}")

        rate = compute_draft_acceptance_rate(files)
        ok = _assert(rate["total"] == 10, "total=10", f"got {rate['total']}")
        failures += 0 if ok else 1
        ok = _assert(rate["accepted"] == 4, "accepted=4", f"got {rate['accepted']}")
        failures += 0 if ok else 1
        ok = _assert(abs(rate["accepted_rate"] - 0.4) < 1e-9,
                     "accepted_rate=0.4", f"got {rate['accepted_rate']}")
        failures += 0 if ok else 1

    # ── 100% acceptance ────────────────────────────────────────────────────
    print("\n-- 100% acceptance (10 of 10) --")

    with tempfile.TemporaryDirectory() as tmpdir:
        d = Path(tmpdir)
        files = []
        for i in range(10):
            pending = d / f"SKILL-{i}.md.pending"
            pending.write_text(f"draft {i}")
            files.append(pending)
            (d / f"SKILL-{i}.md").write_text(f"accepted {i}")

        rate = compute_draft_acceptance_rate(files)
        ok = _assert(abs(rate["accepted_rate"] - 1.0) < 1e-9,
                     "100% accepted", f"got {rate['accepted_rate']}")
        failures += 0 if ok else 1

    # ── 0% acceptance ─────────────────────────────────────────────────────
    print("\n-- 0% acceptance (none accepted) --")

    with tempfile.TemporaryDirectory() as tmpdir:
        d = Path(tmpdir)
        files = []
        for i in range(10):
            pending = d / f"SKILL-{i}.md.pending"
            pending.write_text(f"draft {i}")
            files.append(pending)
            # No accepted sibling.

        rate = compute_draft_acceptance_rate(files)
        ok = _assert(abs(rate["accepted_rate"] - 0.0) < 1e-9,
                     "0% accepted", f"got {rate['accepted_rate']}")
        failures += 0 if ok else 1

    print(f"\n{'=' * 40}")
    if failures == 0:
        print("ALL TESTS PASSED")
    else:
        print(f"{failures} TEST(S) FAILED", file=sys.stderr)
    return 0 if failures == 0 else 1


def main() -> None:
    """CLI entry point for the draft-acceptance scorer."""
    ap = argparse.ArgumentParser(
        description=(
            "Compute the draft-acceptance-rate over real captured .pending skill drafts. "
            f"Requires at least {MIN_DRAFTS_REQUIRED} real .pending files; "
            "exits non-zero with fewer (no fake rate reported)."
        )
    )
    ap.add_argument(
        "paths",
        nargs="*",
        help=(
            "Paths to .pending files, or a directory containing .pending files. "
            "If a directory is given, all *.pending files in it are examined."
        ),
    )
    ap.add_argument(
        "--self-test",
        action="store_true",
        help="Run module self-tests and exit.",
    )
    args = ap.parse_args()

    if args.self_test:
        sys.exit(_self_test())

    if not args.paths:
        ap.print_help()
        sys.exit(1)

    pending_files: list[Path] = []
    for raw_path in args.paths:
        p = Path(raw_path)
        if p.is_dir():
            pending_files.extend(sorted(p.glob("*.pending")))
        elif p.suffix == ".pending" or raw_path.endswith(".pending"):
            pending_files.append(p)
        else:
            print(f"WARNING: skipping non-.pending path: {p}", file=sys.stderr)

    if not pending_files:
        print(
            "ERROR: no .pending files found in the provided paths.",
            file=sys.stderr,
        )
        sys.exit(1)

    rate = compute_draft_acceptance_rate(pending_files)

    print(f"Draft acceptance rate: {rate['accepted_rate']:.1%}")
    print(f"  Total:    {rate['total']}")
    print(f"  Accepted: {rate['accepted']}")
    print(f"  Rejected: {rate['rejected']}")
    print()
    for item in rate["per_file"]:
        status = "ACCEPTED" if item["accepted"] else "rejected"
        print(f"  [{status}] {item['path']}")


if __name__ == "__main__":
    main()
