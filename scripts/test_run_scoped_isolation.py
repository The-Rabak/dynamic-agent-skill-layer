"""
test_run_scoped_isolation.py — Regression test for ticket #200.

Proves that stale report files that exist OUTSIDE the run-scoped directory
cannot contaminate the current run's aggregate or its summary.

Run with:
    python3 -m pytest scripts/test_run_scoped_isolation.py -v
or:
    python3 scripts/test_run_scoped_isolation.py
"""

from __future__ import annotations

import importlib.util
import json
import pathlib
import subprocess
import sys
import tempfile
import textwrap
from typing import Any


# ---------------------------------------------------------------------------
# Locate generate-e2e-summary.py relative to this test file so it works
# regardless of the current working directory.
# ---------------------------------------------------------------------------
_SCRIPTS_DIR = pathlib.Path(__file__).resolve().parent
_SUMMARY_SCRIPT = _SCRIPTS_DIR / "generate-e2e-summary.py"


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_passing_report(test_name: str, test_id: str) -> dict[str, Any]:
    """Return a minimal valid E2EReport JSON object with a Passed outcome."""
    return {
        "test_name": test_name,
        "test_id": test_id,
        "started_at": "2026-06-06T12:00:00+00:00",
        "duration_ms": 100,
        "outcome": {"status": "Passed"},
        "sections": [],
        "environment": {
            "pg_version": "15.3",
            "qdrant_version": "1.7.0",
            "ollama_model": "nomic-embed-text",
            "redis_version": "7.2",
        },
        "contract_assertions": [],
        "degradation_events": [],
        "latency_samples": [],
    }


def _make_failing_report(test_name: str, test_id: str) -> dict[str, Any]:
    """Return a minimal valid E2EReport JSON object with a Failed outcome."""
    report = _make_passing_report(test_name, test_id)
    report["outcome"] = {"status": "Failed", "reason": "assertion error in stale run"}
    return report


def _aggregate_run_dir(run_dir: pathlib.Path, run_id: str, aggregate_path: pathlib.Path) -> dict[str, Any]:
    """Reproduce the aggregation logic from run-e2e-tests.sh in pure Python.

    Only globs ``run_dir/*.json``, skipping any file whose name starts with
    ``run__`` (to avoid re-ingesting a previous aggregate).  Writes the
    aggregate to ``aggregate_path`` and returns the parsed object.
    """
    reports: list[dict[str, Any]] = []
    for path in sorted(run_dir.glob("*.json")):
        if path.name.startswith("run__"):
            continue
        with open(path, encoding="utf-8") as fh:
            try:
                reports.append(json.load(fh))
            except json.JSONDecodeError:
                pass  # malformed files are skipped with a warning in the real script

    total = len(reports)
    passed = sum(1 for r in reports if r.get("outcome", {}).get("status") == "Passed")
    failed = sum(1 for r in reports if r.get("outcome", {}).get("status") == "Failed")
    degraded_passed = sum(
        1
        for r in reports
        if r.get("outcome", {}).get("status") == "Passed"
        and any(d.get("service") for d in r.get("degradation_events", []))
    )

    aggregate: dict[str, Any] = {
        "run_id": run_id,
        "run_artifact_root": str(run_dir),
        "run_summary": {
            "total_tests": total,
            "passed": passed,
            "failed": failed,
            "degraded_passed": degraded_passed,
            "total_duration_ms": sum(r.get("duration_ms", 0) for r in reports),
            "start_time": min((r.get("started_at", "") for r in reports), default=""),
            "end_time": max((r.get("started_at", "") for r in reports), default=""),
            "container_versions": {},
        },
        "reports": reports,
    }

    aggregate_path.parent.mkdir(parents=True, exist_ok=True)
    with open(aggregate_path, "w", encoding="utf-8") as fh:
        json.dump(aggregate, fh, indent=2)

    return aggregate


def _run_summary_script(input_path: pathlib.Path, output_path: pathlib.Path) -> str:
    """Invoke generate-e2e-summary.py as a subprocess and return stdout."""
    result = subprocess.run(
        [
            sys.executable,
            str(_SUMMARY_SCRIPT),
            "--input", str(input_path),
            "--output", str(output_path),
        ],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise AssertionError(
            f"generate-e2e-summary.py exited {result.returncode}:\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )
    return result.stdout


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_stale_failing_report_outside_run_dir_does_not_contaminate_aggregate() -> None:
    """A stale failing report written directly to reports/ (not the run dir)
    must NOT appear in the current run's aggregate.

    This is the headline regression for ticket #200: prior to the fix the
    aggregator globbed the entire reports/ tree, so any old failing *.json
    would silently flip the summary to RED.
    """
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = pathlib.Path(tmp)

        # Simulate the broader tests/e2e/reports/ root.
        reports_root = tmp_path / "tests" / "e2e" / "reports"
        reports_root.mkdir(parents=True)

        # ----------------------------------------------------------------
        # 1. Place a STALE FAILING report directly in reports/ — outside
        #    any run-scoped subdirectory.  This mimics historical pollution.
        # ----------------------------------------------------------------
        stale_report = _make_failing_report(
            test_name="stale_old_test",
            test_id="20250101120000",
        )
        stale_path = reports_root / "stale_old_test__20250101120000.json"
        stale_path.write_text(json.dumps(stale_report), encoding="utf-8")

        # ----------------------------------------------------------------
        # 2. Create a run-scoped directory for the CURRENT run and place
        #    only passing reports there.
        # ----------------------------------------------------------------
        run_id = "run-20260606-120000-99"
        run_dir = reports_root / "runs" / run_id
        run_dir.mkdir(parents=True)

        current_report_a = _make_passing_report("golden_path", "20260606120001")
        (run_dir / "golden_path__20260606120001.json").write_text(
            json.dumps(current_report_a), encoding="utf-8"
        )

        current_report_b = _make_passing_report("compile_context", "20260606120002")
        (run_dir / "compile_context__20260606120002.json").write_text(
            json.dumps(current_report_b), encoding="utf-8"
        )

        # ----------------------------------------------------------------
        # 3. Aggregate ONLY the run dir (mirrors the fixed shell script).
        # ----------------------------------------------------------------
        aggregate_path = run_dir / "run__20260606120003.json"
        aggregate = _aggregate_run_dir(run_dir, run_id, aggregate_path)

        # ----------------------------------------------------------------
        # 4. Assert the stale report is absent from the aggregate.
        # ----------------------------------------------------------------
        aggregated_test_names = {r["test_name"] for r in aggregate["reports"]}
        assert "stale_old_test" not in aggregated_test_names, (
            f"Stale failing report leaked into the aggregate. "
            f"Aggregated tests: {aggregated_test_names}"
        )
        assert aggregate["run_summary"]["failed"] == 0, (
            f"Aggregate shows failed={aggregate['run_summary']['failed']} "
            f"but no current-run test failed — stale report must have leaked in."
        )
        assert aggregate["run_summary"]["total_tests"] == 2
        assert aggregate["run_summary"]["passed"] == 2

        print("  [OK] stale report absent from aggregate")


def test_summary_is_green_when_run_dir_has_only_passing_reports() -> None:
    """generate-e2e-summary.py must produce a GREEN summary when the
    run-scoped aggregate contains only passing reports, regardless of stale
    failing files sitting in the broader reports/ directory.
    """
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = pathlib.Path(tmp)
        reports_root = tmp_path / "tests" / "e2e" / "reports"
        reports_root.mkdir(parents=True)

        # Stale failing report at the old broad location.
        stale_report = _make_failing_report("ancient_broken_test", "20240101000000")
        (reports_root / "ancient_broken_test__20240101000000.json").write_text(
            json.dumps(stale_report), encoding="utf-8"
        )

        # Current run with two passing reports.
        run_id = "run-20260606-130000-77"
        run_dir = reports_root / "runs" / run_id
        run_dir.mkdir(parents=True)

        for name, tid in [("test_alpha", "20260606130001"), ("test_beta", "20260606130002")]:
            report = _make_passing_report(name, tid)
            (run_dir / f"{name}__{tid}.json").write_text(
                json.dumps(report), encoding="utf-8"
            )

        aggregate_path = run_dir / "run__20260606130003.json"
        _aggregate_run_dir(run_dir, run_id, aggregate_path)

        summary_path = run_dir / "summary.md"
        stdout = _run_summary_script(aggregate_path, summary_path)

        summary_text = summary_path.read_text(encoding="utf-8")

        # The summary must declare GREEN.
        assert "GREEN" in summary_text, (
            f"Expected GREEN in summary but got:\n{summary_text[:800]}"
        )
        assert "RED" not in summary_text, (
            f"Expected no RED in summary but stale failure appears to have leaked:\n"
            f"{summary_text[:800]}"
        )

        # The run identity header must be present.
        assert run_id in summary_text, (
            f"Run ID {run_id!r} missing from summary header:\n{summary_text[:800]}"
        )
        assert str(run_dir) in summary_text, (
            f"Artifact root {str(run_dir)!r} missing from summary header"
        )

        print("  [OK] summary is GREEN; stale failing report did not flip result")
        print("  [OK] run identity header present in summary")

        # Print the top of the summary for evidence.
        header_lines = summary_text.splitlines()[:30]
        print("\n--- rendered summary top ---")
        for line in header_lines:
            print(line)
        print("---")


def test_stale_failing_report_not_in_summary_table() -> None:
    """The per-test result table in the summary must not list the stale test."""
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = pathlib.Path(tmp)
        reports_root = tmp_path / "tests" / "e2e" / "reports"
        reports_root.mkdir(parents=True)

        stale = _make_failing_report("notorious_stale_flaky_test", "20230515090000")
        (reports_root / "notorious_stale_flaky_test__20230515090000.json").write_text(
            json.dumps(stale), encoding="utf-8"
        )

        run_id = "run-20260606-140000-55"
        run_dir = reports_root / "runs" / run_id
        run_dir.mkdir(parents=True)

        report = _make_passing_report("fresh_clean_test", "20260606140001")
        (run_dir / "fresh_clean_test__20260606140001.json").write_text(
            json.dumps(report), encoding="utf-8"
        )

        aggregate_path = run_dir / "run__20260606140002.json"
        _aggregate_run_dir(run_dir, run_id, aggregate_path)

        summary_path = run_dir / "summary.md"
        _run_summary_script(aggregate_path, summary_path)
        summary_text = summary_path.read_text(encoding="utf-8")

        assert "notorious_stale_flaky_test" not in summary_text, (
            "Stale test name appeared in summary — stale report leaked."
        )
        assert "fresh_clean_test" in summary_text, (
            "Current test name missing from summary."
        )

        print("  [OK] stale test name absent from summary table")


# ---------------------------------------------------------------------------
# Self-runner for direct invocation (python3 scripts/test_run_scoped_isolation.py)
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    tests = [
        test_stale_failing_report_outside_run_dir_does_not_contaminate_aggregate,
        test_summary_is_green_when_run_dir_has_only_passing_reports,
        test_stale_failing_report_not_in_summary_table,
    ]
    failures: list[str] = []
    for fn in tests:
        label = fn.__name__
        try:
            fn()
            print(f"PASS  {label}")
        except Exception as exc:
            failures.append(label)
            print(f"FAIL  {label}: {exc}")

    if failures:
        print(f"\n{len(failures)} test(s) FAILED: {', '.join(failures)}")
        sys.exit(1)
    else:
        print(f"\nAll {len(tests)} tests passed.")
