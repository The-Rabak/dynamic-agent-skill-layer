"""
Fixture-driven acceptance test for scripts/generate-e2e-summary.py.

Runs the generator against sample_run_aggregate.json and asserts that
the rendered latest-summary.md contains all required sections with expected
content derived deterministically from the fixture.

Usage:
    python3 tests/e2e/fixtures/test_summary_generator.py

Exit 0 = all assertions passed (Green).
Exit 1 = one or more assertions failed (Red or regression).
"""

import subprocess
import sys
import os
import tempfile
import pathlib

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent.parent.parent
GENERATOR = REPO_ROOT / "scripts" / "generate-e2e-summary.py"
FIXTURE = REPO_ROOT / "tests" / "e2e" / "fixtures" / "sample_run_aggregate.json"

REQUIRED_SECTIONS = [
    "## Overall Status",
    "## Latency",
    "## Graph Version Progression",
    "## Extraction Attempts and Completions",
    "## Pending Draft Count",
    "## Degraded and Recovery Events",
    "## Ignored Dream Contracts",
    "## Environment",
]

REQUIRED_CONTENT = [
    # Overall status: GREEN/RED + per-test table
    "RED",           # 1 failure makes this RED
    "DS-003_dependency_chaos_matrix",
    "DS-004_outbox_backlog_replay",
    "DS-005_qdrant_pg_drift",
    "PASS",
    "FAIL",
    # Latency: percentile labels
    "p50",
    "p95",
    "p99",
    # sample count present
    "samples",
    # graph version progression section shows observed version value
    "v5",
    # extraction
    "extraction.completed",
    # pending draft
    "pending",
    # degradation events
    "qdrant",
    "recovered",
    # ignored contracts: dream state contracts listed by function name
    # DS-001: full_session_analysis_extraction_ingestion_retrieval_loop_is_deterministic
    "full_session_analysis_extraction_ingestion_retrieval_loop_is_deterministic",
    # DS-008: multi_repo_scope_isolation_prevents_cross_tenant_context_leakage
    "multi_repo_scope_isolation_prevents_cross_tenant_context_leakage",
    # environment
    "granite4:3b",
    "16.3",
]


def run_generator(output_path: str) -> subprocess.CompletedProcess:
    """Invoke the generator with explicit --input and --output overrides."""
    return subprocess.run(
        [
            sys.executable,
            str(GENERATOR),
            "--input", str(FIXTURE),
            "--output", output_path,
        ],
        capture_output=True,
        text=True,
    )


def assert_section_present(content: str, section_header: str) -> None:
    """Raise AssertionError if the named section header is absent from content."""
    if section_header not in content:
        raise AssertionError(f"Required section missing: {section_header!r}")


def assert_content_present(content: str, fragment: str) -> None:
    """Raise AssertionError if the expected text fragment is absent from content."""
    if fragment not in content:
        raise AssertionError(f"Required content fragment missing: {fragment!r}")


def main() -> int:
    """Run all fixture-driven assertions; return 0 on pass, 1 on failure."""
    failures: list[str] = []

    if not GENERATOR.exists():
        print(f"FAIL: generator not found at {GENERATOR}", file=sys.stderr)
        return 1

    with tempfile.NamedTemporaryFile(suffix=".md", mode="w", delete=False) as tmp:
        output_path = tmp.name

    try:
        result = run_generator(output_path)
        if result.returncode != 0:
            print(f"FAIL: generator exited {result.returncode}", file=sys.stderr)
            print(result.stderr, file=sys.stderr)
            return 1

        content = pathlib.Path(output_path).read_text()

        for section in REQUIRED_SECTIONS:
            try:
                assert_section_present(content, section)
            except AssertionError as exc:
                failures.append(str(exc))

        for fragment in REQUIRED_CONTENT:
            try:
                assert_content_present(content, fragment)
            except AssertionError as exc:
                failures.append(str(exc))

        # Idempotency: run again and verify byte-identical output
        result2 = run_generator(output_path)
        if result2.returncode != 0:
            failures.append(f"Second run (idempotency check) failed: {result2.stderr}")
        else:
            content2 = pathlib.Path(output_path).read_text()
            if content != content2:
                failures.append("Idempotency violated: second run produced different output")

    finally:
        os.unlink(output_path)

    if failures:
        print(f"FAIL: {len(failures)} assertion(s) failed:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        return 1

    print(f"PASS: all {len(REQUIRED_SECTIONS)} sections and {len(REQUIRED_CONTENT)} content fragments verified; idempotency confirmed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
