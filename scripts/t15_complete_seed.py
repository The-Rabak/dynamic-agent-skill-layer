#!/usr/bin/env python3
"""T15 N=40 — surgically COMPLETE the seed step after a partial drain.

The N=40 seed step ingested all 40 OFF transcripts, drained 30, then the worker
hit transient resource exhaustion on the 10 LARGEST transcripts (claude subprocess
exit 1 under memory pressure — proven NOT a size/deny-rule bug: the exact normalizer
call succeeds in isolation). The 10 were re-queued (pending). `drain_until_empty`
correctly failed loud rather than gating a partial corpus.

This finishes the seed step IDEMPOTENTLY — WITHOUT re-ingesting the 30 already-done
(which a plain loop-resume would do, creating duplicates):
  1. drain the 10 still-pending transcripts (reuses loop.drain_until_empty),
  2. gate ALL .pending drafts (the 30-done + 10-new),
  3. reconcile + snapshot rebuild,
  4. write the checkpoint's `seeding` field so the main loop resume skips straight
     to Round 2.
Then re-run the main loop command → it sees seeding done → solves Round 2.
"""
import json
import sys
from pathlib import Path

_SCRIPTS_DIR = Path(__file__).parent.resolve()
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

import t15_selfseed_loop as loop      # noqa: E402
import t15_swebench_seed as seedmod   # noqa: E402

RUN_ID = "selfseed-django-n40"
SCOPE = loop.PROJECT_ROOT / "swebench-django-n40"
LOG_DIR = loop.runner.REPO_ROOT / "logs/t15-selfseed" / RUN_ID
CKPT = LOG_DIR / "checkpoint.json"


def main() -> int:
    print(f"=== complete-seed for {RUN_ID} (scope {SCOPE}) ===", flush=True)
    pending = loop._queue_pending()
    print(f"[complete-seed] queue pending before = {pending}", flush=True)

    # 1) drain the remaining pending transcripts (machine is idle now; serial worker)
    drain = loop.drain_until_empty(SCOPE, LOG_DIR, timeout_s=3600)
    print(f"[complete-seed] drained in {drain['passes']} pass(es); queue now empty", flush=True)

    # 2) gate ALL drafts on disk (30-done + 10-new) via the real rename gate
    gated = loop.gate_drafts(SCOPE)
    print(f"[complete-seed] gated {len(gated)} skill(s)", flush=True)
    skill_source: dict[str, str | None] = {}
    for g in gated:
        src_iid = loop._iid_from_session(g["source_session_id"], RUN_ID)
        skill_source[g["name"]] = src_iid
        print(f"        + {g['name']}  (from {src_iid or g['source_session_id'] or '?'})", flush=True)

    # 3) reconcile filesystem → PG + rebuild mcp-server snapshot
    baseline_pg = int(seedmod.psql("SELECT count(*) FROM skills WHERE status != 'retired'") or 0)
    seedmod.reconcile_and_rebuild(wait_s=180, expected_total=len(gated))
    final_pg = int(seedmod.psql("SELECT count(*) FROM skills WHERE status != 'retired'") or 0)
    print(f"[complete-seed] PG skills: {baseline_pg} → {final_pg}", flush=True)

    # 4) write the checkpoint seeding field so the loop resume skips to Round 2
    ckpt = json.loads(CKPT.read_text())
    ckpt["seeding"] = {"ingested": len(ckpt["round1"]), "gated_skills": gated,
                       "skill_source": skill_source, "drain_passes": drain["passes"]}
    tmp = CKPT.with_suffix(".tmp")
    tmp.write_text(json.dumps(ckpt, indent=2) + "\n")
    tmp.replace(CKPT)
    print(f"[complete-seed] checkpoint seeding marked done ({len(gated)} skills). "
          f"Re-run the main loop command to proceed to Round 2.", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
