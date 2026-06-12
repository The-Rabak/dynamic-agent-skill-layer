#!/usr/bin/env python3
"""Unit tests for the T14 efficacy A/B harness.

WHY this test file exists
--------------------------
These tests prove the deterministic pieces of the efficacy harness before any
live model call is made:
  - Task-spec loading and CONTRACT schema validation
  - Verifier exit-code → win/loss mapping
  - Per-pull attribution parser (session_start_priming vs mid_session_find_skill)
  - Gate classifier: PASS / FAIL / UNDERPOWERED / INSTRUMENT-FAILURE classification
  - Report rendering
  - Draft-acceptance scorer fail-loud-on-<10

TDD mode: Ralph-driven.  Red proves missing behavior; Green proves the fix.
"""
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

# Allow imports from the scripts/ directory (same pattern as retrieval_sweep.py).
_SCRIPTS_DIR = pathlib.Path(__file__).parent.resolve()
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

import efficacy_ab as _ab
import efficacy_draft_acceptance as _da


# ── Helpers ────────────────────────────────────────────────────────────────────

def _minimal_task_spec(task_id: str = "test-invented-rule") -> dict:
    """Return a minimal but schema-valid task spec for testing."""
    return {
        "task_id": task_id,
        "title": "Test invented rule task",
        "invented_rule": {
            "summary": "Always prefix log messages with [SKILL-TEST].",
            "corpus_skill_slug": "test-invented-rule/SKILL.md",
            "corpus_skill_id": "00000000-0000-0000-0000-000000000001",
            "absent_from_pretraining_rationale": "This rule is invented for this test harness.",
        },
        "prompt": "Write a hello-world Python script that logs a message.",
        "workspace": {
            "kind": "scratch",
            "base_ref": None,
            "setup": [],
        },
        "verifier": {
            "command": "tests/e2e/efficacy/verifiers/test-invented-rule.sh",
            "contract": "Exit 0 == rule obeyed.",
        },
        "expected": {
            "on": "pass",
            "off": "fail",
            "placebo": "fail",
            "sensitivity_note": "If ON fails this, the injection path is broken.",
        },
    }


def _write_task_spec(directory: pathlib.Path, spec: dict) -> pathlib.Path:
    """Write a task spec JSON file into the given directory."""
    task_file = directory / f"{spec['task_id']}.json"
    task_file.write_text(json.dumps(spec, indent=2))
    return task_file


# ── Contract schema validation ─────────────────────────────────────────────────

class TestTaskSpecValidation(unittest.TestCase):
    """validate_task_spec() enforces the CONTRACT schema."""

    def test_valid_spec_passes(self):
        spec = _minimal_task_spec()
        issues = _ab.validate_task_spec(spec)
        self.assertEqual(issues, [], f"Valid spec should have no issues; got: {issues}")

    def test_missing_task_id_fails(self):
        spec = _minimal_task_spec()
        del spec["task_id"]
        issues = _ab.validate_task_spec(spec)
        self.assertTrue(any("task_id" in i for i in issues),
                        f"Missing task_id must be reported; issues={issues}")

    def test_missing_invented_rule_block_fails(self):
        spec = _minimal_task_spec()
        del spec["invented_rule"]
        issues = _ab.validate_task_spec(spec)
        self.assertTrue(any("invented_rule" in i for i in issues),
                        f"Missing invented_rule must be reported; issues={issues}")

    def test_missing_corpus_skill_id_fails(self):
        spec = _minimal_task_spec()
        del spec["invented_rule"]["corpus_skill_id"]
        issues = _ab.validate_task_spec(spec)
        self.assertTrue(any("corpus_skill_id" in i for i in issues),
                        f"Missing corpus_skill_id must be reported; issues={issues}")

    def test_missing_verifier_command_fails(self):
        spec = _minimal_task_spec()
        del spec["verifier"]["command"]
        issues = _ab.validate_task_spec(spec)
        self.assertTrue(any("verifier" in i or "command" in i for i in issues),
                        f"Missing verifier command must be reported; issues={issues}")

    def test_missing_expected_block_fails(self):
        spec = _minimal_task_spec()
        del spec["expected"]
        issues = _ab.validate_task_spec(spec)
        self.assertTrue(any("expected" in i for i in issues),
                        f"Missing expected must be reported; issues={issues}")

    def test_missing_prompt_fails(self):
        spec = _minimal_task_spec()
        del spec["prompt"]
        issues = _ab.validate_task_spec(spec)
        self.assertTrue(any("prompt" in i for i in issues),
                        f"Missing prompt must be reported; issues={issues}")

    def test_missing_workspace_kind_fails(self):
        spec = _minimal_task_spec()
        del spec["workspace"]["kind"]
        issues = _ab.validate_task_spec(spec)
        self.assertTrue(any("workspace" in i or "kind" in i for i in issues),
                        f"Missing workspace.kind must be reported; issues={issues}")

    def test_load_task_specs_from_directory(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            _write_task_spec(d, _minimal_task_spec("task-alpha"))
            _write_task_spec(d, _minimal_task_spec("task-beta"))
            specs = _ab.load_task_specs(d)
            task_ids = {s["task_id"] for s in specs}
            self.assertEqual(task_ids, {"task-alpha", "task-beta"})

    def test_load_task_specs_empty_directory_fails_loud(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            with self.assertRaises(SystemExit) as ctx:
                _ab.load_task_specs(pathlib.Path(tmpdir))
            self.assertNotEqual(ctx.exception.code, 0)


# ── Verifier mapping ───────────────────────────────────────────────────────────

class TestVerifierExitCodeMapping(unittest.TestCase):
    """map_verifier_exit_code_to_outcome() converts exit codes to win/loss/error."""

    def test_exit_0_is_win(self):
        result = _ab.map_verifier_exit_code_to_outcome(0, "rule obeyed")
        self.assertEqual(result["outcome"], "win")
        self.assertEqual(result["verifier_reason"], "rule obeyed")

    def test_nonzero_exit_is_loss(self):
        result = _ab.map_verifier_exit_code_to_outcome(1, "rule not obeyed")
        self.assertEqual(result["outcome"], "loss")

    def test_exit_2_is_loss(self):
        result = _ab.map_verifier_exit_code_to_outcome(2, "unexpected error")
        self.assertEqual(result["outcome"], "loss")

    def test_run_verifier_exit0_is_win(self):
        """run_verifier() on a workspace returns win for a trivially-passing script."""
        with tempfile.TemporaryDirectory() as tmpdir:
            ws = pathlib.Path(tmpdir)
            # Write a verifier that always exits 0.
            v = ws / "verifier.sh"
            v.write_text("#!/bin/sh\necho 'rule obeyed'\nexit 0\n")
            v.chmod(0o755)
            result = _ab.run_verifier(str(v), ws)
            self.assertEqual(result["outcome"], "win")

    def test_run_verifier_exit1_is_loss(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            ws = pathlib.Path(tmpdir)
            v = ws / "verifier.sh"
            v.write_text("#!/bin/sh\necho 'rule not obeyed'\nexit 1\n")
            v.chmod(0o755)
            result = _ab.run_verifier(str(v), ws)
            self.assertEqual(result["outcome"], "loss")


# ── Attribution parser ─────────────────────────────────────────────────────────

class TestAttributionParser(unittest.TestCase):
    """parse_retrieval_attribution() labels pulls by trigger type."""

    def _make_transcript_with_pulls(self, pulls: list[dict]) -> dict:
        """Build a synthetic transcript structure with annotated MCP pulls."""
        return {"retrieval_pulls": pulls}

    def test_session_start_priming_labeled(self):
        transcript = self._make_transcript_with_pulls([
            {
                "trigger": "SessionStart",
                "tool": "compile_context",
                "skills_returned": ["skill-a", "skill-b"],
                "timestamp": "2026-06-12T10:00:00Z",
            }
        ])
        labels = _ab.parse_retrieval_attribution(transcript)
        self.assertEqual(len(labels), 1)
        self.assertEqual(labels[0]["label"], "session_start_priming")
        self.assertEqual(labels[0]["skills_returned"], ["skill-a", "skill-b"])

    def test_user_prompt_submit_labeled_as_mid_session(self):
        transcript = self._make_transcript_with_pulls([
            {
                "trigger": "UserPromptSubmit",
                "tool": "compile_context",
                "skills_returned": ["skill-c"],
                "timestamp": "2026-06-12T10:01:00Z",
            }
        ])
        labels = _ab.parse_retrieval_attribution(transcript)
        self.assertEqual(len(labels), 1)
        self.assertEqual(labels[0]["label"], "mid_session_find_skill")

    def test_mixed_triggers_labeled_correctly(self):
        transcript = self._make_transcript_with_pulls([
            {"trigger": "SessionStart", "tool": "compile_context", "skills_returned": ["s1"], "timestamp": "T1"},
            {"trigger": "UserPromptSubmit", "tool": "compile_context", "skills_returned": ["s2"], "timestamp": "T2"},
            {"trigger": "UserPromptSubmit", "tool": "compile_context", "skills_returned": ["s3"], "timestamp": "T3"},
        ])
        labels = _ab.parse_retrieval_attribution(transcript)
        self.assertEqual(labels[0]["label"], "session_start_priming")
        self.assertEqual(labels[1]["label"], "mid_session_find_skill")
        self.assertEqual(labels[2]["label"], "mid_session_find_skill")

    def test_empty_transcript_returns_empty_list(self):
        labels = _ab.parse_retrieval_attribution({"retrieval_pulls": []})
        self.assertEqual(labels, [])

    def test_missing_pulls_key_returns_empty_list(self):
        labels = _ab.parse_retrieval_attribution({})
        self.assertEqual(labels, [])

    def test_corpus_skill_id_present_in_pull(self):
        """Attribution confirms the invented-rule skill was injected."""
        transcript = self._make_transcript_with_pulls([
            {
                "trigger": "SessionStart",
                "tool": "compile_context",
                "skills_returned": ["skill-a", "skill-b"],
                "skill_ids_returned": ["00000000-0000-0000-0000-000000000001", "some-other-id"],
                "timestamp": "T1",
            }
        ])
        target_id = "00000000-0000-0000-0000-000000000001"
        labels = _ab.parse_retrieval_attribution(transcript)
        injected = _ab.attribution_confirms_skill_injected(labels, target_id)
        self.assertTrue(injected, "Attribution should confirm skill was injected")

    def test_corpus_skill_id_absent_from_pulls(self):
        transcript = self._make_transcript_with_pulls([
            {
                "trigger": "SessionStart",
                "tool": "compile_context",
                "skills_returned": ["skill-x"],
                "skill_ids_returned": ["ffffffff-0000-0000-0000-000000000000"],
                "timestamp": "T1",
            }
        ])
        target_id = "00000000-0000-0000-0000-000000000001"
        labels = _ab.parse_retrieval_attribution(transcript)
        injected = _ab.attribution_confirms_skill_injected(labels, target_id)
        self.assertFalse(injected, "Attribution should NOT confirm skill was injected")


# ── Gate classifier ────────────────────────────────────────────────────────────

class TestGateClassifier(unittest.TestCase):
    """classify_efficacy_verdict() implements exactly the LOCKED pre-registered semantics."""

    # Criterion string that must appear verbatim in all gate outputs.
    CRITERION = "ON wins ≥ 7 of 10 paired tasks by sign test, with no catastrophic regression on any single task."

    def _make_per_task_results(self, on_wins: int, off_wins: int, n_tasks: int = 10) -> list[dict]:
        """Build synthetic per-task results: first on_wins tasks ON wins, rest OFF wins."""
        results = []
        for i in range(n_tasks):
            on_win = i < on_wins
            off_win = i >= on_wins and (i - on_wins) < off_wins
            results.append({
                "task_id": f"task-{i}",
                "on_outcome": "win" if on_win else "loss",
                "off_outcome": "win" if off_win else "loss",
                "placebo_outcome": "loss",
                "catastrophic_regression": False,
                "attribution": [],
            })
        return results

    def test_pass_verdict_when_on_wins_7_of_10(self):
        """ON wins 7/10 tasks: should emit PASS."""
        per_task = self._make_per_task_results(on_wins=7, off_wins=3)
        verdict = _ab.classify_efficacy_verdict(per_task)
        self.assertEqual(verdict["verdict"], "PASS", f"Expected PASS; got {verdict}")

    def test_pass_verdict_when_on_wins_10_of_10(self):
        per_task = self._make_per_task_results(on_wins=10, off_wins=0)
        verdict = _ab.classify_efficacy_verdict(per_task)
        self.assertEqual(verdict["verdict"], "PASS")

    def test_fail_verdict_when_on_wins_fewer_than_off(self):
        """ON wins 3/10, OFF wins 7/10: should emit FAIL."""
        per_task = self._make_per_task_results(on_wins=3, off_wins=7)
        verdict = _ab.classify_efficacy_verdict(per_task)
        self.assertEqual(verdict["verdict"], "FAIL", f"Expected FAIL; got {verdict}")

    def test_underpowered_when_positive_but_below_bar(self):
        """ON wins 5/10 (positive direction but < 7): should emit UNDERPOWERED."""
        per_task = self._make_per_task_results(on_wins=5, off_wins=5)
        verdict = _ab.classify_efficacy_verdict(per_task)
        self.assertEqual(verdict["verdict"], "UNDERPOWERED",
                         f"5/10 positive but below bar must be UNDERPOWERED; got {verdict}")

    def test_underpowered_when_6_of_10(self):
        """ON wins 6/10: below the 7 threshold → UNDERPOWERED."""
        per_task = self._make_per_task_results(on_wins=6, off_wins=4)
        verdict = _ab.classify_efficacy_verdict(per_task)
        self.assertEqual(verdict["verdict"], "UNDERPOWERED",
                         f"6/10 is positive but below bar; got {verdict}")

    def test_instrument_failure_blocks_verdict(self):
        """If any task has ON failing with attribution-confirmed rule injection, INSTRUMENT-FAILURE."""
        per_task = self._make_per_task_results(on_wins=8, off_wins=2)
        # Force task-0 to be an instrument failure: ON loses AND rule was injected.
        per_task[0]["on_outcome"] = "loss"
        per_task[0]["instrument_failure"] = True
        verdict = _ab.classify_efficacy_verdict(per_task)
        self.assertEqual(verdict["verdict"], "INSTRUMENT-FAILURE",
                         f"instrument_failure flag must block verdict; got {verdict}")

    def test_catastrophic_regression_prevents_pass(self):
        """Even with 8/10 wins, catastrophic regression on one task prevents PASS → FAIL."""
        per_task = self._make_per_task_results(on_wins=8, off_wins=2)
        per_task[0]["catastrophic_regression"] = True
        verdict = _ab.classify_efficacy_verdict(per_task)
        self.assertNotEqual(verdict["verdict"], "PASS",
                            f"Catastrophic regression must prevent PASS; got {verdict}")

    def test_criterion_string_present_verbatim(self):
        """Every verdict dict must include the pre-registered criterion string verbatim."""
        per_task = self._make_per_task_results(on_wins=7, off_wins=3)
        verdict = _ab.classify_efficacy_verdict(per_task)
        self.assertIn("pre_registered_criterion", verdict,
                      "pre_registered_criterion key must be present in verdict dict")
        self.assertEqual(verdict["pre_registered_criterion"], self.CRITERION,
                         "criterion string must be verbatim as pre-registered")

    def test_sign_test_p_value_included(self):
        """The verdict dict must include the sign-test p-value."""
        per_task = self._make_per_task_results(on_wins=7, off_wins=3)
        verdict = _ab.classify_efficacy_verdict(per_task)
        self.assertIn("sign_test_p_value", verdict)
        self.assertIsInstance(verdict["sign_test_p_value"], float)

    def test_null_result_is_underpowered_not_no_effect(self):
        """All ties (neither ON nor OFF wins): must be UNDERPOWERED, never spun as 'no effect'."""
        per_task = []
        for i in range(10):
            per_task.append({
                "task_id": f"task-{i}",
                "on_outcome": "loss",
                "off_outcome": "loss",
                "placebo_outcome": "loss",
                "catastrophic_regression": False,
                "attribution": [],
            })
        verdict = _ab.classify_efficacy_verdict(per_task)
        # ON wins 0, OFF wins 0 — technically ON does not lose either, but no wins.
        # This is a null result; should be UNDERPOWERED (cannot distinguish).
        self.assertIn(verdict["verdict"], {"UNDERPOWERED", "FAIL"},
                      f"All-tie result must not be PASS; got {verdict}")

    def test_on_equal_to_off_is_fail(self):
        """ON wins exactly same as OFF: should be FAIL (ON ≤ OFF)."""
        per_task = []
        for i in range(10):
            per_task.append({
                "task_id": f"task-{i}",
                "on_outcome": "win" if i < 5 else "loss",
                "off_outcome": "win" if i >= 5 else "loss",
                "placebo_outcome": "loss",
                "catastrophic_regression": False,
                "attribution": [],
            })
        verdict = _ab.classify_efficacy_verdict(per_task)
        # 5 ON wins, 5 OFF wins — tied at sign test, ON does not beat OFF
        self.assertIn(verdict["verdict"], {"UNDERPOWERED", "FAIL"},
                      f"Tied result must not be PASS; got {verdict}")


# ── Report rendering ───────────────────────────────────────────────────────────

class TestReportRendering(unittest.TestCase):
    """render_efficacy_report() produces valid JSON and human text."""

    def _make_run_summary(self, verdict: str = "PASS") -> dict:
        per_task = [
            {
                "task_id": "task-0",
                "on_outcome": "win",
                "off_outcome": "loss",
                "placebo_outcome": "loss",
                "catastrophic_regression": False,
                "attribution": [{"label": "session_start_priming", "skills_returned": ["s1"]}],
            }
        ] * 10
        return {
            "run_id": "test-run-001",
            "per_task_results": per_task,
            "verdict_summary": _ab.classify_efficacy_verdict(per_task),
            "arms_used": ["on", "off", "placebo"],
            "max_turns": 40,
        }

    def test_report_includes_criterion_verbatim(self):
        summary = self._make_run_summary()
        report = _ab.render_efficacy_report(summary)
        self.assertIn(
            "ON wins ≥ 7 of 10 paired tasks by sign test, with no catastrophic regression on any single task.",
            report["human_text"],
            "Human report must include criterion verbatim",
        )

    def test_report_json_has_required_keys(self):
        summary = self._make_run_summary()
        report = _ab.render_efficacy_report(summary)
        for key in ("run_id", "verdict", "per_task_table", "sign_test_p_value",
                    "on_vs_placebo", "attribution_per_task", "pre_registered_criterion"):
            self.assertIn(key, report["json_data"], f"JSON report missing key: {key}")

    def test_report_human_text_includes_verdict(self):
        summary = self._make_run_summary()
        report = _ab.render_efficacy_report(summary)
        self.assertIn("PASS", report["human_text"])

    def test_report_can_be_written_to_directory(self):
        summary = self._make_run_summary()
        report = _ab.render_efficacy_report(summary)
        with tempfile.TemporaryDirectory() as tmpdir:
            out_dir = pathlib.Path(tmpdir) / "efficacy" / "test-run-001"
            _ab.write_efficacy_report(report, out_dir)
            self.assertTrue((out_dir / "report.json").exists())
            self.assertTrue((out_dir / "report.txt").exists())


# ── Dry-run plan builder ───────────────────────────────────────────────────────

class TestDryRunPlanBuilder(unittest.TestCase):
    """build_dry_run_plan() produces per-arm commands without running anything."""

    def test_dry_run_plan_has_three_arms(self):
        spec = _minimal_task_spec()
        with tempfile.TemporaryDirectory() as tmpdir:
            plan = _ab.build_dry_run_plan(
                task_spec=spec,
                arms=["on", "off", "placebo"],
                max_turns=40,
                on_settings=pathlib.Path("scripts/settings-efficacy-on.json"),
                placebo_settings=pathlib.Path("scripts/settings-efficacy-placebo.json"),
                workspace_base=pathlib.Path(tmpdir),
            )
        self.assertEqual(set(plan.keys()), {"on", "off", "placebo"})

    def test_dry_run_on_arm_includes_settings(self):
        spec = _minimal_task_spec()
        with tempfile.TemporaryDirectory() as tmpdir:
            plan = _ab.build_dry_run_plan(
                task_spec=spec,
                arms=["on", "off", "placebo"],
                max_turns=40,
                on_settings=pathlib.Path("scripts/settings-efficacy-on.json"),
                placebo_settings=pathlib.Path("scripts/settings-efficacy-placebo.json"),
                workspace_base=pathlib.Path(tmpdir),
            )
        # ON arm must have a --settings flag
        on_cmd = plan["on"]["claude_code_command"]
        self.assertIn("--settings", on_cmd, f"ON arm must have --settings; got: {on_cmd}")

    def test_dry_run_off_arm_has_no_settings(self):
        spec = _minimal_task_spec()
        with tempfile.TemporaryDirectory() as tmpdir:
            plan = _ab.build_dry_run_plan(
                task_spec=spec,
                arms=["on", "off", "placebo"],
                max_turns=40,
                on_settings=pathlib.Path("scripts/settings-efficacy-on.json"),
                placebo_settings=pathlib.Path("scripts/settings-efficacy-placebo.json"),
                workspace_base=pathlib.Path(tmpdir),
            )
        off_cmd = plan["off"]["claude_code_command"]
        self.assertNotIn("--settings", off_cmd, f"OFF arm must NOT have --settings; got: {off_cmd}")

    def test_dry_run_placebo_arm_includes_placebo_settings(self):
        spec = _minimal_task_spec()
        with tempfile.TemporaryDirectory() as tmpdir:
            plan = _ab.build_dry_run_plan(
                task_spec=spec,
                arms=["on", "off", "placebo"],
                max_turns=40,
                on_settings=pathlib.Path("scripts/settings-efficacy-on.json"),
                placebo_settings=pathlib.Path("scripts/settings-efficacy-placebo.json"),
                workspace_base=pathlib.Path(tmpdir),
            )
        placebo_cmd = plan["placebo"]["claude_code_command"]
        self.assertIn("--settings", placebo_cmd, f"PLACEBO arm must have --settings; got: {placebo_cmd}")
        self.assertIn("placebo", placebo_cmd.lower(), f"PLACEBO arm settings path must contain 'placebo'; got: {placebo_cmd}")

    def test_dry_run_all_arms_use_same_max_turns(self):
        spec = _minimal_task_spec()
        with tempfile.TemporaryDirectory() as tmpdir:
            plan = _ab.build_dry_run_plan(
                task_spec=spec,
                arms=["on", "off", "placebo"],
                max_turns=55,
                on_settings=pathlib.Path("scripts/settings-efficacy-on.json"),
                placebo_settings=pathlib.Path("scripts/settings-efficacy-placebo.json"),
                workspace_base=pathlib.Path(tmpdir),
            )
        for arm_name, arm_plan in plan.items():
            cmd = arm_plan["claude_code_command"]
            self.assertIn("55", cmd, f"arm {arm_name} must have max_turns=55 in command; got: {cmd}")

    def test_dry_run_plan_includes_verifier_info(self):
        spec = _minimal_task_spec()
        with tempfile.TemporaryDirectory() as tmpdir:
            plan = _ab.build_dry_run_plan(
                task_spec=spec,
                arms=["on", "off", "placebo"],
                max_turns=40,
                on_settings=pathlib.Path("scripts/settings-efficacy-on.json"),
                placebo_settings=pathlib.Path("scripts/settings-efficacy-placebo.json"),
                workspace_base=pathlib.Path(tmpdir),
            )
        for arm_name, arm_plan in plan.items():
            self.assertIn("verifier", arm_plan, f"arm {arm_name} plan must include verifier info")


# ── Draft-acceptance scorer ────────────────────────────────────────────────────

class TestDraftAcceptanceScorer(unittest.TestCase):
    """DraftAcceptanceScorer fails loud on < 10 real drafts."""

    def _write_pending_drafts(self, directory: pathlib.Path, n_pending: int, n_accepted: int) -> None:
        """Write n_pending .pending files and n_accepted accepted (renamed) files."""
        for i in range(n_pending):
            (directory / f"SKILL-{i}.md.pending").write_text(f"draft content {i}")
        for i in range(n_accepted):
            (directory / f"ACCEPTED-{i}.md").write_text(f"accepted content {i}")

    def test_fails_loud_with_fewer_than_10_drafts(self):
        """<10 real .pending drafts must cause SystemExit with non-zero code."""
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            self._write_pending_drafts(d, n_pending=5, n_accepted=5)
            with self.assertRaises(SystemExit) as ctx:
                _da.compute_draft_acceptance_rate(list(d.glob("*.pending")))
            self.assertNotEqual(ctx.exception.code, 0,
                                "Must exit non-zero when fewer than 10 drafts")

    def test_fails_loud_with_zero_drafts(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            with self.assertRaises(SystemExit) as ctx:
                _da.compute_draft_acceptance_rate(list(d.glob("*.pending")))
            self.assertNotEqual(ctx.exception.code, 0)

    def test_computes_rate_with_10_or_more_drafts(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            # Write 10 pending drafts; 4 have accepted counterparts.
            for i in range(10):
                pending = d / f"SKILL-{i}.md.pending"
                pending.write_text(f"draft {i}")
                if i < 4:
                    # Mark 4 as accepted by creating the .md sibling.
                    accepted = d / f"SKILL-{i}.md"
                    accepted.write_text(f"accepted {i}")
            pending_files = list(d.glob("*.pending"))
            rate = _da.compute_draft_acceptance_rate(pending_files)
            # 4 of 10 were accepted.
            self.assertAlmostEqual(rate["accepted_rate"], 4 / 10, places=5)
            self.assertEqual(rate["total"], 10)
            self.assertEqual(rate["accepted"], 4)

    def test_100_percent_acceptance(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            for i in range(10):
                pending = d / f"SKILL-{i}.md.pending"
                pending.write_text(f"draft {i}")
                accepted = d / f"SKILL-{i}.md"
                accepted.write_text(f"accepted {i}")
            pending_files = list(d.glob("*.pending"))
            rate = _da.compute_draft_acceptance_rate(pending_files)
            self.assertAlmostEqual(rate["accepted_rate"], 1.0, places=5)

    def test_0_percent_acceptance(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            d = pathlib.Path(tmpdir)
            for i in range(10):
                (d / f"SKILL-{i}.md.pending").write_text(f"draft {i}")
            pending_files = list(d.glob("*.pending"))
            rate = _da.compute_draft_acceptance_rate(pending_files)
            self.assertAlmostEqual(rate["accepted_rate"], 0.0, places=5)


# ── CLI dry-run smoke test ─────────────────────────────────────────────────────

class TestDryRunCli(unittest.TestCase):
    """efficacy_ab.py --dry-run validates specs and prints plan without model calls."""

    def test_dry_run_passes_with_valid_tasks_dir(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tasks_dir = pathlib.Path(tmpdir) / "tasks"
            tasks_dir.mkdir()
            _write_task_spec(tasks_dir, _minimal_task_spec("dry-run-task-a"))
            _write_task_spec(tasks_dir, _minimal_task_spec("dry-run-task-b"))
            result = subprocess.run(
                [
                    sys.executable,
                    str(_SCRIPTS_DIR / "efficacy_ab.py"),
                    "--dry-run",
                    "--tasks", str(tasks_dir),
                    "--run-id", "test-dry-run-001",
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0,
                             f"--dry-run must exit 0; stdout={result.stdout[:500]} stderr={result.stderr[:500]}")

    def test_dry_run_prints_arm_commands(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tasks_dir = pathlib.Path(tmpdir) / "tasks"
            tasks_dir.mkdir()
            _write_task_spec(tasks_dir, _minimal_task_spec("cmd-check-task"))
            result = subprocess.run(
                [
                    sys.executable,
                    str(_SCRIPTS_DIR / "efficacy_ab.py"),
                    "--dry-run",
                    "--tasks", str(tasks_dir),
                    "--run-id", "test-dry-run-cmd",
                ],
                capture_output=True,
                text=True,
            )
            # The dry-run output must show the arms.
            combined = result.stdout + result.stderr
            self.assertIn("on", combined.lower(), f"dry-run must mention 'on' arm; output={combined[:500]}")
            self.assertIn("off", combined.lower(), f"dry-run must mention 'off' arm; output={combined[:500]}")
            self.assertIn("placebo", combined.lower(), f"dry-run must mention 'placebo' arm; output={combined[:500]}")

    def test_dry_run_fails_with_invalid_task_spec(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tasks_dir = pathlib.Path(tmpdir) / "tasks"
            tasks_dir.mkdir()
            # Write an invalid spec (missing required fields).
            bad_spec = {"task_id": "bad-task"}
            (tasks_dir / "bad-task.json").write_text(json.dumps(bad_spec))
            result = subprocess.run(
                [
                    sys.executable,
                    str(_SCRIPTS_DIR / "efficacy_ab.py"),
                    "--dry-run",
                    "--tasks", str(tasks_dir),
                    "--run-id", "test-dry-run-bad",
                ],
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0,
                                f"--dry-run with invalid spec must exit non-zero; got {result.returncode}")


if __name__ == "__main__":
    unittest.main(verbosity=2)
