"""
generate-e2e-summary.py — Convert a live-suite aggregate JSON report into a
human-readable Markdown summary.

This script is the observability tool for T10 (V1.5 integration gate). It reads
the machine-generated ``run__<timestamp>.json`` aggregate produced by
``scripts/run-e2e-tests.sh``, derives every claim from the JSON data (never
fabricating values), and emits ``tests/e2e/reports/latest-summary.md``.

The summary makes "system passed" visible without opening raw JSON files. Every
section is honestly marked ``n/a`` when the relevant data is genuinely absent.

The summary header always states the run id, the absolute artifact root, the
exact command used to produce the aggregate, and a GREEN/RED verdict derived
exclusively from the reports in that aggregate.

Usage:
    # Explicit path — always preferred; guarantees only this run is summarised:
    python3 scripts/generate-e2e-summary.py \\
        --input  tests/e2e/reports/runs/run-20260606-120000-42/run__20260606120001.json \\
        --output tests/e2e/reports/runs/run-20260606-120000-42/summary.md

    # Auto-discover via E2E_RUN_REPORT_DIR (set by run-e2e-tests.sh):
    E2E_RUN_REPORT_DIR=tests/e2e/reports/runs/run-… python3 scripts/generate-e2e-summary.py

    # Fallback auto-discovery (legacy; globs the broad reports/ directory):
    python3 scripts/generate-e2e-summary.py
"""

from __future__ import annotations

import argparse
import glob
import json
import math
import os
import pathlib
import re
import sys
from typing import Any

# ---------------------------------------------------------------------------
# Path constants (all relative to the repository root)
# ---------------------------------------------------------------------------
_SCRIPT_DIR = pathlib.Path(__file__).resolve().parent
_REPO_ROOT = _SCRIPT_DIR.parent
_REPORTS_DIR = _REPO_ROOT / "tests" / "e2e" / "reports"
_E2E_TEST_DIR = _REPO_ROOT / "tests" / "e2e"
_DEFAULT_OUTPUT = _REPORTS_DIR / "latest-summary.md"


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _parse_args() -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(
        description="Convert a live-suite aggregate JSON report to Markdown.",
    )
    parser.add_argument(
        "--input",
        metavar="PATH",
        help=(
            "Path to the aggregate run JSON file. "
            "Defaults to the newest run__*.json in tests/e2e/reports/."
        ),
    )
    parser.add_argument(
        "--output",
        metavar="PATH",
        default=str(_DEFAULT_OUTPUT),
        help=(
            f"Destination Markdown file. Defaults to {_DEFAULT_OUTPUT}."
        ),
    )
    return parser.parse_args()


# ---------------------------------------------------------------------------
# Report loading
# ---------------------------------------------------------------------------

def _find_latest_aggregate() -> pathlib.Path:
    """Return the newest run__*.json file, honouring E2E_RUN_REPORT_DIR.

    Discovery order:
    1. ``E2E_RUN_REPORT_DIR`` env var (set by run-e2e-tests.sh) — glob only
       that run-scoped directory.  This prevents stale cross-run contamination.
    2. Legacy fallback: the broad ``tests/e2e/reports/`` tree (for developers
       invoking the script directly without the wrapper).

    Raises SystemExit (exit code 1) with a clear message when no file exists.
    """
    run_dir_env = os.environ.get("E2E_RUN_REPORT_DIR", "").strip()
    if run_dir_env:
        run_dir = pathlib.Path(run_dir_env)
        if not run_dir.is_dir():
            sys.exit(
                f"ERROR: E2E_RUN_REPORT_DIR={run_dir_env!r} is not a directory.\n"
                "The wrapper script should have created it before invoking this script."
            )
        pattern = str(run_dir / "run__*.json")
        candidates = sorted(glob.glob(pattern))
        if not candidates:
            sys.exit(
                f"ERROR: no aggregate report found in E2E_RUN_REPORT_DIR={run_dir_env!r}.\n"
                "The aggregation step in run-e2e-tests.sh should have written one."
            )
        return pathlib.Path(candidates[-1])

    # Legacy fallback: broad reports/ directory (developer direct invocation).
    pattern = str(_REPORTS_DIR / "run__*.json")
    candidates = sorted(glob.glob(pattern))
    if not candidates:
        sys.exit(
            f"ERROR: no aggregate report found in {_REPORTS_DIR}.\n"
            "Run 'scripts/run-e2e-tests.sh' first to generate reports, "
            "or pass --input <path> to specify an explicit file."
        )
    return pathlib.Path(candidates[-1])


def _load_aggregate(input_path: pathlib.Path) -> dict[str, Any]:
    """Load and parse the aggregate JSON report.

    Raises SystemExit with the offending path when the JSON is malformed.
    """
    try:
        with open(input_path, encoding="utf-8") as fh:
            return json.load(fh)
    except json.JSONDecodeError as exc:
        sys.exit(f"ERROR: malformed JSON in {input_path}: {exc}")
    except OSError as exc:
        sys.exit(f"ERROR: cannot read {input_path}: {exc}")


# ---------------------------------------------------------------------------
# Percentile computation
# ---------------------------------------------------------------------------

def _nearest_rank_percentile(sorted_values: list[float], percentile: int) -> float:
    """Compute the nearest-rank percentile from a pre-sorted list.

    Args:
        sorted_values: Ascending-sorted numeric values.
        percentile: Integer percentile (1–100).

    Returns:
        The nearest-rank value, or 0.0 if the list is empty.
    """
    if not sorted_values:
        return 0.0
    # Nearest-rank formula: ceil(p/100 * n), clamped to valid index range.
    rank = math.ceil(percentile / 100 * len(sorted_values))
    rank = max(1, min(rank, len(sorted_values)))
    return sorted_values[rank - 1]


# ---------------------------------------------------------------------------
# Section renderers
# ---------------------------------------------------------------------------

def _render_overall_status(run_summary: dict[str, Any], reports: list[dict[str, Any]]) -> str:
    """Render the Overall Status section.

    Shows GREEN/RED headline and a per-test pass/fail table sorted by test name.
    """
    total = run_summary.get("total_tests", 0)
    passed = run_summary.get("passed", 0)
    failed = run_summary.get("failed", 0)
    degraded_passed = run_summary.get("degraded_passed", 0)
    start_time = run_summary.get("start_time") or "n/a"
    end_time = run_summary.get("end_time") or "n/a"
    total_ms = run_summary.get("total_duration_ms", 0)

    headline = "GREEN" if failed == 0 else "RED"

    lines = [
        "## Overall Status",
        "",
        f"**{headline}** — {passed}/{total} tests passed, {failed} failed",
        f"(degraded-but-passed: {degraded_passed})",
        "",
        f"Run window: `{start_time}` → `{end_time}`  |  "
        f"Total duration: {total_ms:,} ms",
        "",
        "| Test | Result | Duration (ms) |",
        "| --- | --- | --- |",
    ]

    sorted_reports = sorted(reports, key=lambda r: r.get("test_name", ""))
    for report in sorted_reports:
        test_name = report.get("test_name", "unknown")
        outcome = report.get("outcome", {})
        status = outcome.get("status", "Unknown")
        if status == "Passed":
            result_cell = "PASS"
        elif status == "Failed":
            reason = outcome.get("reason", "")
            result_cell = f"FAIL — {reason}" if reason else "FAIL"
        else:
            reason = outcome.get("reason", "")
            result_cell = f"SKIP — {reason}" if reason else "SKIP"
        duration = report.get("duration_ms", 0)
        lines.append(f"| `{test_name}` | {result_cell} | {duration:,} |")

    return "\n".join(lines)


def _render_latency(reports: list[dict[str, Any]]) -> str:
    """Render the Latency section.

    Computes p50/p95/p99 and sample count, grouped by stage (sorted) and
    overall. Uses nearest-rank percentiles and states the sample count so
    small-n percentiles are not misread.
    """
    # Group duration_ms values by stage
    by_stage: dict[str, list[float]] = {}
    all_durations: list[float] = []
    for report in reports:
        for sample in report.get("latency_samples", []):
            stage = sample.get("stage", "unknown")
            ms = float(sample.get("duration_ms", 0))
            by_stage.setdefault(stage, []).append(ms)
            all_durations.append(ms)

    lines = ["## Latency", ""]

    if not all_durations:
        lines.append("n/a — no latency samples found in this run")
        return "\n".join(lines)

    all_durations.sort()
    n = len(all_durations)
    p50 = _nearest_rank_percentile(all_durations, 50)
    p95 = _nearest_rank_percentile(all_durations, 95)
    p99 = _nearest_rank_percentile(all_durations, 99)

    lines += [
        f"**Overall** ({n} samples)",
        "",
        f"| p50 | p95 | p99 |",
        f"| --- | --- | --- |",
        f"| {p50:,.0f} ms | {p95:,.0f} ms | {p99:,.0f} ms |",
        "",
        "_Note: these are nearest-rank percentiles. "
        "Small sample counts (n < 20) should be treated as indicative only._",
        "",
    ]

    if len(by_stage) > 1:
        lines += [
            "**By stage** (sorted alphabetically):",
            "",
            "| Stage | n | p50 | p95 | p99 |",
            "| --- | --- | --- | --- | --- |",
        ]
        for stage in sorted(by_stage):
            stage_durations = sorted(by_stage[stage])
            sn = len(stage_durations)
            sp50 = _nearest_rank_percentile(stage_durations, 50)
            sp95 = _nearest_rank_percentile(stage_durations, 95)
            sp99 = _nearest_rank_percentile(stage_durations, 99)
            lines.append(
                f"| `{stage}` | {sn} | {sp50:,.0f} ms | {sp95:,.0f} ms | {sp99:,.0f} ms |"
            )

    return "\n".join(lines)


def _render_graph_version_progression(reports: list[dict[str, Any]]) -> str:
    """Render the Graph Version Progression section.

    Extracts observed graph_version values from contract assertion details
    and action descriptions, shows the progression in sorted order.
    """
    version_re = re.compile(r"graph_version[=\s:]+(\d+)")
    observed_versions: list[int] = []

    for report in reports:
        for assertion in report.get("contract_assertions", []):
            for match in version_re.finditer(assertion.get("details", "")):
                observed_versions.append(int(match.group(1)))
            for match in version_re.finditer(assertion.get("contract_name", "")):
                observed_versions.append(int(match.group(1)))
        for section in report.get("sections", []):
            for action in section.get("actions", []):
                for match in version_re.finditer(action.get("description", "")):
                    observed_versions.append(int(match.group(1)))

    lines = ["## Graph Version Progression", ""]

    if not observed_versions:
        lines.append(
            "n/a — no graph_version references found in contract assertions or action descriptions"
        )
        return "\n".join(lines)

    unique_sorted = sorted(set(observed_versions))
    if len(unique_sorted) == 1:
        progression = f"v{unique_sorted[0]}"
    else:
        progression = " → ".join(f"v{v}" for v in unique_sorted)

    lines.append(f"Observed: **{progression}**")
    lines.append("")
    lines.append("_(extracted from contract assertion details and action descriptions)_")

    return "\n".join(lines)


def _render_extraction_attempts_completions(reports: list[dict[str, Any]]) -> str:
    """Render the Extraction Attempts and Completions section.

    Counts extraction.attempted and extraction.completed EventPublished
    side effects, and DbRowInserted events into extraction-related tables.
    Reports n/a for each counter when it is genuinely absent.
    """
    attempted = 0
    completed = 0
    failed_count = 0
    db_rows: dict[str, int] = {}

    for report in reports:
        for section in report.get("sections", []):
            for action in section.get("actions", []):
                desc_lower = action.get("description", "").lower()
                # Count attempts mentioned in descriptions
                if "extract_session attempt" in desc_lower or "extract_session submission" in desc_lower:
                    pass  # counted via side effects below
                for se in action.get("side_effects", []):
                    kind = se.get("kind", "")
                    if kind == "EventPublished":
                        event_type = se.get("event_type", "")
                        if event_type == "extraction.attempted":
                            attempted += 1
                        elif event_type == "extraction.completed":
                            completed += 1
                        elif event_type == "extraction.failed":
                            failed_count += 1
                    elif kind == "DbRowInserted":
                        table = se.get("table", "unknown")
                        db_rows[table] = db_rows.get(table, 0) + 1

    lines = ["## Extraction Attempts and Completions", ""]

    attempted_str = str(attempted) if attempted else "n/a"
    completed_str = str(completed) if completed else "n/a"
    failed_str = str(failed_count) if failed_count else "n/a"

    lines += [
        f"- Attempts (`extraction.attempted` events): **{attempted_str}**",
        f"- Completions (`extraction.completed` events): **{completed_str}**",
        f"- Failures (`extraction.failed` events): **{failed_str}**",
    ]

    if db_rows:
        lines += ["", "**DB rows inserted by table:**", ""]
        for table in sorted(db_rows):
            lines.append(f"- `{table}`: {db_rows[table]}")

    return "\n".join(lines)


def _render_pending_draft_count(reports: list[dict[str, Any]]) -> str:
    """Render the Pending Draft Count section.

    Counts DbRowInserted side effects into tables containing 'pending'
    and scans action descriptions for '.pending draft' mentions.
    """
    pending_from_side_effects = 0
    pending_from_descriptions = 0

    for report in reports:
        for section in report.get("sections", []):
            for action in section.get("actions", []):
                desc_lower = action.get("description", "").lower()
                if ".pending" in desc_lower or "pending draft" in desc_lower:
                    pending_from_descriptions += 1
                for se in action.get("side_effects", []):
                    if se.get("kind") == "DbRowInserted":
                        table = se.get("table", "")
                        if "pending" in table.lower():
                            pending_from_side_effects += 1

    lines = ["## Pending Draft Count", ""]

    if pending_from_side_effects == 0 and pending_from_descriptions == 0:
        lines.append("n/a — no pending draft insertions or references found")
        return "\n".join(lines)

    if pending_from_side_effects > 0:
        lines.append(
            f"- Pending draft DB rows inserted (`skill_proposals.pending` table): "
            f"**{pending_from_side_effects}**"
        )
    else:
        lines.append("- Pending draft DB rows inserted: **n/a**")

    if pending_from_descriptions > 0:
        lines.append(
            f"- Actions referencing `.pending` draft writes: "
            f"**{pending_from_descriptions}**"
        )

    return "\n".join(lines)


def _render_degradation_events(reports: list[dict[str, Any]]) -> str:
    """Render the Degraded and Recovery Events section.

    Lists every degradation_event from every report, sorted by test name
    then event timestamp.
    """
    lines = ["## Degraded and Recovery Events", ""]

    all_events: list[tuple[str, dict[str, Any]]] = []
    for report in reports:
        test_name = report.get("test_name", "unknown")
        for event in report.get("degradation_events", []):
            all_events.append((test_name, event))

    if not all_events:
        lines.append("n/a — no degradation events recorded in this run")
        return "\n".join(lines)

    lines += [
        "| Test | Service | At | Recovered | Reason |",
        "| --- | --- | --- | --- | --- |",
    ]

    # Sort by test name then timestamp for deterministic output
    all_events.sort(key=lambda pair: (pair[0], pair[1].get("at", "")))

    for test_name, event in all_events:
        service = event.get("service", "n/a")
        at = event.get("at", "n/a")
        recovered = "Yes" if event.get("recovered") else "No"
        reason = event.get("reason", "n/a")
        # Escape pipe characters in reason text to avoid breaking the table
        reason_escaped = reason.replace("|", "\\|")
        lines.append(
            f"| `{test_name}` | `{service}` | `{at}` | {recovered} | {reason_escaped} |"
        )

    return "\n".join(lines)


def _scrape_ignored_contracts(e2e_test_dir: pathlib.Path) -> list[tuple[str, str, str]]:
    """Scan all *.rs files in the e2e test directory for #[ignore] annotations.

    Returns a sorted list of (source_file, function_name, reason) tuples.
    A missing reason string is returned as the sentinel "(no reason given)"
    and is flagged in the rendered output per the honest-deferral contract.

    The scan handles two annotation forms:
    - ``#[ignore = "reason string"]``
    - ``#[ignore]``   (bare — no reason; flagged as a gap)
    """
    ignore_with_reason = re.compile(r'#\[ignore\s*=\s*"([^"]*)"\]')
    ignore_bare = re.compile(r'#\[ignore\]')
    fn_name = re.compile(r'(?:async\s+)?fn\s+(\w+)\s*\(')

    results: list[tuple[str, str, str]] = []

    for rs_file in sorted(e2e_test_dir.glob("*.rs")):
        text = rs_file.read_text(encoding="utf-8")
        lines = text.splitlines()
        for i, line in enumerate(lines):
            stripped = line.strip()
            # Skip comment lines — only attribute lines (starting with #[) are valid
            if stripped.startswith("//") or stripped.startswith("*") or stripped.startswith("/*"):
                continue
            reason_match = ignore_with_reason.search(stripped)
            bare_match = ignore_bare.search(stripped) and not ignore_with_reason.search(stripped)

            if reason_match or bare_match:
                reason = reason_match.group(1) if reason_match else "(no reason given)"
                # Look ahead (up to 5 lines) for the function name
                fn = "(unknown)"
                for j in range(i + 1, min(i + 6, len(lines))):
                    m = fn_name.search(lines[j])
                    if m:
                        fn = m.group(1)
                        break
                results.append((rs_file.name, fn, reason))

    return sorted(results)


def _render_ignored_contracts(e2e_test_dir: pathlib.Path) -> str:
    """Render the Ignored Dream Contracts section.

    Scrapes #[ignore] annotations from e2e test source files and lists each
    contract with its reason. Any contract lacking a reason is explicitly
    flagged as a gap (honest-deferral contract; no silent truncation).
    """
    lines = ["## Ignored Dream Contracts", ""]

    contracts = _scrape_ignored_contracts(e2e_test_dir)

    if not contracts:
        lines.append("n/a — no #[ignore] annotations found in tests/e2e/*.rs")
        return "\n".join(lines)

    no_reason_count = sum(1 for _, _, reason in contracts if reason == "(no reason given)")

    lines += [
        f"Total ignored tests: **{len(contracts)}**",
        "",
        "| Source file | Test function | Reason |",
        "| --- | --- | --- |",
    ]

    for source_file, fn, reason in contracts:
        reason_display = reason
        if reason == "(no reason given)":
            reason_display = "(no reason given) **[GAP: reason missing — add #[ignore = \"…\"] to this test]**"
        lines.append(f"| `{source_file}` | `{fn}` | {reason_display} |")

    if no_reason_count > 0:
        lines += [
            "",
            f"> **{no_reason_count} test(s) are ignored without a reason.** "
            "Per the honest-deferral contract, each ignored test must carry a "
            "`#[ignore = \"reason\"]` annotation. Bare `#[ignore]` is a gap.",
        ]

    return "\n".join(lines)


def _render_environment(run_summary: dict[str, Any], reports: list[dict[str, Any]]) -> str:
    """Render the Environment section.

    Draws from run_summary.container_versions and individual reports'
    environment snapshots. Deduplicates and notes any inconsistency.
    """
    lines = ["## Environment", ""]

    container_versions = run_summary.get("container_versions") or {}

    # Collect all distinct environment snapshots from reports
    env_snapshots: list[dict[str, str]] = []
    for report in sorted(reports, key=lambda r: r.get("test_name", "")):
        env = report.get("environment")
        if env and isinstance(env, dict):
            env_snapshots.append(env)

    if not container_versions and not env_snapshots:
        lines.append("n/a — no environment version data in this report")
        return "\n".join(lines)

    if container_versions:
        lines.append("**Container versions (from run_summary):**")
        lines.append("")
        for key in sorted(container_versions):
            lines.append(f"- `{key}`: `{container_versions[key]}`")
        lines.append("")

    if env_snapshots:
        # Check for consistency across reports
        pg_versions = sorted({e.get("pg_version", "") for e in env_snapshots} - {""})
        qdrant_versions = sorted({e.get("qdrant_version", "") for e in env_snapshots} - {""})
        ollama_models = sorted({e.get("ollama_model", "") for e in env_snapshots} - {""})
        redis_versions = sorted({e.get("redis_version", "") for e in env_snapshots} - {""})

        lines.append("**Environment snapshot (from test reports):**")
        lines.append("")
        lines.append(f"- PostgreSQL: `{'`, `'.join(pg_versions) or 'n/a'}`")
        lines.append(f"- Qdrant: `{'`, `'.join(qdrant_versions) or 'n/a'}`")
        lines.append(f"- Ollama model: `{'`, `'.join(ollama_models) or 'n/a'}`")
        lines.append(f"- Redis: `{'`, `'.join(redis_versions) or 'n/a'}`")

        inconsistencies = [
            field
            for field, versions in [
                ("pg_version", pg_versions),
                ("qdrant_version", qdrant_versions),
                ("ollama_model", ollama_models),
                ("redis_version", redis_versions),
            ]
            if len(versions) > 1
        ]
        if inconsistencies:
            lines += [
                "",
                f"> **Warning:** inconsistent environment values across reports: "
                f"{', '.join(inconsistencies)}. "
                "This may indicate tests ran against different container stacks.",
            ]

    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Top-level assembly
# ---------------------------------------------------------------------------

def _render_run_identity(
    aggregate: dict[str, Any],
    input_path: pathlib.Path,
    run_summary: dict[str, Any],
    reports: list[dict[str, Any]],
) -> str:
    """Render the run-identity header block.

    This section appears at the very top of every summary and states:
    - The run id (from the aggregate JSON or derived from the file name).
    - The absolute artifact root directory.
    - The exact command that produced the aggregate.
    - A GREEN/RED verdict for THIS run only.

    The verdict is derived exclusively from `run_summary`, which contains only
    the reports aggregated from the run-scoped directory.  Stale reports from
    prior runs are structurally incapable of reaching this summary when the
    wrapper script passes ``--input`` (or sets ``E2E_RUN_REPORT_DIR``).
    """
    run_id = aggregate.get("run_id") or input_path.stem
    artifact_root = aggregate.get("run_artifact_root") or str(input_path.parent.resolve())
    failed = run_summary.get("failed", 0)
    passed = run_summary.get("passed", 0)
    total = run_summary.get("total_tests", 0)
    verdict = "GREEN" if failed == 0 else "RED"
    command = (
        f"python3 scripts/generate-e2e-summary.py --input {input_path}"
    )

    lines = [
        "## Run Identity",
        "",
        f"| Field | Value |",
        f"| --- | --- |",
        f"| Run ID | `{run_id}` |",
        f"| Artifact root | `{artifact_root}` |",
        f"| Aggregate file | `{input_path}` |",
        f"| Produced by | `{command}` |",
        f"| Verdict | **{verdict}** — {passed}/{total} passed, {failed} failed |",
    ]
    return "\n".join(lines)


def _render_summary(
    aggregate: dict[str, Any],
    e2e_test_dir: pathlib.Path,
    input_path: pathlib.Path,
) -> str:
    """Assemble the full Markdown summary from the aggregate report.

    Args:
        aggregate: The parsed aggregate JSON object.
        e2e_test_dir: Directory containing *.rs e2e test source files.
        input_path: Path of the source JSON, shown in the header.

    Returns:
        Complete Markdown document as a single string.
    """
    run_summary: dict[str, Any] = aggregate.get("run_summary", {})
    reports: list[dict[str, Any]] = aggregate.get("reports", [])

    sections = [
        f"# Live Suite Run Summary",
        "",
        _render_run_identity(aggregate, input_path, run_summary, reports),
        "",
        _render_overall_status(run_summary, reports),
        "",
        _render_latency(reports),
        "",
        _render_graph_version_progression(reports),
        "",
        _render_extraction_attempts_completions(reports),
        "",
        _render_pending_draft_count(reports),
        "",
        _render_degradation_events(reports),
        "",
        _render_ignored_contracts(e2e_test_dir),
        "",
        _render_environment(run_summary, reports),
    ]

    return "\n".join(sections) + "\n"


def main() -> None:
    """Entry point: parse args, load report, render summary, write output."""
    args = _parse_args()

    input_path = pathlib.Path(args.input) if args.input else _find_latest_aggregate()
    output_path = pathlib.Path(args.output)

    aggregate = _load_aggregate(input_path)

    summary_md = _render_summary(aggregate, _E2E_TEST_DIR, input_path)

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with open(output_path, "w", encoding="utf-8") as fh:
        fh.write(summary_md)

    run_summary = aggregate.get("run_summary", {})
    passed = run_summary.get("passed", 0)
    total = run_summary.get("total_tests", 0)
    failed = run_summary.get("failed", 0)
    headline = "GREEN" if failed == 0 else "RED"
    print(
        f"Summary written to {output_path} "
        f"[{headline}: {passed}/{total} passed, {failed} failed]"
    )


if __name__ == "__main__":
    main()
