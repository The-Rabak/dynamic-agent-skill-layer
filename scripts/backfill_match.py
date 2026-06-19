#!/usr/bin/env python3
"""Confidence-gated semantic matching: fresh re-extracted drafts -> existing target skills.

WHY: the re-architected frontier prompt RE-SEGMENTS sessions, so re-extraction does
NOT reproduce the existing skill identities by name (a target may be renamed, split
into 2, or reframed to a different aspect). To backfill HONESTLY we accept a fresh
draft's grounded fields onto an existing target ONLY when we can verify they are the
SAME skill. This script scores every (target, fresh-draft-from-the-same-source) pair
by description-embedding cosine (real ollama qwen3-embedding:4b over the live HTTP
endpoint — no in-process fakes) and emits a review ledger + a gated candidate map.

Gate (all must hold to auto-propose; the orchestrator still eyeball-confirms):
  - cosine(target.desc, fresh.desc) >= THRESHOLD (default 0.90)
  - mutual best: the fresh draft's best target is this target (1:1, rejects splits)
  - margin: best cosine - 2nd-best candidate cosine >= MARGIN (default 0.03)
  - same source session (fresh draft's source 8-hex == target's source 8-hex)

Usage: scripts/backfill_match.py [--threshold 0.90] [--margin 0.03]
Outputs: tests/e2e/reports/retrieval/backfill_match_review.json (full table)
         /tmp/backfill_match_candidates.json (gated proposals for the apply step)
"""
import json
import struct
import sys
import urllib.request
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
SCRATCH = Path("/tmp/backfill-reextract/skills")
OLLAMA = "http://127.0.0.1:11444/api/embeddings"
MODEL = "qwen3-embedding:4b"
FIELDS = ["use_when", "requires", "avoid_when", "invariants"]


def embed(text):
    body = json.dumps({"model": MODEL, "prompt": text}).encode()
    req = urllib.request.Request(OLLAMA, data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=120) as resp:
        return json.loads(resp.read())["embedding"]


def cos(a, b):
    dot = sum(x * y for x, y in zip(a, b))
    na = sum(x * x for x in a) ** 0.5
    nb = sum(y * y for y in b) ** 0.5
    return dot / (na * nb) if na and nb else 0.0


def src_hex(session_id):
    # backfill-NNNN-<8hex> or replica-NNNN-<8hex>
    return session_id.split("-")[-1] if session_id else None


def parse_pending(path):
    text = path.read_text(errors="replace")
    fm = {}
    if text.startswith("---\n") and "\n---\n" in text[4:]:
        raw, _ = text[4:].split("\n---\n", 1)
        try:
            fm = yaml.safe_load(raw) or {}
        except yaml.YAMLError:
            fm = {}
    return dict(name=fm.get("name", path.parent.name), description=fm.get("description", ""),
                source=src_hex(fm.get("source_session_id", "")),
                use_when=fm.get("use_when") or [], requires=fm.get("requires") or [],
                avoid_when=fm.get("avoid_when") or [], invariants=fm.get("invariants") or [])


def load_targets():
    """Existing targets: live SKILL.md description + source 8-hex + current field state."""
    rec = json.load(open("/tmp/ss_targets.json"))["recoverable"]
    fields = {r["name"]: r for r in json.load(open("/tmp/all_skills.json"))}
    out = []
    for name in rec:
        hits = sorted(ROOT.glob(f"tests/e2e/reports/**/.skills/{name}/SKILL.md"),
                      key=lambda p: (0 if "replica-run" in str(p) else 1))
        if not hits:
            out.append(dict(name=name, missing=True))
            continue
        text = hits[0].read_text()
        fm = yaml.safe_load(text[4:].split("\n---\n", 1)[0]) if text.startswith("---\n") else {}
        f = fields.get(name, {})
        out.append(dict(name=name, description=fm.get("description", ""),
                        source=src_hex(fm.get("source_session_id", "")),
                        live_uw=f.get("uw", 0), live_rq=f.get("rq", 0), missing=False,
                        file=str(hits[0].relative_to(ROOT))))
    return out


def main():
    thr = float(sys.argv[sys.argv.index("--threshold") + 1]) if "--threshold" in sys.argv else 0.90
    margin = float(sys.argv[sys.argv.index("--margin") + 1]) if "--margin" in sys.argv else 0.03

    drafts = [parse_pending(p) for p in SCRATCH.rglob("*.pending")]
    drafts = [d for d in drafts if not d["name"].endswith("(preference)")]
    targets = [t for t in load_targets() if not t["missing"]]
    print(f"[match] {len(targets)} targets vs {len(drafts)} fresh drafts; embedding descriptions...", file=sys.stderr)

    # Embed all (dedup identical text).
    texts = {t["description"] for t in targets} | {d["description"] for d in drafts}
    texts.discard("")
    vec = {}
    for i, txt in enumerate(texts):
        vec[txt] = embed(txt)
        if (i + 1) % 20 == 0:
            print(f"[match] embedded {i+1}/{len(texts)}", file=sys.stderr)

    # draft best-target (for mutual-best test)
    draft_best = {}
    for d in drafts:
        if not d["description"]:
            continue
        cands = [(cos(vec[d["description"]], vec[t["description"]]), t["name"])
                 for t in targets if t["description"] and t["source"] == d["source"]]
        if cands:
            draft_best[d["name"]] = max(cands)[1]

    review, proposals = [], {}
    for t in targets:
        if not t["description"]:
            continue
        cands = []
        for d in drafts:
            if not d["description"] or d["source"] != t["source"]:
                continue
            cands.append((cos(vec[t["description"]], vec[d["description"]]), d))
        cands.sort(key=lambda x: -x[0])
        top = cands[:3]
        best_cos, best_d = (top[0] if top else (0.0, None))
        second = top[1][0] if len(top) > 1 else 0.0
        mutual = best_d and draft_best.get(best_d["name"]) == t["name"]
        passes = bool(best_d and best_cos >= thr and mutual and (best_cos - second) >= margin)
        stageable = {}
        if best_d:
            for f in FIELDS:
                live_has = (t["live_uw"] if f == "use_when" else t["live_rq"] if f == "requires" else 0)
                # only use_when/requires tracked in /tmp; for avoid_when/invariants rely on live file check at apply
                if best_d.get(f) and (f not in ("use_when", "requires") or live_has == 0):
                    stageable[f] = best_d[f]
        review.append(dict(target=t["name"], live_uw=t["live_uw"], live_rq=t["live_rq"],
                           best_draft=best_d["name"] if best_d else None,
                           best_cos=round(best_cos, 4), second_cos=round(second, 4),
                           mutual_best=bool(mutual), passes_gate=passes,
                           stageable_fields={k: len(v) for k, v in stageable.items()},
                           target_desc=t["description"][:120],
                           best_draft_desc=best_d["description"][:120] if best_d else None))
        if passes:
            proposals[t["name"]] = dict(file=t["file"], from_draft=best_d["name"],
                                        cosine=round(best_cos, 4), fields=stageable)

    review.sort(key=lambda r: -r["best_cos"])
    out = ROOT / "tests/e2e/reports/retrieval/backfill_match_review.json"
    out.write_text(json.dumps(review, indent=2))
    json.dump(proposals, open("/tmp/backfill_match_candidates.json", "w"), indent=2)

    print(f"\n=== match review (threshold={thr} margin={margin}) ===")
    print(f"{'target':<46}{'cos':>7}{'2nd':>7} mut gate  best_draft")
    for r in review:
        print(f"{r['target']:<46}{r['best_cos']:>7}{r['second_cos']:>7}"
              f"{'  Y' if r['mutual_best'] else '  .'}{'  PASS' if r['passes_gate'] else '  ----'}  {r['best_draft']}")
    print(f"\nGATED PROPOSALS: {len(proposals)} / {len(targets)} targets")
    print(f"review: {out}\ncandidates: /tmp/backfill_match_candidates.json")


if __name__ == "__main__":
    main()
