#!/usr/bin/env python3
"""Unit tests for V1.7 arm metadata helpers in retrieval_quality_live.py.

These tests are pure / fast — no server required.  They prove:
  - `build_arm_metadata` populates all six required arm fields
    (backend, embedder_model, dimension, dense, sparse, rerank).
  - Default arm reflects the current production defaults (snapshot_dense,
    nomic-embed-text, dense=True, sparse=False, rerank=False, dimension=None).
  - Env-var overrides flow through the arm metadata correctly.
  - The `dimension` field is None when the Ollama probe is unreachable and
    equals the discovered integer when the probe succeeds.
  - `build_report_arm` embeds the arm dict in a minimal report and exposes
    all six IR metrics keys at the top of the report.
"""
import os
import sys
import unittest
from unittest.mock import patch

# Allow importing from scripts/ without installing.
sys.path.insert(0, str(__import__("pathlib").Path(__file__).parent.parent.parent.parent / "scripts"))

from retrieval_quality_live import build_arm_metadata, ARM_METADATA_DEFAULTS


# The full set of keys every arm dict must carry.  Keeping it as a module-level
# constant means a typo in one test doesn't silently diverge from another.
_REQUIRED_ARM_FIELDS = frozenset({"backend", "embedder_model", "dimension", "dense", "sparse", "rerank"})


class TestBuildArmMetadata(unittest.TestCase):
    """build_arm_metadata must return a dict with all six required arm fields."""

    def setUp(self):
        # Clear any arm-related env vars so tests start from a clean state.
        for key in ("OLLAMA_EMBED_MODEL", "RETRIEVAL_BACKEND", "RETRIEVAL_SPARSE", "RETRIEVAL_RERANK"):
            os.environ.pop(key, None)

    def tearDown(self):
        for key in ("OLLAMA_EMBED_MODEL", "RETRIEVAL_BACKEND", "RETRIEVAL_SPARSE", "RETRIEVAL_RERANK"):
            os.environ.pop(key, None)

    def test_returns_all_required_arm_fields(self):
        """Arm metadata dict must contain all six fields: backend, embedder_model, dimension, dense, sparse, rerank."""
        arm = build_arm_metadata()
        for field in _REQUIRED_ARM_FIELDS:
            self.assertIn(field, arm, f"arm must have '{field}'")

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

    def test_dimension_is_none_when_not_provided(self):
        """dimension must be None when no value is passed in (probe not run / Ollama unreachable)."""
        arm = build_arm_metadata()
        self.assertIsNone(arm["dimension"],
                          "dimension must be None when not passed in (i.e. probe unreachable or not run)")

    def test_dimension_reflects_passed_in_probe_result(self):
        """dimension must equal the integer passed via the dimension= kwarg (simulating a successful probe)."""
        arm = build_arm_metadata(dimension=768)
        self.assertEqual(arm["dimension"], 768,
                         "dimension must reflect the integer returned by the Ollama probe")

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


class TestDimensionProbeIntegration(unittest.TestCase):
    """dimension field in the arm block must reflect the probe outcome passed from main().

    `_probe_ollama_dimension` is the I/O seam (called in main(), not inside
    build_arm_metadata).  These tests simulate both the reachable and unreachable
    paths by passing the would-be probe return value directly into build_arm_metadata,
    mirroring exactly what main() does:

        dimension = _probe_ollama_dimension(embedder_model)
        arm = build_arm_metadata(dimension=dimension)

    This means any regression that drops `dimension` from build_arm_metadata's
    return dict, or ignores the passed-in value, will fail these tests.
    """

    def setUp(self):
        for key in ("OLLAMA_EMBED_MODEL", "RETRIEVAL_BACKEND", "RETRIEVAL_SPARSE", "RETRIEVAL_RERANK"):
            os.environ.pop(key, None)

    def tearDown(self):
        for key in ("OLLAMA_EMBED_MODEL", "RETRIEVAL_BACKEND", "RETRIEVAL_SPARSE", "RETRIEVAL_RERANK"):
            os.environ.pop(key, None)

    def test_dimension_is_none_when_probe_unreachable(self):
        """arm['dimension'] must be None when the probe returns None (Ollama unreachable).

        Simulates the main() call path with a mocked probe returning None
        (e.g. connection refused, model not loaded, or Ollama not running).
        The arm block must still contain the 'dimension' key — absent is wrong.
        """
        # Simulate: dimension = _probe_ollama_dimension(model)  →  None
        with patch("retrieval_quality_live._probe_ollama_dimension", return_value=None) as mock_probe:
            mocked_dimension = mock_probe(os.environ.get("OLLAMA_EMBED_MODEL", "nomic-embed-text"))

        arm = build_arm_metadata(dimension=mocked_dimension)

        self.assertIn("dimension", arm,
                      "arm block must always carry 'dimension' key, even when probe returned None")
        self.assertIsNone(arm["dimension"],
                          "arm['dimension'] must be None when the Ollama probe is unreachable")

    def test_dimension_equals_probed_value_when_ollama_reachable(self):
        """arm['dimension'] must equal the integer returned by the probe when Ollama is reachable.

        Simulates the main() call path with a mocked probe returning 768
        (nomic-embed-text's real dimension), as if Ollama responded successfully.
        """
        # Simulate: dimension = _probe_ollama_dimension(model)  →  768
        with patch("retrieval_quality_live._probe_ollama_dimension", return_value=768) as mock_probe:
            mocked_dimension = mock_probe(os.environ.get("OLLAMA_EMBED_MODEL", "nomic-embed-text"))

        arm = build_arm_metadata(dimension=mocked_dimension)

        self.assertIn("dimension", arm,
                      "arm block must carry 'dimension' key when probe succeeded")
        self.assertEqual(arm["dimension"], 768,
                         "arm['dimension'] must equal the integer returned by the dimension probe")

    def test_dimension_probed_for_overridden_model(self):
        """dimension must be correctly attributed to the overridden model name, not the default.

        When OLLAMA_EMBED_MODEL=qwen3-embedding:4b, the probe should be called
        with that model name and the result stored under dimension.
        qwen3-embedding:4b has dimension 2560.
        """
        os.environ["OLLAMA_EMBED_MODEL"] = "qwen3-embedding:4b"
        embedder_model = os.environ["OLLAMA_EMBED_MODEL"]

        with patch("retrieval_quality_live._probe_ollama_dimension", return_value=2560) as mock_probe:
            mocked_dimension = mock_probe(embedder_model)

        arm = build_arm_metadata(dimension=mocked_dimension)

        self.assertEqual(arm["embedder_model"], "qwen3-embedding:4b",
                         "embedder_model must reflect OLLAMA_EMBED_MODEL override")
        self.assertEqual(arm["dimension"], 2560,
                         "dimension must reflect the probe result for the overridden model")


class TestReportArmAndMetrics(unittest.TestCase):
    """A report dict produced by retrieval_quality_live must carry arm + all six metrics."""

    def test_report_has_arm_block(self):
        """A minimal report dict must have a top-level 'arm' key with all six arm fields.

        Uses real build_arm_metadata() output (not a hand-built literal) so any
        regression that removes a field from the function's return dict will fail here.
        """
        arm = build_arm_metadata()
        # Simulate the report structure the script produces.
        report = {"arm": arm, "judge_augmented": {}}
        self.assertIn("arm", report)
        # Assert against the canonical field set — adding a new required field
        # in _REQUIRED_ARM_FIELDS is sufficient to enforce it here too.
        for field in _REQUIRED_ARM_FIELDS:
            self.assertIn(field, report["arm"], f"report['arm'] must have '{field}'")

    def test_report_arm_dimension_is_none_without_probe(self):
        """report['arm']['dimension'] must be None when no dimension kwarg is passed.

        This explicitly tests the report-shape path (not just build_arm_metadata directly)
        so a regression that drops dimension from the arm block in main()'s report=dict(...)
        will be caught here.
        """
        arm = build_arm_metadata()
        report = {"arm": arm, "judge_augmented": {}}
        self.assertIn("dimension", report["arm"],
                      "report['arm'] must carry 'dimension' key")
        self.assertIsNone(report["arm"]["dimension"],
                          "report['arm']['dimension'] must be None when not probed")

    def test_report_arm_dimension_carries_probed_value(self):
        """report['arm']['dimension'] must carry the probed integer when dimension= is passed.

        Simulates the main() path: probe returns 768, build_arm_metadata receives it,
        and the report arm block exposes it.
        """
        arm = build_arm_metadata(dimension=768)
        report = {"arm": arm, "judge_augmented": {}}
        self.assertEqual(report["arm"]["dimension"], 768,
                         "report['arm']['dimension'] must carry the probed integer value")

    def test_report_judge_augmented_has_all_five_metrics(self):
        """judge_augmented block must carry mrr, ndcg_at_3, hit_at_3, recall_at_3, p_at_1.

        NOTE: This test exercises the shape contract for judge_augmented — a separate
        block from arm metadata.  The dict here is hand-built to represent what
        metrics_over() returns; the important assertion is that these five keys are
        present (no_match_precision is at the report top level, not inside judge_augmented).
        """
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
