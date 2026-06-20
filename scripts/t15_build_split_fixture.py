#!/usr/bin/env python3
"""T15 Phase-1 — materialize the deterministic SEED/TEST split fixture.

WHY this exists (pre-registration discipline, todo 283)
-------------------------------------------------------
The compounding DiD must consume a SEED/TEST partition that is fixed by a
recorded, reproducible rule BEFORE any TEST-set solve — so the verdict cannot be
reverse-engineered from the data. This script materializes that partition once
into a checked-in fixture that Phase 2/3 consume deterministically.

NO TEST DATA IS OBSERVED HERE. The script reads ONLY instance *ids* (metadata)
from the SWE-bench Lite enumerator — never the `problem_statement` or any test
content. Instance ids are pre-registration metadata, not the held-out TEST data.

Locked split rule (owner, 2026-06-19; broad-convention RANDOM split):
  * Repo pool          = all `django__django-*` Lite test instances MINUS the
                         3 Phase-0 seeds (14999, 16046, 13447).
  * Deterministic order = sort ascending by
                         sha1(PREREG_SALT + instance_id).hexdigest().
  * SEED block          = first N_SEED (12) of that order.
  * TEST block          = the remainder, in that same order; Phase 3 takes the
                         first N_TEST (locked just before Phase 3 — NOT here).
  * CTRL foreign corpus = the sympy Phase-0 seed drafts (a parallel scope).

N_TEST is intentionally NOT baked in — the fixture records the full deterministic
ordering and the runner selects SEED=first 12, TEST=first N of the test block.

Usage:
  scripts/t15_build_split_fixture.py [--out tests/fixtures/t15_swebench_split.json]
"""
import argparse
import hashlib
import json
import sys
import time
import urllib.request
from pathlib import Path

PREREG_SALT = "t15-preereg-2026-06-19"  # LOCKED — do not change (changes the split)
N_SEED = 12  # LOCKED seed-block size
PHASE0_SEEDS = ["django__django-14999", "django__django-16046", "django__django-13447"]
SYMPY_PHASE0_SEEDS = [  # the CTRL foreign-seed corpus (Phase-0 sympy solves)
    "sympy__sympy-20590",
    "sympy__sympy-13146",
    "sympy__sympy-11400",
]
REPO_PREFIX = "django__django-"

_HF_ROWS_URL = (
    "https://datasets-server.huggingface.co/rows"
    "?dataset=princeton-nlp%2FSWE-bench_Lite"
    "&config=default&split=test&offset={offset}&limit=100"
)
_HEADERS = {"User-Agent": "dynamic-agent-skill-layer/t15-split-builder"}
_MAX_OFFSET = 300  # Lite test split is 300 rows.


def fetch_all_instance_ids() -> list[str]:
    """Enumerate every Lite test instance_id (metadata only — NO problem_statement).

    Fails loud on a network error rather than returning a partial/fake list.
    """
    ids: list[str] = []
    for offset in range(0, _MAX_OFFSET + 1, 100):
        url = _HF_ROWS_URL.format(offset=offset)
        data = None
        # Transient HF gateway blips (502/503/timeouts) are infra noise, not a real
        # failure — retry with backoff. A persistent failure still fails loud (no
        # partial/fake split), per the standing rule.
        last_exc = None
        for attempt in range(6):
            req = urllib.request.Request(url, headers=_HEADERS)
            try:
                with urllib.request.urlopen(req, timeout=30) as resp:
                    data = json.loads(resp.read())
                break
            except Exception as exc:  # noqa: BLE001
                last_exc = exc
                print(f"  [enumerator] offset={offset} attempt {attempt + 1} failed: {exc}",
                      file=sys.stderr)
                time.sleep(2 * (attempt + 1))
        if data is None:
            print(f"ERROR: enumerator failed at offset={offset} after retries: {last_exc}",
                  file=sys.stderr)
            sys.exit(1)
        rows = data.get("rows", [])
        if not rows:
            break
        for row in rows:
            iid = row.get("row", {}).get("instance_id")
            if iid:
                ids.append(iid)  # ONLY the id — never the problem_statement
    if not ids:
        print("ERROR: enumerator returned zero instance ids", file=sys.stderr)
        sys.exit(1)
    return ids


def sha1_key(instance_id: str) -> str:
    """The pre-registered deterministic sort key for an instance id."""
    return hashlib.sha1((PREREG_SALT + instance_id).encode()).hexdigest()


def build_split(all_ids: list[str]) -> dict:
    django = sorted(i for i in all_ids if i.startswith(REPO_PREFIX))
    pool = [i for i in django if i not in PHASE0_SEEDS]
    # Deterministic pre-registered order.
    ordered = sorted(pool, key=sha1_key)
    seed_block = ordered[:N_SEED]
    test_block = ordered[N_SEED:]

    # Sanity (recorded, not fatal — the dataset revision is pinned upstream):
    # SWE-bench Lite has 114 django instances; pool = 114 - 3 = 111.
    return {
        "fixture": "t15_swebench_split",
        "purpose": (
            "Phase-1 pre-registered deterministic SEED/TEST split for the T15 "
            "within-repo compounding DiD. N_TEST is NOT locked here (chosen just "
            "before Phase 3). NO TEST problem statements were observed to build this."
        ),
        "prereg_salt": PREREG_SALT,
        "sort_rule": "ascending by sha1(prereg_salt + instance_id).hexdigest()",
        "repo": "django",
        "dataset": "princeton-nlp/SWE-bench_Lite",
        "split": "test",
        "n_seed_locked": N_SEED,
        "n_test_locked": None,
        "phase0_seeds_excluded": PHASE0_SEEDS,
        "ctrl_foreign_corpus": {
            "repo": "sympy",
            "source": "Phase-0 sympy seed drafts",
            "instances": SYMPY_PHASE0_SEEDS,
        },
        "counts": {
            "lite_total": len(all_ids),
            "django_total": len(django),
            "pool_after_excluding_phase0_seeds": len(pool),
        },
        "seed_block": seed_block,
        "test_block_ordered": test_block,
        # Per-instance key recorded so the ordering is independently verifiable.
        "ordering_keys": {iid: sha1_key(iid) for iid in ordered},
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--out",
        default="tests/fixtures/t15_swebench_split.json",
        help="Output fixture path.",
    )
    args = ap.parse_args()

    all_ids = fetch_all_instance_ids()
    split = build_split(all_ids)
    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(split, indent=2) + "\n")

    c = split["counts"]
    print(f"[split] lite_total={c['lite_total']} django_total={c['django_total']} "
          f"pool={c['pool_after_excluding_phase0_seeds']}")
    print(f"[split] SEED ({len(split['seed_block'])}): {', '.join(split['seed_block'])}")
    print(f"[split] TEST block size (N unlocked): {len(split['test_block_ordered'])}")
    print(f"[split] wrote {out}")
    if c["django_total"] != 114:
        print(f"WARNING: expected 114 django Lite instances, got {c['django_total']} "
              "(dataset revision drift?) — recorded, not fatal.", file=sys.stderr)


if __name__ == "__main__":
    main()
