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
    # ── V1.7 arm candidates (T04) ─────────────────────────────────────────────
    # These arms change RETRIEVAL_BACKEND and/or OLLAMA_EMBED_MODEL.  They use
    # reboot_arm() (implemented in T04-D) rather than reboot_mcp(): reboot_arm
    # restarts graph-builder → polls for rebuild → restarts mcp-server → warms up.
    # The arm label prefix "v1.7-" excludes them from parameter-tuning winner
    # selection (see the tuning_results filter below).
    #
    # snapshot_hybrid: in-memory dense cosine (same read path as snapshot_dense)
    # but graph-builder also writes BM25 sparse vectors into skills__nomic-embed-text__hybrid.
    # The mcp-server snapshot_dense backend reads only from the in-memory pool-union;
    # the hybrid Qdrant collection is write-only from mcp-server's perspective (CQRS intact).
    #
    # qdrant_hybrid: Qdrant becomes the READ PATH (CQRS break vs snapshot_dense).
    # mcp-server queries skills__nomic-embed-text__hybrid for dense+sparse fusion.
    # This is the T04 measurement arm; T08 decides promotion based on this evidence.
    #
    # NOTE(T02/qwen4b): qwen arm omitted from this sweep — qwen3-embedding:4b requires
    # a separate collection (skills__qwen3-embedding-4b) that is currently empty (0 pts).
    # Populating it needs a full 234-skill rebuild with the qwen embedder (~minutes on GPU).
    # Include once T02 arm collection is confirmed non-empty; see #243 for tracking.
    ("v1.7-snapshot-hybrid", {"RETRIEVAL_BACKEND": "snapshot_hybrid", "RETRIEVAL_SPARSE": "true"}),  # T04-D
    ("v1.7-qdrant-hybrid",   {"RETRIEVAL_BACKEND": "qdrant_hybrid",   "RETRIEVAL_SPARSE": "true"}),  # T04-D
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


def _target_collection_for_arm(overrides: dict) -> str:
    """Derive the Qdrant collection name that the arm's write path targets.

    For snapshot_dense and snapshot_hybrid arms the write side always uses the
    dense-keyed collection (``skills__<model-slug>``); the read side for these
    arms is the in-memory RetrievalSnapshot, not Qdrant.

    For qdrant_hybrid the read path uses the hybrid-keyed collection
    (``skills__<model-slug>__hybrid``), so that is the collection we must poll
    to confirm the rebuild populated it before measure() runs.

    Args:
        overrides: the arm's env-override dict from CONFIGS.

    Returns:
        The Qdrant collection name that will contain vectors after a successful
        rebuild for this arm.
    """
    embed_model = overrides.get(
        "OLLAMA_EMBED_MODEL",
        os.environ.get("OLLAMA_EMBED_MODEL", "nomic-embed-text"),
    )
    backend = overrides.get("RETRIEVAL_BACKEND", "")
    if backend == "qdrant_hybrid":
        slug = re.sub(r"[^a-z0-9-]", "-", embed_model.lower())
        slug = re.sub(r"-+", "-", slug).strip("-")
        return f"skills__{slug}__hybrid"
    return _model_keyed_collection_name(embed_model)


def _delete_qdrant_collection(collection: str) -> None:
    """Delete a Qdrant collection so a subsequent rebuild starts from 0 points.

    graph-builder recreates the collection on startup (``ensure_hybrid_collection``
    is idempotent). Deleting first guarantees that the rebuild-completion poll
    (``_poll_collection_until_nonempty``) measures a FRESH rebuild, not stale
    points from a previous arm run that happen to satisfy the threshold.

    Raises:
        SystemExit(1): if the DELETE request fails or Qdrant is unreachable.
    """
    import urllib.request
    req = urllib.request.Request(
        f"{QDRANT_REST_URL}/collections/{collection}",
        method="DELETE",
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            body = json.loads(resp.read())
        if body.get("result") is True or body.get("status") == "ok":
            print(f"  reboot_arm: deleted hybrid collection '{collection}'", flush=True)
        else:
            print(f"  reboot_arm: hybrid collection delete returned unexpected body: {body}", flush=True)
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            # Collection did not exist — nothing to delete, that's fine.
            print(f"  reboot_arm: hybrid collection '{collection}' did not exist (nothing to delete)", flush=True)
        else:
            print(
                f"\nFATAL: could not delete Qdrant collection '{collection}': {exc}\n"
                f"  Fix: is Qdrant running at {QDRANT_REST_URL}?",
                flush=True,
            )
            sys.exit(1)
    except Exception as exc:
        print(
            f"\nFATAL: could not delete Qdrant collection '{collection}': {exc}\n"
            f"  Fix: is Qdrant running at {QDRANT_REST_URL}?",
            flush=True,
        )
        sys.exit(1)


def _poll_collection_until_nonempty(collection: str, rebuild_poll_interval_s: int = 10,
                                    stuck_deadline_s: int = 1800,
                                    min_points: int = 1) -> None:
    """Block until the Qdrant collection accumulates at least ``min_points`` points.

    This is the rebuild-completion signal for the qdrant_hybrid arm: graph-builder
    writes sparse+dense vectors to the hybrid collection only when
    RETRIEVAL_BACKEND=qdrant_hybrid and the outbox events carry a ``sparse`` field.
    A point count >= min_points in the hybrid collection means the rebuild and relay
    completed for at least that many skills.

    The stuck_deadline_s is a STUCK detector, NOT a work cap.  It fires only
    when the collection stays below min_points for the entire window, which indicates
    graph-builder has stalled (e.g. Ollama or Qdrant unreachable).  A healthy
    but slow rebuild (large corpus, slow GPU) will simply keep polling.  Do not
    lower this value to cap legitimate work (project memory: no-arbitrary-limits).

    NOTE: This function is ONLY appropriate for the qdrant_hybrid arm where Qdrant
    is the read path.  For snapshot arms (snapshot_dense, snapshot_hybrid), the
    mcp-server reads from an in-memory snapshot loaded from PG, not from Qdrant.
    For those arms, use ``wait_ready()`` as the rebuild-completion signal instead
    — the in-memory snapshot is populated by graph-builder's Redis graph.rebuilt
    event, not by Qdrant point count.

    Args:
        collection: Qdrant collection name to poll (must be the hybrid collection).
        rebuild_poll_interval_s: seconds between Qdrant REST probes.
        stuck_deadline_s: fail loud if < min_points for longer than this (stuck detector).
        min_points: minimum point count to consider rebuild complete (default 1).
            Use 30 for the qdrant_hybrid arm to guard against partial relay completion.

    Raises:
        RuntimeError: if stuck_deadline_s elapses with < min_points (real stuck state only).
        SystemExit(1): if Qdrant is unreachable (fatal precondition failure).
    """
    url = f"{QDRANT_REST_URL}/collections/{collection}"
    start = time.time()
    while True:
        elapsed = time.time() - start
        try:
            with urllib.request.urlopen(url, timeout=10) as resp:
                info = json.loads(resp.read())
            result = info.get("result", {})
            vectors_count = result.get("vectors_count", 0) or 0
            points_count = result.get("points_count", 0) or 0
            total = max(vectors_count, points_count)
            if total >= min_points:
                print(
                    f"  rebuild-poll: collection '{collection}' has {total} points "
                    f"(≥ {min_points} threshold) after {elapsed:.0f}s — rebuild complete",
                    flush=True,
                )
                return
            print(
                f"  rebuild-poll: collection '{collection}' has {total} points "
                f"(need {min_points}; {elapsed:.0f}s elapsed); waiting for rebuild ...",
                flush=True,
            )
        except Exception as exc:
            if elapsed > stuck_deadline_s:
                raise RuntimeError(
                    f"Qdrant unreachable at {url} after {elapsed:.0f}s: {exc}"
                ) from exc
            print(f"  rebuild-poll: Qdrant probe failed ({exc}); retrying ...", flush=True)

        if elapsed > stuck_deadline_s:
            raise RuntimeError(
                f"STUCK: Qdrant collection '{collection}' had < {min_points} points for "
                f"{elapsed:.0f}s (deadline {stuck_deadline_s}s).  "
                f"graph-builder has not completed a rebuild into this collection.  "
                f"Check graph-builder logs for Ollama/Qdrant connectivity errors.  "
                f"This is a real stuck state, not a slow-but-healthy rebuild."
            )
        time.sleep(rebuild_poll_interval_s)


def _pg_purge_published_outbox_events() -> int:
    """Delete all published outbox events from the test Postgres database.

    Required for the qdrant_hybrid arm to force graph-builder to emit new
    ``vector.upsert`` events that carry the ``sparse`` field in their payload.

    The problem: the outbox uses a content-addressed idempotency key
    (``graph.rebuild:vector:<skill_id>``).  When skills are rebuilt, the key
    already exists as ``published`` (from a prior dense-only run), so the
    rebuild with ``RETRIEVAL_BACKEND=qdrant_hybrid`` skips event creation and
    the hybrid Qdrant collection never receives the sparse+dense vectors.

    Deleting ``published`` events is safe in this test environment because:
      1. The test Postgres DB is not shared with production.
      2. The outbox events are append-only artifacts — the published events
         represent work that has already been durably written to Qdrant;
         re-creating them just re-writes the same vectors (idempotent upsert).
      3. The rebuild that follows will create fresh events with the correct
         sparse payload for the qdrant_hybrid arm.

    This must only be called inside the sweep, not in any production path.

    Returns:
        The number of rows deleted.

    Raises:
        RuntimeError: if the psql command fails (e.g. Postgres unreachable).
    """
    # Use docker exec + psql so we don't need a pg client on the host.
    # The compose service name is 'postgres' and the test DB is 'skill_layer_test'.
    result = subprocess.run(
        ["docker", "exec", "dynamic-agent-skill-layer-postgres-1",
         "psql", "-U", "skill_layer", "-d", "skill_layer_test",
         "-c",
         "DELETE FROM outbox_events WHERE status = 'published';"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"Failed to purge published outbox events via psql: {result.stderr.strip()}"
        )
    # psql outputs "DELETE N" where N is the row count.
    stdout = result.stdout.strip()
    try:
        # stdout is like "DELETE 238"
        parts = stdout.split()
        deleted = int(parts[-1]) if parts and parts[0] == "DELETE" else 0
    except (ValueError, IndexError):
        deleted = 0
    return deleted


def reboot_arm(overrides: dict) -> None:
    """Restart graph-builder and mcp-server for the given arm, then wait for readiness.

    Implements the full arm-switch lifecycle required for any arm that changes
    the embedding model, the Qdrant collection target, or the backend.

    The rebuild-completion signal differs by arm type:

    **Snapshot arms** (snapshot_dense, snapshot_hybrid):
      mcp-server reads from an in-memory snapshot loaded from PG via the Redis
      ``graph.rebuilt`` event.  Qdrant is write-only for these arms (CQRS intact).
      The rebuild-completion signal is ``wait_ready()`` — mcp-server serving a
      known query confirms the 234-skill snapshot was applied.

    **qdrant_hybrid arm**:
      mcp-server queries the hybrid Qdrant collection for retrieval.  The rebuild-
      completion signal is the hybrid collection having ≥1 point.
      COMPLICATION: graph-builder uses a content-addressed idempotency key
      (``graph.rebuild:vector:<skill_id>``); if the prior run published dense-only
      events, the qdrant_hybrid rebuild skips event creation (ON CONFLICT DO NOTHING)
      and the hybrid collection never receives sparse+dense vectors.
      FIX: purge published outbox events before restarting graph-builder so the
      qdrant_hybrid rebuild creates fresh events with the ``sparse`` field.

    reboot_mcp() MUST NOT be used for these arms: it only restarts mcp-server and
    leaves graph-builder running with the prior arm's env, so the arm's collection
    may be stale.

    The 1800-second stuck detector in ``_poll_collection_until_nonempty`` is NOT a
    work cap.  A healthy rebuild of 234 skills over a slow local GPU may take many
    minutes; the deadline fires only when graph-builder is genuinely stalled with
    0 progress.

    Args:
        overrides: env-override dict from CONFIGS (e.g. {"RETRIEVAL_BACKEND": "qdrant_hybrid",
            "RETRIEVAL_SPARSE": "true"}).  Must already be applied to os.environ
            before calling (set_env(overrides) should run first).
    """
    backend = overrides.get("RETRIEVAL_BACKEND", "")
    is_qdrant_hybrid = backend == "qdrant_hybrid"
    target_collection = _target_collection_for_arm(overrides)
    print(
        f"  reboot_arm: backend={backend or '(default)'}  "
        f"target={target_collection!r}  "
        f"embedder={overrides.get('OLLAMA_EMBED_MODEL', '(default)')}",
        flush=True,
    )

    if is_qdrant_hybrid:
        # For qdrant_hybrid, purge published outbox events so graph-builder emits
        # new events with the ``sparse`` field that populate the hybrid collection.
        # Without this, the idempotency mechanism prevents new event creation
        # for skills already published as dense-only in a prior arm run.
        print(
            "  reboot_arm: purging published outbox events (qdrant_hybrid needs fresh "
            "sparse-capable events) ...",
            flush=True,
        )
        deleted = _pg_purge_published_outbox_events()
        print(f"  reboot_arm: deleted {deleted} published outbox events", flush=True)

        # Delete the hybrid collection so the rebuild starts from 0 points.
        # REQUIRED to prevent stale points from a previous run from satisfying the
        # rebuild-completion poll early before the fresh rebuild is done.
        # graph-builder recreates the collection on startup (ensure_hybrid_collection
        # is idempotent and safe to call on a non-existent collection).
        print(
            f"  reboot_arm: deleting hybrid collection '{target_collection}' "
            "(clean rebuild — stale points from a prior arm run must not pollute measurement) ...",
            flush=True,
        )
        _delete_qdrant_collection(target_collection)

    # Restart graph-builder.  The watcher starts with an empty previous_snapshot,
    # discovers all 234 skill files as additions, and triggers a full rebuild cycle.
    print("  reboot_arm: restarting graph-builder ...", flush=True)
    subprocess.run(
        COMPOSE + ["up", "-d", "--no-deps", "--force-recreate", "graph-builder"],
        check=True, capture_output=True, text=True,
    )

    if is_qdrant_hybrid:
        # For qdrant_hybrid, the rebuild writes sparse+dense vectors to the hybrid
        # collection.  Poll until the hybrid collection has enough points to confirm
        # a full rebuild cycle completed (not just the startup drain).
        # Minimum 30 points guards against the collection being satisfied by a
        # partial relay; the full corpus is 234 skills.
        print(
            f"  reboot_arm: polling Qdrant hybrid collection '{target_collection}' "
            f"for rebuild completion ...",
            flush=True,
        )
        _poll_collection_until_nonempty(target_collection, min_points=30)
    else:
        # For snapshot arms (snapshot_dense, snapshot_hybrid): the rebuild-completion
        # signal is wait_ready() after mcp-server restarts.  The mcp-server loads
        # the 234-skill in-memory snapshot from PG via the graph.rebuilt Redis event.
        # Qdrant dense collection is write-only and not the read-path for these arms.
        print(
            "  reboot_arm: snapshot arm — will wait for mcp-server readiness "
            "(Qdrant dense is write-only for this arm; in-memory snapshot from PG is the read path)",
            flush=True,
        )

    # Restart mcp-server with the same env overrides so it reads the correct arm.
    print("  reboot_arm: restarting mcp-server ...", flush=True)
    subprocess.run(
        COMPOSE + ["up", "-d", "--no-deps", "--force-recreate", "mcp-server"],
        check=True, capture_output=True, text=True,
    )

    # Warm up — poll until mcp-server responds to find_skill.
    # For snapshot arms: this also serves as the rebuild-completion signal.
    # For qdrant_hybrid: the Qdrant poll already confirmed rebuild; this confirms
    # the mcp-server connected to the populated hybrid collection.
    print("  reboot_arm: waiting for mcp-server readiness ...", flush=True)
    wait_ready()

    # Belt-and-suspenders guard before measure().
    # For qdrant_hybrid: checks the hybrid collection is non-empty.
    # For snapshot arms: skip Qdrant guard — these arms read from in-memory PG snapshot,
    # not from Qdrant; the dense collection may be empty due to the idempotency
    # mechanism (events already published in prior runs), which is correct behavior.
    if is_qdrant_hybrid:
        assert_collection_nonempty(overrides)


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


def _is_arm_config(label: str) -> bool:
    """Return True for V1.7 arm configs that need reboot_arm instead of reboot_mcp.

    V1.7 arm configs change RETRIEVAL_BACKEND or OLLAMA_EMBED_MODEL, which means
    graph-builder must rebuild into a different Qdrant collection before mcp-server
    is rebooted.  Parameter-tuning configs (lambda0, beta_heavy, etc.) only change
    ranking weights and are safe to hot-swap via reboot_mcp.
    """
    return label.startswith("v1.7-")


def main():
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    results = []
    for label, overrides in CONFIGS:
        print(f"\n########## CONFIG: {label}  {overrides or '(default)'} ##########", flush=True)
        set_env(overrides)
        # V1.7 arm configs change the backend or embedder model — they need a full
        # graph-builder restart so the arm's Qdrant collection is populated before
        # mcp-server serves.  Parameter-tuning configs only change ranking weights
        # and can safely hot-swap via reboot_mcp (no collection rebuild needed).
        if _is_arm_config(label):
            reboot_arm(overrides)
        else:
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
    print(f"{'config':24s} {'MRR':>7s} {'nDCG@3':>7s} {'P@1':>7s} {'hit@3':>7s} {'p95ms':>7s} {'backend'}")
    for label, _, mrr, ndcg, p1, hit, arm, lat in results:
        p95 = lat.get("p95", 0.0)
        backend = arm.get("backend", "?")
        print(f"{label:24s} {mrr:>7.3f} {ndcg:>7.3f} {p1:>7.3f} {hit:>7.3f} {p95:>7.1f} {backend}")

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

    # ── T04-D: per-arm held-out sweep ─────────────────────────────────────────
    # Measure all three V1.7 arms against the held-out split.  This is the
    # primary output of T04-D: honest, per-arm quality evidence over data not
    # seen during tuning.  snapshot_dense is the baseline; snapshot_hybrid and
    # qdrant_hybrid are the candidates.
    #
    # Report files follow the T01 format: tests/e2e/reports/v17-<arm>__held_out.json
    # Corpus-size guard: fail loud if < 30 held-out positives (the fixture has 40).
    V17_ARM_HELD_OUT_REPORT_DIR = Path("tests/e2e/reports")
    V17_ARM_HELD_OUT_REPORT_DIR.mkdir(parents=True, exist_ok=True)

    v17_arm_configs = [
        ("snapshot_dense",   {}),
        ("snapshot_hybrid",  {"RETRIEVAL_BACKEND": "snapshot_hybrid",  "RETRIEVAL_SPARSE": "true"}),
        ("qdrant_hybrid",    {"RETRIEVAL_BACKEND": "qdrant_hybrid",     "RETRIEVAL_SPARSE": "true"}),
    ]
    arm_held_out_results = []
    BASELINE_MRR = 0.767  # mmr_relevance-WINNER held-out (prior measurement)
    REGRESSION_FLOOR = 0.60

    print("\n\n=== T04-D: V1.7 ARM HELD-OUT SWEEP ===")
    print("Measuring snapshot_dense (baseline), snapshot_hybrid, and qdrant_hybrid")
    print("against the held-out split via the REAL mcp-server.")
    print("snapshot_dense keeps Qdrant write-only (CQRS intact).")
    print("qdrant_hybrid makes Qdrant a read-path dependency (CQRS break → T08 ADR).")
    print()

    for arm_name, arm_overrides in v17_arm_configs:
        print(f"\n########## T04-D ARM: {arm_name} ##########", flush=True)
        set_env(arm_overrides)
        # All three arms change RETRIEVAL_BACKEND: use reboot_arm for each.
        # snapshot_dense overrides are empty {} (default backend), but we still
        # use reboot_arm to get a fresh graph-builder rebuild with clean env
        # (clearing any residual RETRIEVAL_BACKEND from a prior arm).
        reboot_arm(arm_overrides)

        out_path = V17_ARM_HELD_OUT_REPORT_DIR / f"v17-{arm_name}__held_out.json"
        cmd = [sys.executable, "scripts/retrieval_quality_live.py",
               "--split", "held_out",
               "--config-label", f"v17-{arm_name}",
               "--limit", str(LIMIT),
               "--out", str(out_path),
               "--verdict-cache", "tests/e2e/reports/retrieval_234_live_verdicts.json",
               "--gate",
               "--regression-floor", str(REGRESSION_FLOOR)]
        res = subprocess.run(cmd, text=True)
        arm_rep = json.loads(out_path.read_text())
        arm_rep["_gate_exit"] = res.returncode
        arm_ja = arm_rep["judge_augmented"]
        arm_meta = arm_rep.get("arm", {})
        arm_lat = arm_rep.get("latency_ms", {})

        # Corpus-size guard: fail loud if the held-out positives are too few to trust.
        # The fixture has 40 held-out positives; fewer than 30 means the corpus is
        # degraded and the numbers are untrustworthy (do NOT silently report them).
        positives_count = arm_rep.get("positives", 0)
        if positives_count < 30:
            print(
                f"\nFATAL: arm '{arm_name}' held-out measurement has only {positives_count} "
                f"positive queries (minimum 30 required for trustworthy numbers).\n"
                f"The live corpus is degraded — the mcp-server is likely serving fewer\n"
                f"than the expected 234 skills (check graph-builder rebuild logs).\n"
                f"Do NOT report these numbers as the T04-D baseline comparison.",
                flush=True,
            )
            sys.exit(1)

        arm_held_out_results.append(dict(
            arm_name=arm_name,
            overrides=arm_overrides,
            positives=positives_count,
            negatives=arm_rep.get("negatives", 0),
            mrr=arm_ja["mrr"],
            ndcg_at_3=arm_ja["ndcg_at_3"],
            hit_at_3=arm_ja["hit_at_3"],
            recall_at_3=arm_ja["recall_at_3"],
            p_at_1=arm_ja["p_at_1"],
            no_match_precision=arm_rep.get("no_match_precision"),
            latency_ms=arm_lat,
            backend=arm_meta.get("backend"),
            embedder_model=arm_meta.get("embedder_model"),
            dimension=arm_meta.get("dimension"),
            sparse=arm_meta.get("sparse"),
            rerank=arm_meta.get("rerank"),
            gate_exit=res.returncode,
            report_path=str(out_path),
        ))

        print(f"  arm meta: backend={arm_meta.get('backend')}  "
              f"embedder={arm_meta.get('embedder_model')}  dim={arm_meta.get('dimension')}  "
              f"sparse={arm_meta.get('sparse')}  rerank={arm_meta.get('rerank')}", flush=True)
        print(f"  positives={positives_count}  negatives={arm_rep.get('negatives', 0)}", flush=True)
        print(f"  latency: mean={arm_lat.get('mean')}ms  p95={arm_lat.get('p95')}ms", flush=True)
        print(f"  held-out judge-aug: MRR={arm_ja['mrr']:.3f}  nDCG@3={arm_ja['ndcg_at_3']:.3f}  "
              f"hit@3={arm_ja['hit_at_3']:.3f}  recall@3={arm_ja['recall_at_3']:.3f}  "
              f"no_match_prec={arm_rep.get('no_match_precision')}", flush=True)
        p95 = arm_lat.get("p95", 0.0)
        if p95 >= 500.0:
            print(
                f"  FLAG: p95 latency {p95:.1f}ms >= 500ms for arm '{arm_name}'.\n"
                f"  This arm exceeds the compile_context budget SLO.\n"
                f"  Do NOT silently promote — document as flag-gated / find_skill-only.",
                flush=True,
            )

    # Print the T04-D comparison table.
    print("\n=== T04-D: V1.7 ARM COMPARISON (held-out, judge-augmented) ===")
    print(f"  Baseline (snapshot_dense): MRR={BASELINE_MRR:.3f}  (prior mmr_relevance-WINNER measurement)")
    print(f"  Aspiration: MRR>=0.80 nDCG@3>=0.80 no_match_prec>=0.90")
    print(f"  Regression floor: MRR>={REGRESSION_FLOOR}")
    print()
    print(f"{'arm':20s} {'MRR':>7s} {'nDCG@3':>7s} {'hit@3':>7s} {'recall@3':>8s} "
          f"{'no_match':>9s} {'p95ms':>7s} {'gate':>5s}")
    for r in arm_held_out_results:
        lat = r["latency_ms"]
        gate_str = "PASS" if r["gate_exit"] == 0 else "FAIL"
        print(f"{r['arm_name']:20s} {r['mrr']:>7.3f} {r['ndcg_at_3']:>7.3f} "
              f"{r['hit_at_3']:>7.3f} {r['recall_at_3']:>8.3f} "
              f"{r['no_match_precision'] or 'N/A':>9} "
              f"{lat.get('p95', 0.0):>7.1f} {gate_str:>5s}")

    # Honest verdict.
    print("\n=== T04-D: HONEST VERDICT ===")
    baseline_row = next((r for r in arm_held_out_results if r["arm_name"] == "snapshot_dense"), None)
    if baseline_row:
        print(f"snapshot_dense (baseline): MRR={baseline_row['mrr']:.3f}  "
              f"nDCG@3={baseline_row['ndcg_at_3']:.3f}  p95={baseline_row['latency_ms'].get('p95')}ms")
    for r in arm_held_out_results:
        if r["arm_name"] == "snapshot_dense":
            continue
        delta = r["mrr"] - (baseline_row["mrr"] if baseline_row else BASELINE_MRR)
        beats_baseline = delta > 0
        meets_aspiration = r["mrr"] >= 0.80 and r["ndcg_at_3"] >= 0.80
        p95 = r["latency_ms"].get("p95", 0.0)
        latency_ok = p95 < 500.0
        print(f"\n{r['arm_name']}:")
        print(f"  MRR={r['mrr']:.3f}  delta vs baseline={delta:+.3f}  "
              f"{'BEATS BASELINE' if beats_baseline else 'DOES NOT BEAT BASELINE'}")
        print(f"  Aspiration (0.80/0.80): {'MET' if meets_aspiration else 'UNMET'}")
        print(f"  p95={p95:.1f}ms  (<500ms SLO: {'PASS' if latency_ok else 'FAIL — flag-gated only'})")
        if r["arm_name"] == "qdrant_hybrid":
            print("  CQRS: qdrant_hybrid makes Qdrant a read-path dependency (CQRS break).")
            print("        Promotion decision deferred to T08 ADR based on this evidence.")
            print("        Qdrant v1.18.0 @ :16333.")
            if r["mrr"] > 0 and r.get("positives", 0) > 0:
                print("        qdrant_hybrid returned REAL non-empty mapped results — C3 live gap CLOSED.")
            else:
                print("        BLOCKER: qdrant_hybrid returned empty results — C3 live gap NOT CLOSED.")
        elif r["arm_name"] == "snapshot_hybrid":
            print("  CQRS: snapshot_hybrid keeps Qdrant write-only (CQRS intact).")
            print("        mcp-server reads from in-memory pool-union (BM25 only on write side).")

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
        t04_arm_held_out=arm_held_out_results,
    )
    Path("tests/e2e/reports/retrieval_234_sweep_summary.json").write_text(json.dumps(summary, indent=1))
    print("\nsweep summary: tests/e2e/reports/retrieval_234_sweep_summary.json")
    print(f"gate exit for winner held-out: {held['_gate_exit']} (0=meets target, 1=below)")


if __name__ == "__main__":
    main()
