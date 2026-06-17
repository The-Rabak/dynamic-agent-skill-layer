#!/usr/bin/env python3
"""Per-stratum comparison across two-or-more t12_task_quality_probe.py reports.

WHY: an aggregate MRR hides *where* a retrieval lever acts. The skill-side dense
views (RETRIEVAL_DENSE_VIEWS) and the e_negative penalty (RETRIEVAL_NEGATIVE_VIEW_WEIGHT)
concentrate their effect in specific strata (use_when / multiview / transcript for
dense views; negative / no_match for e_negative). This tool reads the `per_stratum`
block each probe now emits and prints a side-by-side table + deltas vs the first
(baseline) report, so a contribution that barely moves the aggregate is visible.

Usage:
  python3 scripts/retrieval_stratum_diff.py BASELINE.json ARM1.json [ARM2.json ...]
  # e.g. dense on/off:
  python3 scripts/retrieval_stratum_diff.py mvprobe_dv_off_4b.json mvprobe_dv_on_4b.json
  # e_negative weight sweep:
  python3 scripts/retrieval_stratum_diff.py neg_w0.json neg_w0p2.json neg_w0p4.json

Fails loud if a report lacks `per_stratum` (re-run the probe to regenerate).
"""
import json
import sys
from pathlib import Path

METRIC_ORDER = ["mrr_at3", "ndcg_at3", "hit_at3", "candidate_recall_at_limit", "no_match_precision"]


def load(path):
    rep = json.loads(Path(path).read_text())
    if "per_stratum" not in rep:
        raise SystemExit(f"FAIL: {path} has no `per_stratum` block — re-run t12_task_quality_probe.py.")
    label = rep.get("config_label", Path(path).stem)
    return label, rep["per_stratum"], rep.get("metrics", {})


def fmt(v):
    return "  -  " if v is None else f"{v:.4f}"


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    reports = [load(p) for p in sys.argv[1:]]
    base_label, base_strata, _ = reports[0]
    all_strata = []
    for _, strata, _ in reports:
        for k in strata:
            if k not in all_strata:
                all_strata.append(k)

    print(f"\nBASELINE = {base_label}   (Δ columns are arm − baseline)\n")
    for stratum in all_strata:
        metrics_here = [m for m in METRIC_ORDER if any(m in r[1].get(stratum, {}) for r in reports)]
        if not metrics_here:
            continue
        n = base_strata.get(stratum, {}).get("n", "?")
        print(f"── {stratum}  (n={n}) ──")
        header = f"  {'metric':<26}" + "".join(f"{lab[:14]:>15}" for lab, _, _ in reports)
        print(header)
        for metric in metrics_here:
            row = f"  {metric:<26}"
            base_val = base_strata.get(stratum, {}).get(metric)
            for i, (_, strata, _) in enumerate(reports):
                v = strata.get(stratum, {}).get(metric)
                cell = fmt(v)
                if i > 0 and v is not None and base_val is not None:
                    cell = f"{v:.4f}({v - base_val:+.3f})"
                row += f"{cell:>15}"
            print(row)
        print()


if __name__ == "__main__":
    main()
