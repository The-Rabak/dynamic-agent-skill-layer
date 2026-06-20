#!/usr/bin/env python3
"""T15 Phase-0 — verbose compile_context retrieval de-risk (FREE, no solves).

Risk #1 for the SWE-bench compounding experiment: the TREAT arm injects skills at
SessionStart using the FULL (verbose) issue/problem statement as the retrieval query.
T18 measured the verbose substratum as the WEAKEST priming case (coverage@3 0.027 vs
0.110 thin). If compile_context craters on verbose queries EVEN WHEN topically-relevant
skills exist in the corpus, the TREAT arm retrieves nothing and T15 would measure
through a broken priming path.

This probe answers, against the LIVE 277-skill corpus, for free:
  - On realistic VERBOSE GitHub-issue-style problem statements whose intent maps to
    known rich corpus skills, does compile_context (trigger=session_start) return
    status=ok with TOPICALLY-RELEVANT skills, or no_match/dilution?
  - How much worse is verbose vs a thin one-line version of the same intent?

Each probe pins `expected` = corpus skill-name fragments a correct retrieval should
surface. We report raw injected names (eyeball relevance) + a strict expected-hit.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from t12_priming_sweep import call_compile_context  # noqa: E402

# Each: verbose (multi-paragraph, SWE-bench-issue-shaped) + thin (one line) + expected
# corpus skill-name fragments a relevant retrieval should surface.
PROBES = [
    {
        "id": "silent-fallback",
        "expected": ["fail-loud", "backend-selector", "silently-default"],
        "thin": "A retrieval backend silently returns an empty result instead of failing loud when misconfigured.",
        "verbose": (
            "## Bug: misconfigured retrieval backend silently returns empty results\n\n"
            "When the `RETRIEVAL_BACKEND` environment variable is set to an unknown value, the "
            "service does not error. Instead it falls through a `match` arm into a default branch "
            "that returns an empty candidate set, so every query comes back as `no_match` with no "
            "indication that the backend was never wired. This masked a production outage for hours "
            "because `/health` still reported healthy.\n\n"
            "Expected behavior: an unknown or unset backend selector should fail loudly at "
            "construction time (or at the seam) rather than passing through to a silent empty/default "
            "path. We keep finding variants of this — a function returns `Ok(empty)` or "
            "`unwrap_or_default()` where it should have surfaced the gap.\n\n"
            "Steps to reproduce: set RETRIEVAL_BACKEND=nonsense, start the server, issue any "
            "find_skill query, observe a successful empty response instead of a startup failure."
        ),
    },
    {
        "id": "llm-malformed-json",
        "expected": ["thinking-model", "structured-json", "json-fence", "extraction-test", "substantive-fixture"],
        "thin": "An LLM extraction call intermittently returns malformed JSON and drops required fields.",
        "verbose": (
            "## Extraction intermittently produces malformed JSON / drops required fields\n\n"
            "The session-extractor calls a local model to emit a structured skill object. On larger "
            "transcript windows the model occasionally returns JSON with the chain-of-thought leaked "
            "into the keys, or a truncated body with required fields missing, which our parser then "
            "silently defaults to empty. Downstream this yields zero candidates with no error.\n\n"
            "We need the structured-output call to be robust: disable the model's thinking/leak, force "
            "a strict structured response, strip any code-fence wrapping the JSON, and fail loud on a "
            "parse miss rather than serde-defaulting required fields to empty. Smaller inputs never "
            "malform — only large ones — which suggests a context/truncation interaction.\n\n"
            "Acceptance: extraction over a substantive fixture returns a well-formed object with all "
            "required fields, and a malformed model body is surfaced (not swallowed)."
        ),
    },
    {
        "id": "postgres-volume-missing-db",
        "expected": ["postgres-docker-stale-volume", "postgres-volume-reuse", "missing-database"],
        "thin": "After a docker compose recreate the Postgres database is empty / missing.",
        "verbose": (
            "## After `docker compose up --force-recreate`, the Postgres database is gone\n\n"
            "Our compose stack mounts a named volume for Postgres, but after recreating the container "
            "the application connects to an empty cluster — all tables missing, `graph_version` 0, "
            "every query returns no_match. The real data cluster appears to live in a per-container "
            "anonymous volume that gets orphaned on recreate, while the named volume we mounted is at "
            "the wrong path for this Postgres image's PGDATA.\n\n"
            "We need the volume mount to point at the directory this image actually uses for PGDATA so "
            "the cluster persists across `--force-recreate`, and the service should fail loud (not "
            "serve an empty DB) when it connects to a database with no expected schema.\n\n"
            "Reproduce: docker compose up -d, ingest data, docker compose up -d --force-recreate "
            "postgres, observe the corpus is empty."
        ),
    },
    {
        "id": "migration-unwired",
        "expected": ["migration-file-unwired", "check-migration-slot", "orphaned-migration", "dead-schema-migration"],
        "thin": "A new SQL migration file exists on disk but never runs because it is not registered.",
        "verbose": (
            "## New migration never applies — schema column missing at runtime\n\n"
            "I added a new migration `012_add_skeleton_flag.sql` to the migrations directory, but the "
            "column never appears in the live database and the app fails at runtime querying it. It "
            "turns out migrations are not directory-scanned — they are a compile-time array in "
            "`postgres.rs`, and the new file was never added to that registry, so it silently does "
            "nothing.\n\n"
            "We need to detect an orphaned/unwired migration file (present on disk but absent from the "
            "registry) and block on it, and confirm the next free migration slot before authoring so "
            "two migrations don't collide on the same number.\n\n"
            "Acceptance: an unwired migration file is detected and fails the build/test rather than "
            "silently skipping."
        ),
    },
]


def judge(injected, expected):
    """Strict expected-hit: any injected name containing any expected fragment."""
    hits = [n for n in injected if any(frag in n for frag in expected)]
    return hits


def run():
    print(f"{'probe':<26} {'mode':<8} {'status':<10} {'lat':>5}  {'#inj':>4}  expected-hit")
    print("-" * 100)
    any_verbose_fail = False
    rows = []
    for p in PROBES:
        for mode in ("thin", "verbose"):
            status, injected, lat = call_compile_context(p[mode], with_trigger=True)
            hits = judge(injected, p["expected"])
            hit_str = ",".join(h[:34] for h in hits) if hits else "—— NONE ——"
            print(f"{p['id']:<26} {mode:<8} {status:<10} {lat:>5}  {len(injected):>4}  {hit_str}")
            rows.append({"id": p["id"], "mode": mode, "status": status, "lat": lat,
                         "injected": injected, "hits": hits})
            if mode == "verbose" and not hits:
                any_verbose_fail = True
        # show raw verbose injections for eyeballing relevance
        vrow = [r for r in rows if r["id"] == p["id"] and r["mode"] == "verbose"][0]
        print(f"    verbose injected: {vrow['injected']}")
        print()

    n = len(PROBES)
    v_hit = sum(1 for p in PROBES
                if judge([r for r in rows if r["id"] == p["id"] and r["mode"] == "verbose"][0]["injected"], p["expected"]))
    t_hit = sum(1 for p in PROBES
                if judge([r for r in rows if r["id"] == p["id"] and r["mode"] == "thin"][0]["injected"], p["expected"]))
    print("=" * 100)
    print(f"VERBOSE expected-hit: {v_hit}/{n}   THIN expected-hit: {t_hit}/{n}")
    print(f"GATE (risk #1 retired iff verbose retrieves a relevant skill on a majority): "
          f"{'PASS' if v_hit >= (n + 1) // 2 else 'FAIL — verbose dilution kills TREAT-arm retrieval'}")
    return 0


if __name__ == "__main__":
    sys.exit(run())
