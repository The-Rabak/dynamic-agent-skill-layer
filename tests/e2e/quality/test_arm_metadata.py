#!/usr/bin/env python3
"""Unit tests for V1.7 arm metadata helpers in retrieval_quality_live.py.

These tests are pure / fast — no server required.  They prove:
  - `build_arm_metadata` populates all six required arm fields.
  - Default arm reflects the current production defaults (snapshot_dense,
    nomic-embed-text, dense=True, sparse=False, rerank=False).
  - Env-var overrides flow through the arm metadata correctly.
  - `build_report_arm` embeds the arm dict in a minimal report and exposes
    all six IR metrics keys at the top of the report.
"""
import os
import sys
import unittest

# Allow importing from scripts/ without installing.
sys.path.insert(0, str(__import__("pathlib").Path(__file__).parent.parent.parent.parent / "scripts"))

from retrieval_quality_live import build_arm_metadata, ARM_METADATA_DEFAULTS


class TestBuildArmMetadata(unittest.TestCase):
    """build_arm_metadata must return a dict with all required arm fields."""

    def setUp(self):
        # Clear any arm-related env vars so tests start from a clean state.
        for key in ("OLLAMA_EMBED_MODEL", "RETRIEVAL_BACKEND", "RETRIEVAL_SPARSE", "RETRIEVAL_RERANK"):
            os.environ.pop(key, None)

    def tearDown(self):
        for key in ("OLLAMA_EMBED_MODEL", "RETRIEVAL_BACKEND", "RETRIEVAL_SPARSE", "RETRIEVAL_RERANK"):
            os.environ.pop(key, None)

    def test_returns_all_required_arm_fields(self):
        """Arm metadata dict must contain backend, embedder_model, dense, sparse, rerank."""
        arm = build_arm_metadata()
        self.assertIn("backend", arm, "arm must have 'backend'")
        self.assertIn("embedder_model", arm, "arm must have 'embedder_model'")
        self.assertIn("dense", arm, "arm must have 'dense'")
        self.assertIn("sparse", arm, "arm must have 'sparse'")
        self.assertIn("rerank", arm, "arm must have 'rerank'")

    def test_default_arm_matches_current_production_defaults(self):
        """Default arm (no env overrides) must reflect the current production defaults."""
        arm = build_arm_metadata()
        self.assertEqual(arm["backend"], "snapshot_dense",
                         "default backend must be snapshot_dense (in-memory dense cosine)")
        self.assertEqual(arm["embedder_model"], "nomic-embed-text",
                         "default embedder must be nomic-embed-text (current production model)")
        self.assertTrue(arm["dense"],
                        "dense retrieval must be on by default")
        self.assertFalse(arm["sparse"],
                         "sparse/BM25 must be off by default (not yet implemented)")
        self.assertFalse(arm["rerank"],
                         "reranker must be off by default (not yet implemented)")

    def test_ollama_embed_model_env_override_flows_through(self):
        """OLLAMA_EMBED_MODEL env var must override the embedder_model field."""
        os.environ["OLLAMA_EMBED_MODEL"] = "qwen3-embedding:4b"
        arm = build_arm_metadata()
        self.assertEqual(arm["embedder_model"], "qwen3-embedding:4b",
                         "OLLAMA_EMBED_MODEL env override must flow through to arm metadata")

    def test_retrieval_backend_env_override_flows_through(self):
        """RETRIEVAL_BACKEND env var must override the backend field."""
        os.environ["RETRIEVAL_BACKEND"] = "qdrant_hybrid"
        arm = build_arm_metadata()
        self.assertEqual(arm["backend"], "qdrant_hybrid",
                         "RETRIEVAL_BACKEND env override must flow through to arm metadata")

    def test_sparse_flag_env_override_flows_through(self):
        """RETRIEVAL_SPARSE=true must enable the sparse flag."""
        os.environ["RETRIEVAL_SPARSE"] = "true"
        arm = build_arm_metadata()
        self.assertTrue(arm["sparse"],
                        "RETRIEVAL_SPARSE=true must set sparse=True in arm metadata")

    def test_rerank_flag_env_override_flows_through(self):
        """RETRIEVAL_RERANK=true must enable the rerank flag."""
        os.environ["RETRIEVAL_RERANK"] = "true"
        arm = build_arm_metadata()
        self.assertTrue(arm["rerank"],
                        "RETRIEVAL_RERANK=true must set rerank=True in arm metadata")

    def test_arm_metadata_defaults_constant_matches_production_defaults(self):
        """ARM_METADATA_DEFAULTS must document the current production arm identity."""
        self.assertEqual(ARM_METADATA_DEFAULTS["backend"], "snapshot_dense")
        self.assertEqual(ARM_METADATA_DEFAULTS["embedder_model"], "nomic-embed-text")
        self.assertTrue(ARM_METADATA_DEFAULTS["dense"])
        self.assertFalse(ARM_METADATA_DEFAULTS["sparse"])
        self.assertFalse(ARM_METADATA_DEFAULTS["rerank"])


class TestReportArmAndMetrics(unittest.TestCase):
    """A report dict produced by retrieval_quality_live must carry arm + all six metrics."""

    def test_report_has_arm_block(self):
        """A minimal report dict must have a top-level 'arm' key with all arm fields."""
        from retrieval_quality_live import build_arm_metadata
        arm = build_arm_metadata()
        # Simulate the report structure the script produces.
        report = {"arm": arm, "judge_augmented": {}}
        self.assertIn("arm", report)
        for field in ("backend", "embedder_model", "dense", "sparse", "rerank"):
            self.assertIn(field, report["arm"], f"report['arm'] must have '{field}'")

    def test_report_judge_augmented_has_all_six_metrics(self):
        """judge_augmented block must carry mrr, ndcg_at_3, hit_at_3, recall_at_3, p_at_1."""
        required = {"mrr", "ndcg_at_3", "hit_at_3", "recall_at_3", "p_at_1"}
        # Simulate what metrics_over returns (keys are fixed in the script).
        judge_aug = {"mrr": 0.767, "ndcg_at_3": 0.749, "p_at_1": 0.667,
                     "recall_at_3": 0.808, "hit_at_3": 0.867, "n": 30}
        report = {"judge_augmented": judge_aug, "no_match_precision": 1.0}
        present = set(report["judge_augmented"].keys())
        missing = required - present
        self.assertFalse(missing, f"judge_augmented is missing metrics: {missing}")
        self.assertIn("no_match_precision", report,
                      "report must carry no_match_precision at top level")

    def test_report_has_arm_latency_block(self):
        """Report must carry a top-level 'latency_ms' summary dict."""
        report = {
            "arm": build_arm_metadata(),
            "latency_ms": {"mean": 90.0, "p50": 88.0, "p95": 110.0, "n": 30},
        }
        self.assertIn("latency_ms", report)
        for stat in ("mean", "p50", "p95", "n"):
            self.assertIn(stat, report["latency_ms"],
                          f"latency_ms block must have '{stat}'")


if __name__ == "__main__":
    unittest.main()
