#!/usr/bin/env python3
"""
fetch-problem-statement.py — Fetch a SWE-bench Lite problem statement by instance ID.

Prints the problem statement to stdout so the runner can inject it into the
Claude Code prompt. Exits non-zero if the instance is not found.

Usage:
    python3 fetch-problem-statement.py <instance_id>

Example:
    python3 fetch-problem-statement.py psf__requests-863

The instance ID format is: <org>__<repo>-<issue_number>
(e.g. psf__requests-863, pallets__flask-4045, sympy__sympy-20590)

Data source: HuggingFace datasets server (princeton-nlp/SWE-bench_Lite, test split).
No API key required; uses the public datasets-server endpoint.
"""

import json
import sys
import urllib.request
import urllib.error

_HF_ROWS_URL = (
    "https://datasets-server.huggingface.co/rows"
    "?dataset=princeton-nlp%2FSWE-bench_Lite"
    "&config=default"
    "&split=test"
    "&offset={offset}"
    "&limit=100"
)
_HEADERS = {"User-Agent": "dynamic-agent-skill-layer/swebench-spike"}
_MAX_OFFSET = 300


def fetch_problem_statement(instance_id: str) -> str:
    """
    Fetches the problem_statement for the given SWE-bench Lite instance_id from
    the HuggingFace datasets server (test split). Scans pages of 100 rows until found.

    Raises SystemExit(1) with an error message if the instance is not found or the
    network request fails — never returns an empty string or a fake value.
    """
    for offset in range(0, _MAX_OFFSET + 1, 100):
        url = _HF_ROWS_URL.format(offset=offset)
        req = urllib.request.Request(url, headers=_HEADERS)
        try:
            with urllib.request.urlopen(req, timeout=20) as resp:
                data = json.loads(resp.read())
        except urllib.error.URLError as exc:
            print(
                f"ERROR: network request failed at offset={offset}: {exc}",
                file=sys.stderr,
            )
            sys.exit(1)

        rows = data.get("rows", [])
        if not rows:
            break

        for row in rows:
            r = row.get("row", {})
            if r.get("instance_id") == instance_id:
                statement = r.get("problem_statement", "").strip()
                if not statement:
                    print(
                        f"ERROR: instance {instance_id!r} found but problem_statement is empty",
                        file=sys.stderr,
                    )
                    sys.exit(1)
                return statement

    print(
        f"ERROR: instance {instance_id!r} not found in SWE-bench Lite test split "
        f"(searched {_MAX_OFFSET} rows). "
        "Check the instance ID format: <org>__<repo>-<issue>",
        file=sys.stderr,
    )
    sys.exit(1)


def main() -> None:
    if len(sys.argv) != 2 or sys.argv[1] in ("-h", "--help"):
        print(__doc__)
        sys.exit(0 if sys.argv[1:] else 1)

    instance_id = sys.argv[1]
    statement = fetch_problem_statement(instance_id)
    print(statement)


if __name__ == "__main__":
    main()
