#!/usr/bin/env python3
"""Fetch the CL-bench acquisition-band contexts (T14) from tencent/CL-bench.

Reads tests/e2e/efficacy/clband/manifest.json, downloads the pinned CL-bench
parquet (or uses --parquet for a local copy), and materializes each selected
context under tests/e2e/efficacy/clband/contexts/<name>/:

    system.md      - the instance's system prompt (shared across siblings)
    context.md     - the shared knowledge document (common prefix of first user turns)
    tasks.json     - per-task: task_id, depth, question (final user turn),
                     prior_turns (for multi-turn siblings), rubrics (verbatim)

Fail-loud guarantees (no-fakes standing rule):
  * The live HF dataset sha is checked against the manifest pin; drift aborts
    unless --allow-drift is passed (then the observed sha is printed loudly).
  * Every manifest sentinel must appear (case-insensitive) in system+context
    text; a missing sentinel aborts the run.
  * Contexts are NOT committed to the repo; this script is the reproducible
    source. Run it before any Session A authoring.

Usage:
    python3 scripts/fetch_clband_contexts.py                # download + extract all
    python3 scripts/fetch_clband_contexts.py --parquet /tmp/claude-1000/clbench/clbench.parquet
    python3 scripts/fetch_clband_contexts.py --only 7833ca0b bc874bce   # smoke pair
Requires: pyarrow (run via `uv run --with pyarrow scripts/fetch_clband_contexts.py`).
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.request
from collections import defaultdict
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CLBAND_DIR = REPO_ROOT / "tests" / "e2e" / "efficacy" / "clband"
MANIFEST = CLBAND_DIR / "manifest.json"
PARQUET_URL = "https://huggingface.co/api/datasets/tencent/CL-bench/parquet/default/train/0.parquet"
DATASET_API = "https://huggingface.co/api/datasets/tencent/CL-bench"


def die(msg: str) -> None:
    print(f"FATAL: {msg}", file=sys.stderr)
    sys.exit(1)


def check_dataset_pin(pinned_sha: str, allow_drift: bool) -> None:
    try:
        with urllib.request.urlopen(DATASET_API, timeout=30) as resp:
            live_sha = json.load(resp).get("sha")
    except Exception as e:  # offline use with a local parquet is legitimate
        print(f"WARN: could not check live dataset sha ({e}); relying on local parquet")
        return
    if live_sha != pinned_sha:
        msg = (f"tencent/CL-bench drifted: live sha {live_sha} != pinned {pinned_sha}. "
               f"The band's tasks/rubrics may have changed upstream.")
        if allow_drift:
            print(f"WARN (--allow-drift): {msg}")
        else:
            die(msg + " Re-pin the manifest deliberately or pass --allow-drift.")


def load_rows(parquet_path: Path):
    import pyarrow.parquet as pq  # deferred so --help works without pyarrow
    return pq.read_table(parquet_path).to_pylist()


def common_prefix(strings: list[str]) -> str:
    return os.path.commonprefix(strings)


def materialize(entry: dict, tasks: list[dict], out_root: Path) -> None:
    name = entry["name"]
    out = out_root / name
    out.mkdir(parents=True, exist_ok=True)

    systems = [next(m["content"] for m in t["messages"] if m["role"] == "system") for t in tasks]
    firsts = [next(m["content"] for m in t["messages"] if m["role"] == "user") for t in tasks]
    sys_text = common_prefix(systems)
    ctx_text = common_prefix(firsts)
    if sys_text != systems[0] or any(s != systems[0] for s in systems):
        # divergent system prompts across siblings would break the shared-context assumption
        die(f"{name}: sibling system prompts diverge; manifest assumption violated")

    searchable = (sys_text + "\n" + ctx_text).lower()
    missing = [s for s in entry["sentinels"] if s.lower() not in searchable]
    if missing:
        die(f"{name}: sentinels missing from context text: {missing} - "
            f"fidelity gate would be vacuous; fix the manifest")

    # Two placements exist in CL-bench: knowledge in the shared USER prefix
    # (nested multi-turn contexts) or in the SYSTEM prompt (flat siblings with
    # fully distinct user turns). Record which, so Session A materialization
    # knows where the teachable document lives.
    knowledge_home = "system" if len(sys_text) > len(ctx_text) else "user"

    (out / "system.md").write_text(sys_text)
    (out / "context.md").write_text(ctx_text)

    task_records = []
    for t in sorted(tasks, key=lambda t: len(t["messages"])):
        msgs = t["messages"]
        depth = len(msgs)
        teach_only = False
        if depth == 2:
            question = msgs[1]["content"][len(ctx_text):]
            if not question:
                # This sibling's user turn IS the shared context verbatim: the
                # task question is fused into the context document itself. It
                # cannot be posed standalone in Session B -> it is the natural
                # Session A teach task (the agent works it WITH the document).
                teach_only = True
            prior = []
        else:
            if msgs[-1]["role"] != "user":
                die(f"{name}/{t['metadata']['task_id'][:8]}: final message is not a user turn")
            question = msgs[-1]["content"]
            prior = [{"role": m["role"],
                      "content": (m["content"][len(ctx_text):] if i == 1 else m["content"])}
                     for i, m in enumerate(msgs[1:-1], start=1)]
        task_records.append({
            "task_id": t["metadata"]["task_id"],
            "depth": depth,
            "teach_only": teach_only,
            "question": question,
            "prior_turns": prior,
            "rubrics": t["rubrics"],
        })
    n_heldout = sum(1 for r in task_records if not r["teach_only"])
    if n_heldout < 2:
        die(f"{name}: fewer than 2 held-out-capable siblings ({n_heldout}) - "
            f"cannot run an OFF pre-gate plus a measured Session B")
    (out / "tasks.json").write_text(json.dumps(
        {"knowledge_home": knowledge_home, "tasks": task_records}, indent=1))
    print(f"  {name}: {len(task_records)} tasks ({n_heldout} held-out-capable), "
          f"knowledge in {knowledge_home} ({max(len(sys_text), len(ctx_text))} chars), "
          f"sentinels OK ({len(entry['sentinels'])})")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--parquet", type=Path, help="local CL-bench parquet (skips download)")
    ap.add_argument("--only", nargs="*", default=None,
                    help="context_id short prefixes to fetch (default: all in manifest)")
    ap.add_argument("--allow-drift", action="store_true",
                    help="proceed even if the live dataset sha differs from the pin")
    args = ap.parse_args()

    manifest = json.loads(MANIFEST.read_text())
    check_dataset_pin(manifest["_dataset"]["pinned_sha"], args.allow_drift)

    parquet = args.parquet
    if parquet is None:
        parquet = CLBAND_DIR / ".cache" / "clbench.parquet"
        parquet.parent.mkdir(parents=True, exist_ok=True)
        if not parquet.exists():
            print(f"downloading {PARQUET_URL} -> {parquet} ...")
            urllib.request.urlretrieve(PARQUET_URL, parquet)
    if not parquet.exists():
        die(f"parquet not found: {parquet}")

    rows = load_rows(parquet)
    by_ctx: dict[str, list] = defaultdict(list)
    for r in rows:
        by_ctx[r["metadata"]["context_id"]].append(r)

    out_root = CLBAND_DIR / "contexts"
    wanted = manifest["contexts"]
    if args.only:
        wanted = [e for e in wanted if e["short"] in set(args.only)]
        if len(wanted) != len(args.only):
            die(f"--only matched {len(wanted)} of {len(args.only)} requested entries")

    print(f"materializing {len(wanted)} contexts -> {out_root}")
    for entry in wanted:
        tasks = by_ctx.get(entry["context_id"])
        if not tasks:
            die(f"{entry['name']}: context_id {entry['context_id']} not in dataset "
                f"(dataset drifted past the pin?)")
        if len(tasks) != entry["n_tasks"]:
            die(f"{entry['name']}: expected {entry['n_tasks']} tasks, found {len(tasks)}")
        materialize(entry, tasks, out_root)
    print("done - contexts are local-only (gitignored); re-run anytime to reproduce")


if __name__ == "__main__":
    main()
