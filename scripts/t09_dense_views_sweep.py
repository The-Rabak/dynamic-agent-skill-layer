#!/usr/bin/env python3
"""T09 focused sweep: dense multi-view fusion (RETRIEVAL_DENSE_VIEWS) OFF vs ON.

Reuses the real measurement machinery from retrieval_quality_sweep.py — set_env,
reboot_mcp, wait_ready, measure — which drive retrieval_quality_live.py against the
LIVE mcp-server over HTTP. There is NO in-process reconstruction here (standing rule:
measurement drives the real running app end-to-end).

Dense views are a SNAPSHOT arm: views are built unconditionally at mcp-server boot;
the RETRIEVAL_DENSE_VIEWS flag gates only the READ (α fusion). So OFF-vs-ON is a pure
mcp-server env-flip + reboot_mcp — no graph-builder rebuild between arms.

Both arms measure the held_out split. The OFF arm must reproduce the pre-T09 baseline
exactly (flag default-off == byte-for-byte unchanged ranking).
"""
import sys

# Import the real, already-validated helpers (one source of truth for the harness).
from retrieval_quality_sweep import set_env, reboot_mcp, wait_ready, measure


SPLIT = "held_out"


def _run_arm(label: str, overrides: dict) -> dict:
    print(f"\n########## T09 ARM: {label}  {overrides or '(default/OFF)'} ##########", flush=True)
    set_env(overrides)
    reboot_mcp()
    wait_ready()
    rep = measure(label, SPLIT)
    ja = rep["judge_augmented"]
    arm = rep.get("arm", {})
    lat = rep.get("latency_ms", {})
    print(f"  arm: backend={arm.get('backend')}  dense_views={overrides.get('RETRIEVAL_DENSE_VIEWS', '(unset/OFF)')}", flush=True)
    print(f"  latency: mean={lat.get('mean')}ms  p95={lat.get('p95')}ms", flush=True)
    print(f"  judge-aug: MRR={ja['mrr']:.4f} nDCG@3={ja['ndcg_at_3']:.4f} "
          f"P@1={ja['p_at_1']:.4f} hit@3={ja['hit_at_3']:.4f} "
          f"no_match_prec={rep.get('no_match_precision')}", flush=True)
    return rep


def main():
    off = _run_arm("v17-t09-dense-views-OFF", {})
    on = _run_arm("v17-t09-dense-views-ON", {"RETRIEVAL_DENSE_VIEWS": "true"})

    off_ja, on_ja = off["judge_augmented"], on["judge_augmented"]
    off_lat, on_lat = off.get("latency_ms", {}), on.get("latency_ms", {})

    def d(metric, a, b):
        return f"{metric:14s} OFF={a:.4f}  ON={b:.4f}  Δ={b - a:+.4f}"

    print("\n\n=== T09 DENSE-VIEWS HELD-OUT DELTA (ON − OFF) ===", flush=True)
    print(d("MRR", off_ja["mrr"], on_ja["mrr"]), flush=True)
    print(d("nDCG@3", off_ja["ndcg_at_3"], on_ja["ndcg_at_3"]), flush=True)
    print(d("P@1", off_ja["p_at_1"], on_ja["p_at_1"]), flush=True)
    print(d("hit@3", off_ja["hit_at_3"], on_ja["hit_at_3"]), flush=True)
    nm_off = off.get("no_match_precision")
    nm_on = on.get("no_match_precision")
    print(f"{'no_match_prec':14s} OFF={nm_off}  ON={nm_on}", flush=True)
    p95_off = off_lat.get("p95", 0.0)
    p95_on = on_lat.get("p95", 0.0)
    print(f"{'p95_ms':14s} OFF={p95_off:.1f}  ON={p95_on:.1f}  Δ={p95_on - p95_off:+.1f}", flush=True)
    print("\nNote: pre-T03 corpus has EMPTY use_when/avoid_when/requires/invariants/tools/artifacts;", flush=True)
    print("e_task still carries 1769 subunit procedure texts. Meaningful multi-view validation is T11.", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
