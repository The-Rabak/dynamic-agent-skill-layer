#!/usr/bin/env python3
"""Retrieval-quality tuning sweep over the REAL running mcp-server (#210).

For each config, this reboots ONLY the mcp-server container with the config's
RETRIEVAL_* env overrides (RetrievalConfig::from_env), waits until it has
re-embedded the real 234-corpus and is serving, then measures quality by
driving the real server over HTTP (scripts/retrieval_quality_live.py). NO
in-process reconstruction; every config is a fully-booted real server.

Method (binding decisions):
  - Tune on the TUNING split only; the winner is validated on the disjoint
    HELD-OUT split. Target is FROZEN before the sweep: judge-augmented
    MRR >= 0.80, nDCG@3 >= 0.80, no_match precision >= 0.90.
  - Winner selected by judge-augmented tuning MRR (tie-break nDCG@3). No weight
    chosen to pass a single query.
  - The LLM-judge verdict cache is shared across configs (judging drains the
    union pool; cached pairs are never re-judged). No caps.
"""
import json
import os
import re
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

COMPOSE = ["docker", "compose", "-f", "docker-compose.test.yml"]
MCP_URL = "http://127.0.0.1:3001/mcp"
REPORT_DIR = Path("tests/e2e/reports/sweep")
WARMUP_PROMPT = "conventional commits with co-authored-by trailer"  # a known corpus topic

# Qdrant REST base URL — read from the same env var the e2e harness exports so
# local and CI environments agree without hardcoding.
_QDRANT_HTTP_PORT = int(os.environ.get("QDRANT_HTTP_PORT", "16333"))
QDRANT_REST_URL = f"http://127.0.0.1:{_QDRANT_HTTP_PORT}"

# Ranking levers already recognised by RetrievalConfig::from_env.
_RETRIEVAL_ENV_KEYS = [
    "RETRIEVAL_ALPHA", "RETRIEVAL_BETA", "RETRIEVAL_GAMMA", "RETRIEVAL_LAMBDA",
    "RETRIEVAL_MMR_LAMBDA", "RETRIEVAL_CANDIDATE_LIMIT", "RETRIEVAL_MAX_RESULTS",
    "RETRIEVAL_MAX_SUBUNITS_PER_SKILL", "RETRIEVAL_RESCUE_THRESHOLD",
    "RETRIEVAL_RELEVANCE_THRESHOLD", "RETRIEVAL_PROJECT_SCOPE_WEIGHT",
    "RETRIEVAL_GLOBAL_SCOPE_WEIGHT", "RETRIEVAL_RRF_K",
]

# V1.7 arm-selection env vars.  These mirror the env surface the real server
# reads (or will read once T02/T04/T07 wire them):
#   OLLAMA_EMBED_MODEL  — embedding model name (T02: qwen3-embedding:4b)
#   RETRIEVAL_BACKEND   — candidate generation backend (T04: qdrant_hybrid)
#   RETRIEVAL_SPARSE    — BM25/sparse flag (T04: true)
#   RETRIEVAL_RERANK    — local reranker flag (T07: true)
_ARM_ENV_KEYS = [
    "OLLAMA_EMBED_MODEL",
    "RETRIEVAL_BACKEND",
    "RETRIEVAL_SPARSE",
    "RETRIEVAL_RERANK",
]

ENV_KEYS = _RETRIEVAL_ENV_KEYS + _ARM_ENV_KEYS

# Default α=0.45 β=0.35 γ=0.20 λ=0.25, mmr=0.65, cand=50, subunits=3, floor=0.450.
# Each config overrides one or a few levers from default; "default" overrides nothing
# (also the faithfulness check — must reproduce the pre-sweep baseline).
#
# V1.7 arm configs are included here so every sweep produces a labelled baseline
# row alongside the parameter-tuning rows.  The v1.7-baseline arm overrides nothing
# (it is identical to "default") but carries an explicit label that downstream T02/T04/T07
# reports can compare against.  Experimental arms (qwen/hybrid/rerank) will add config
# rows here once the server wires the corresponding env vars in T02/T04/T07.
CONFIGS = [
    # ── parameter-tuning configs (existing) ──────────────────────────────────
    ("default",            {}),
    ("lambda0",            {"RETRIEVAL_LAMBDA": "0.0"}),                                   # #208: graph off
    ("beta_heavy",         {"RETRIEVAL_ALPHA": "0.40", "RETRIEVAL_BETA": "0.45", "RETRIEVAL_GAMMA": "0.15"}),
    ("alpha_heavy",        {"RETRIEVAL_ALPHA": "0.60", "RETRIEVAL_BETA": "0.30", "RETRIEVAL_GAMMA": "0.10"}),
    ("lambda0_beta_heavy", {"RETRIEVAL_LAMBDA": "0.0", "RETRIEVAL_ALPHA": "0.40", "RETRIEVAL_BETA": "0.45", "RETRIEVAL_GAMMA": "0.15"}),
    ("subunit_deep",       {"RETRIEVAL_MAX_SUBUNITS_PER_SKILL": "5", "RETRIEVAL_BETA": "0.45", "RETRIEVAL_ALPHA": "0.40", "RETRIEVAL_GAMMA": "0.15"}),
    ("mmr_relevance",      {"RETRIEVAL_MMR_LAMBDA": "0.85"}),
    ("candidate_wide",     {"RETRIEVAL_CANDIDATE_LIMIT": "100"}),
    # ── V1.7 arm baselines ────────────────────────────────────────────────────
    # v1.7-baseline: current default arm; no env overrides.  This row is the
    # "current production default" reference that all later V1.7 arm comparisons
    # use.  backend=snapshot_dense, embedder=nomic-embed-text, sparse=off, rerank=off.
    # (Arm metadata is automatically read from env / ARM_METADATA_DEFAULTS in the
    # live script, so this row produces a fully-labelled arm block in its report.)
    # NOTE(#237): "default" and "v1.7-baseline" both carry no overrides and are
    # therefore identical in state.  This is a DELIBERATE consistency check: "default"
    # is the historical parameter-tuning reference; "v1.7-baseline" is the labelled
    # V1.7 arm baseline.  Running both confirms they reproduce the same numbers and
    # gives downstream T02/T04/T07 reports a stable arm label to compare against.
    # Do NOT delete either row without noting this intent; do NOT merge them into one
    # until arm-labelled reports replace the raw tuning table entirely.
    ("v1.7-baseline",      {}),
    # !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
    # WARNING — DO NOT UNCOMMENT THESE ROWS until T04/T07 are complete:
    #
    # Switching OLLAMA_EMBED_MODEL (qwen arm) or enabling a new backend means
    # graph-builder must rebuild the WRITE SIDE into that arm's collection
    # (skills__<model-slug>) BEFORE mcp-server is rebooted and BEFORE measure()
    # is called.  reboot_mcp() restarts mcp-server ONLY — graph-builder is never
    # restarted here, so the arm's collection is empty or holds vectors from a
    # prior manual run.  Measuring against an empty/stale collection yields
    # phantom numbers that are unattributable and silently violate the
    # "honest arm comparison" purpose of this sweep.
    #
    # The full fix (reboot_arm: restart graph-builder + poll for rebuild + then
    # reboot mcp-server) is a hard prerequisite INSIDE the T04 ticket (#243).
    # Do not bypass it by manually pre-populating the collection: the arm must
    # be self-contained and reproducible from a clean state.
    # !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
    # ("v1.7-qwen4b",   {"OLLAMA_EMBED_MODEL": "qwen3-embedding:4b"}),            # T02 — needs reboot_arm (T04/#243)
    # ("v1.7-hybrid",   {"RETRIEVAL_BACKEND": "qdrant_hybrid", "RETRIEVAL_SPARSE": "true"}),  # T04 — needs reboot_arm (T04/#243)
    # ("v1.7-rerank",   {"RETRIEVAL_BACKEND": "qdrant_hybrid", "RETRIEVAL_SPARSE": "true", "RETRIEVAL_RERANK": "true"}),  # T07 — needs reboot_arm (T04/#243)
    # TODO(T04): convert results tuple to a dataclass when arm grows beyond 8 fields;
    # currently destructured positionally in 3 places (results.append, tuning_results
    # filter, and the print/summary loops) — see #242 item 5.
]
LIMIT = 5  # find_skill depth for MRR (top-k injection is K=3)


def set_env(overrides: dict):
    for k in ENV_KEYS:
        os.environ.pop(k, None)
    for k, v in overrides.items():
        os.environ[k] = v


def _model_keyed_collection_name(model: str) -> str:
    """Derive the Qdrant collection name for an embedding model.

    Mirrors the Rust ``model_keyed_collection_name`` in
    ``crates/infrastructure/src/vector/qdrant.rs``:
      - lowercased
      - every non-alphanumeric / non-hyphen char → hyphen
      - consecutive hyphens collapsed
      - prefixed with ``skills__``

    Examples:
      "nomic-embed-text"   → "skills__nomic-embed-text"
      "qwen3-embedding:4b" → "skills__qwen3-embedding-4b"
    """
    slug = re.sub(r"[^a-z0-9-]", "-", model.lower())
    slug = re.sub(r"-+", "-", slug).strip("-")
    return f"skills__{slug}"


def assert_collection_nonempty(overrides: dict) -> None:
    """Fail loud if the target Qdrant collection for this arm contains 0 points.

    A zero-point collection means graph-builder has never (or not yet) written
    vectors into this arm's collection.  Measuring retrieval quality against an
    empty collection produces phantom numbers that are entirely unattributable —
    they silently violate the "honest arm comparison" contract of this sweep.

    The target collection is derived from OLLAMA_EMBED_MODEL (or its override)
    using the same slug logic as the Rust ``model_keyed_collection_name``
    function.  QDRANT_COLLECTION env var overrides take precedence, mirroring
    mcp-server runtime behaviour.

    Raises SystemExit(1) if the collection is absent or empty, or if Qdrant
    cannot be reached (unreachable Qdrant is itself a fatal sweep precondition
    failure).
    """
    embed_model = overrides.get(
        "OLLAMA_EMBED_MODEL",
        os.environ.get("OLLAMA_EMBED_MODEL", "nomic-embed-text"),
    )
    # QDRANT_COLLECTION env override mirrors mcp-server runtime behaviour.
    collection = os.environ.get("QDRANT_COLLECTION") or _model_keyed_collection_name(embed_model)
    url = f"{QDRANT_REST_URL}/collections/{collection}"
    try:
        with urllib.request.urlopen(url, timeout=10) as resp:
            info = json.loads(resp.read())
    except Exception as exc:
        print(
            f"\nFATAL: cannot reach Qdrant at {url} — is the stack running?\n"
            f"  error: {exc}\n"
            f"  Fix: docker compose -f docker-compose.test.yml up -d qdrant",
            flush=True,
        )
        sys.exit(1)

    vectors_count = info.get("result", {}).get("vectors_count", 0) or 0
    points_count = info.get("result", {}).get("points_count", 0) or 0
    # Qdrant ≥1.7 uses ``points_count``; older versions used ``vectors_count``.
    total = max(vectors_count, points_count)
    if total == 0:
        print(
            f"\nFATAL: Qdrant collection '{collection}' has 0 points for arm "
            f"embedder='{embed_model}'.\n"
            f"  Measuring retrieval against an empty collection produces phantom\n"
            f"  numbers — this is NOT a valid arm comparison.\n"
            f"\n"
            f"  Root cause: graph-builder has not yet written vectors into this\n"
            f"  collection.  reboot_mcp() restarts mcp-server ONLY; graph-builder\n"
            f"  must also be rebooted and allowed to complete a rebuild cycle into\n"
            f"  '{collection}' before measurement.\n"
            f"\n"
            f"  Fix (T04/#243): implement reboot_arm() that restarts graph-builder\n"
            f"  + polls for rebuild completion before rebooting mcp-server.\n"
            f"  Do NOT manually pre-populate and re-run — arms must be reproducible\n"
            f"  from a clean state.",
            flush=True,
        )
        sys.exit(1)


def reboot_mcp():
    # WARNING (#237): this restarts mcp-server ONLY.  graph-builder is NOT
    # restarted here.  This is safe for arms that share the same embedder model
    # (nomic-embed-text, the current default) because graph-builder has already
    # populated skills__nomic-embed-text.  It is NOT safe for arms that change
    # OLLAMA_EMBED_MODEL (e.g. qwen3-embedding:4b) or that require a different
    # Qdrant collection — those arms need reboot_arm() (T04/#243) which restarts
    # graph-builder, polls for rebuild completion, and THEN reboots mcp-server.
    # The pre-measure assert_collection_nonempty() guard will catch any arm that
    # reaches measure() against an empty collection and fail loud before reporting.
    subprocess.run(COMPOSE + ["up", "-d", "--no-deps", "--force-recreate", "mcp-server"],
                   check=True, capture_output=True, text=True)


def warmup_query():
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": {"name": "find_skill",
                                  "arguments": {"prompt": WARMUP_PROMPT, "limit": 3}}}).encode()
    req = urllib.request.Request(MCP_URL, data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        r = json.loads(resp.read())
    return r.get("result", {}).get("matches", [])


def wait_ready(deadline_s: int = 600):
    """Poll until the rebooted server has re-embedded the corpus and serves a
    known query. Fail loud only on a real stuck state (deadline), per the
    no-arbitrary-caps rule (the deadline is a stuck-detector, not a work cap)."""
    start = time.time()
    while time.time() - start < deadline_s:
        try:
            if warmup_query():
                return
        except Exception:
            pass
        time.sleep(3)
    raise RuntimeError(f"mcp-server did not serve the warmup query within {deadline_s}s after reboot")


def measure(label: str, split: str, gate: bool = False) -> dict:
    out = REPORT_DIR / f"{label}__{split}.json"
    cmd = [sys.executable, "scripts/retrieval_quality_live.py",
           "--split", split, "--config-label", label, "--limit", str(LIMIT),
           "--out", str(out),
           "--verdict-cache", "tests/e2e/reports/retrieval_234_live_verdicts.json"]
    if gate:
        cmd.append("--gate")
    res = subprocess.run(cmd, text=True)
    report = json.loads(out.read_text())
    report["_gate_exit"] = res.returncode
    return report


def main():
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    results = []
    for label, overrides in CONFIGS:
        print(f"\n########## CONFIG: {label}  {overrides or '(default)'} ##########", flush=True)
        set_env(overrides)
        reboot_mcp()
        wait_ready()
        assert_collection_nonempty(overrides)
        rep = measure(label, "tuning")
        ja = rep["judge_augmented"]
        arm = rep.get("arm", {})
        lat = rep.get("latency_ms", {})
        results.append((label, overrides, ja["mrr"], ja["ndcg_at_3"], ja["p_at_1"],
                        ja["hit_at_3"], arm, lat))
        print(f"  arm: backend={arm.get('backend')}  embedder={arm.get('embedder_model')}  "
              f"sparse={arm.get('sparse')}  rerank={arm.get('rerank')}", flush=True)
        print(f"  latency: mean={lat.get('mean')}ms  p95={lat.get('p95')}ms", flush=True)
        print(f"  tuning judge-aug: MRR={ja['mrr']:.3f} nDCG@3={ja['ndcg_at_3']:.3f} "
              f"P@1={ja['p_at_1']:.3f} hit@3={ja['hit_at_3']:.3f}", flush=True)

    print("\n=== TUNING SWEEP (judge-augmented) ===")
    print(f"{'config':22s} {'MRR':>7s} {'nDCG@3':>7s} {'P@1':>7s} {'hit@3':>7s} {'p95ms':>7s} {'backend'}")
    for label, _, mrr, ndcg, p1, hit, arm, lat in results:
        p95 = lat.get("p95", 0.0)
        backend = arm.get("backend", "?")
        print(f"{label:22s} {mrr:>7.3f} {ndcg:>7.3f} {p1:>7.3f} {hit:>7.3f} {p95:>7.1f} {backend}")

    # Winner selection excludes V1.7 arm rows — those are reference baselines,
    # not parameter-tuning candidates.  The winner is the best-MRR config among
    # non-arm rows (those whose labels don't start with "v1.7-").
    tuning_results = [(l, o, m, n, p, h, a, lat) for l, o, m, n, p, h, a, lat in results
                      if not l.startswith("v1.7-")]
    winner = max(tuning_results, key=lambda r: (r[2], r[3]))  # MRR, tie-break nDCG@3
    w_label, w_overrides = winner[0], winner[1]
    print(f"\nWINNER (tuning): {w_label}  {w_overrides or '(default)'}  MRR={winner[2]:.3f}")

    print(f"\n=== VALIDATING WINNER ON HELD-OUT: {w_label} ===")
    set_env(w_overrides)
    reboot_mcp()
    wait_ready()
    assert_collection_nonempty(w_overrides)
    held = measure(f"{w_label}-WINNER", "held_out", gate=True)
    hja = held["judge_augmented"]
    held_arm = held.get("arm", {})
    held_lat = held.get("latency_ms", {})
    print(f"held-out arm: backend={held_arm.get('backend')}  embedder={held_arm.get('embedder_model')}  "
          f"sparse={held_arm.get('sparse')}  rerank={held_arm.get('rerank')}")
    print(f"held-out latency: mean={held_lat.get('mean')}ms  p95={held_lat.get('p95')}ms")
    print(f"held-out judge-aug: MRR={hja['mrr']:.3f} nDCG@3={hja['ndcg_at_3']:.3f} "
          f"P@1={hja['p_at_1']:.3f} hit@3={hja['hit_at_3']:.3f} "
          f"no_match_prec={held['no_match_precision']}")

    summary = dict(
        configs=[
            dict(label=l, overrides=o, arm=a, tuning_mrr=m, tuning_ndcg=n,
                 tuning_p1=p, tuning_hit3=h,
                 latency_ms=lat)
            for l, o, m, n, p, h, a, lat in results
        ],
        winner=dict(label=w_label, overrides=w_overrides),
        winner_held_out=held,
        target=held["target"],
    )
    Path("tests/e2e/reports/retrieval_234_sweep_summary.json").write_text(json.dumps(summary, indent=1))
    print("\nsweep summary: tests/e2e/reports/retrieval_234_sweep_summary.json")
    print(f"gate exit for winner held-out: {held['_gate_exit']} (0=meets target, 1=below)")


if __name__ == "__main__":
    main()
