#!/usr/bin/env python3
"""T14 Efficacy A/B runner — 3-arm harness for the invented-rule task battery.

WHY this module exists
----------------------
T14 (2026-06-12) proves the one unmeasured number: does the skill layer make a
coding agent measurably better?  This is the harness that turns the invented-rule
task battery into the one honest efficacy verdict.

Three arms (byte-identical except skill-context injection):
  ON      — claude-code with compile_context hooks against the live mcp-server
  OFF     — identical claude-code invocation with NO skill-layer MCP / hooks
  PLACEBO — claude-code with matched-token-mass IRRELEVANT skill context;
            explicitly labeled as a measurement control, never a silent fallback

Standing rules (from CONTRACT.md and project memory):
  - Measurement drives the REAL mcp-server over HTTP end-to-end.
  - No fakes: placebo is an explicitly-labeled control, not a production path.
  - No arbitrary caps: --max-turns is sized to let a solve finish; record the value.
  - Serialized heavy actions: the live solve loop runs in Unit 4, one at a time.
  - Fail loud: missing config, bad spec, or <10 drafts exit non-zero immediately.

Pre-registered criterion (verbatim, per LOCKED block in T14 ticket):
  "ON wins ≥ 7 of 10 paired tasks by sign test, with no catastrophic regression
   on any single task."

Gate outcomes (LOCKED pre-registration):
  PASS              — ON wins ≥ 7 of N=10 by sign test AND no catastrophic regression
  UNDERPOWERED      — positive direction but below the bar, or sign test cannot
                      distinguish at N; a null result is UNDERPOWERED, not "no effect"
  FAIL              — ON ≤ OFF overall
  INSTRUMENT-FAILURE — any task where ON fails with attribution-confirmed rule injection;
                       blocks all efficacy verdicts until injection path is fixed

Usage
-----
  python3 scripts/efficacy_ab.py --dry-run --tasks tests/e2e/efficacy/tasks/
  python3 scripts/efficacy_ab.py --run-id <id> --tasks <dir> [--arms on,off,placebo]
  python3 scripts/efficacy_ab.py --self-test
"""
import argparse
import contextlib
import json
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

# Allow imports from the scripts/ directory (same pattern as retrieval_sweep.py).
_SCRIPTS_DIR = Path(__file__).parent.resolve()
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

# Reuse sign_test and paired_rank_diffs from the shared measurement lib (T20).
# Do NOT re-implement these — the standing rule is to reuse T20's machinery.
import retrieval_metrics as _metrics  # noqa: E402

# ── Pre-registered criterion (verbatim, LOCKED) ───────────────────────────────

PRE_REGISTERED_CRITERION = (
    "ON wins ≥ 7 of 10 paired tasks by sign test, "
    "with no catastrophic regression on any single task."
)

# Minimum number of ON wins to claim PASS (pre-registered: 7 of 10).
PASS_THRESHOLD = 7
# Total expected tasks in a full run.
EXPECTED_TASK_COUNT = 10

# Report output home.
REPORT_DIR_DEFAULT = Path("tests/e2e/reports/efficacy")


# ── CONTRACT schema validation ─────────────────────────────────────────────────

# Required top-level keys in every task spec.
_REQUIRED_TOP_LEVEL_KEYS: tuple[str, ...] = (
    "task_id",
    "title",
    "invented_rule",
    "prompt",
    "workspace",
    "verifier",
    "expected",
)

# Required keys inside the invented_rule block.
_REQUIRED_INVENTED_RULE_KEYS: tuple[str, ...] = (
    "summary",
    "corpus_skill_slug",
    "corpus_skill_id",
    "absent_from_pretraining_rationale",
)

# Required keys inside the verifier block.
_REQUIRED_VERIFIER_KEYS: tuple[str, ...] = ("command", "contract")

# Required keys inside the workspace block.
_REQUIRED_WORKSPACE_KEYS: tuple[str, ...] = ("kind",)

# Required keys inside the expected block.
_REQUIRED_EXPECTED_KEYS: tuple[str, ...] = ("on", "off", "placebo", "sensitivity_note")


def validate_task_spec(spec: dict[str, Any]) -> list[str]:
    """Validate a task spec against the CONTRACT schema.

    Returns a list of issue strings.  An empty list means the spec is valid.
    Does NOT make network calls — purely structural validation.

    Args:
        spec: the parsed task spec dict.
    """
    issues: list[str] = []

    for key in _REQUIRED_TOP_LEVEL_KEYS:
        if key not in spec:
            issues.append(f"missing required top-level key: '{key}'")

    # Validate nested blocks only when the parent key is present (avoid cascading errors).
    if "invented_rule" in spec:
        rule = spec["invented_rule"]
        if not isinstance(rule, dict):
            issues.append("invented_rule must be a dict")
        else:
            for key in _REQUIRED_INVENTED_RULE_KEYS:
                if key not in rule:
                    issues.append(f"invented_rule missing required key: '{key}'")

    if "verifier" in spec:
        verifier = spec["verifier"]
        if not isinstance(verifier, dict):
            issues.append("verifier must be a dict")
        else:
            for key in _REQUIRED_VERIFIER_KEYS:
                if key not in verifier:
                    issues.append(f"verifier missing required key: '{key}'")

    if "workspace" in spec:
        workspace = spec["workspace"]
        if not isinstance(workspace, dict):
            issues.append("workspace must be a dict")
        else:
            for key in _REQUIRED_WORKSPACE_KEYS:
                if key not in workspace:
                    issues.append(f"workspace missing required key: '{key}'")

    if "expected" in spec:
        expected = spec["expected"]
        if not isinstance(expected, dict):
            issues.append("expected must be a dict")
        else:
            for key in _REQUIRED_EXPECTED_KEYS:
                if key not in expected:
                    issues.append(f"expected missing required key: '{key}'")

    return issues


def load_task_specs(tasks_dir: Path) -> list[dict[str, Any]]:
    """Load and validate all task spec JSON files from a directory.

    Fails loud (sys.exit non-zero) if:
      - The directory contains no .json files.
      - Any spec fails CONTRACT validation.

    Args:
        tasks_dir: directory containing <task_id>.json files.

    Returns:
        List of validated task spec dicts.
    """
    json_files = sorted(tasks_dir.glob("*.json"))
    if not json_files:
        print(
            f"ERROR: no task spec .json files found in {tasks_dir}",
            file=sys.stderr,
        )
        sys.exit(1)

    specs: list[dict[str, Any]] = []
    any_invalid = False
    for spec_file in json_files:
        try:
            spec = json.loads(spec_file.read_text())
        except json.JSONDecodeError as exc:
            print(f"ERROR: failed to parse {spec_file}: {exc}", file=sys.stderr)
            any_invalid = True
            continue

        issues = validate_task_spec(spec)
        if issues:
            print(f"ERROR: task spec {spec_file} fails CONTRACT validation:", file=sys.stderr)
            for issue in issues:
                print(f"  - {issue}", file=sys.stderr)
            any_invalid = True
        else:
            specs.append(spec)

    if any_invalid:
        sys.exit(1)

    return specs


# ── Verifier runner ────────────────────────────────────────────────────────────

def map_verifier_exit_code_to_outcome(exit_code: int, reason: str) -> dict[str, Any]:
    """Map a verifier's exit code to an outcome record.

    Args:
        exit_code:  the verifier process exit code (0 = win, non-zero = loss).
        reason:     the one-line human reason printed by the verifier to stdout.

    Returns:
        Dict with keys: outcome (str "win"|"loss"), exit_code (int), verifier_reason (str).
    """
    outcome = "win" if exit_code == 0 else "loss"
    return {
        "outcome": outcome,
        "exit_code": exit_code,
        "verifier_reason": reason,
    }


def run_verifier(verifier_command: str, workspace_dir: Path) -> dict[str, Any]:
    """Run the deterministic verifier script against the post-solve workspace.

    The verifier is invoked as ``<command> <workspace_dir>``.  Exit 0 = rule obeyed
    (win); non-zero = rule NOT obeyed (loss).  The verifier's stdout is captured as
    the human reason.

    No network calls, no model calls — verifiers are pure deterministic inspection
    per the CONTRACT.

    Args:
        verifier_command: path to the verifier shell script.
        workspace_dir:    the post-solve workspace directory to inspect.

    Returns:
        Outcome dict as returned by map_verifier_exit_code_to_outcome().
    """
    try:
        proc = subprocess.run(
            [verifier_command, str(workspace_dir)],
            capture_output=True,
            text=True,
            timeout=60,
        )
        reason = proc.stdout.strip() or proc.stderr.strip() or "(verifier produced no output)"
        return map_verifier_exit_code_to_outcome(proc.returncode, reason)
    except FileNotFoundError:
        return {
            "outcome": "error",
            "exit_code": -1,
            "verifier_reason": f"verifier not found: {verifier_command}",
        }
    except subprocess.TimeoutExpired:
        return {
            "outcome": "error",
            "exit_code": -2,
            "verifier_reason": f"verifier timed out: {verifier_command}",
        }


# ── Attribution parser ─────────────────────────────────────────────────────────

def parse_retrieval_attribution(transcript: dict[str, Any]) -> list[dict[str, Any]]:
    """Label each retrieval pull in a solve transcript by trigger type.

    Trigger labels:
      session_start_priming   — pull triggered by SessionStart hook
      mid_session_find_skill  — pull triggered by UserPromptSubmit hook

    These labels are recorded in the per-task report to distinguish priming
    (bulk injection at session start) from targeted mid-session retrieval.
    Attribution is REQUIRED to interpret INSTRUMENT-FAILURE: if ON fails a task
    whose invented-rule skill WAS injected, the injection path is working but
    the agent didn't use it — that is a different class of failure than the skill
    never arriving.

    Args:
        transcript: dict with a "retrieval_pulls" list (each pull has "trigger",
                    "tool", "skills_returned", and optionally "skill_ids_returned").
                    Missing key returns empty list — do NOT raise.

    Returns:
        List of pull dicts augmented with a "label" key.
    """
    pulls = transcript.get("retrieval_pulls", [])
    labeled: list[dict[str, Any]] = []
    for pull in pulls:
        trigger = pull.get("trigger", "")
        if trigger == "SessionStart":
            label = "session_start_priming"
        else:
            # All non-SessionStart triggers (UserPromptSubmit, direct find_skill) are
            # classified as mid-session retrieval.
            label = "mid_session_find_skill"
        labeled.append({**pull, "label": label})
    return labeled


def attribution_confirms_skill_injected(
    labeled_pulls: list[dict[str, Any]],
    corpus_skill_id: str,
) -> bool:
    """Return True if the given corpus_skill_id appears in any labeled pull.

    Used to distinguish INSTRUMENT-FAILURE (skill was injected but agent failed)
    from a retrieval-miss (skill was never fetched).

    Args:
        labeled_pulls:   output of parse_retrieval_attribution().
        corpus_skill_id: the UUID from the task spec's invented_rule.corpus_skill_id.
    """
    for pull in labeled_pulls:
        skill_ids = pull.get("skill_ids_returned", [])
        if corpus_skill_id in skill_ids:
            return True
    return False


# ── Gate classifier ────────────────────────────────────────────────────────────

def classify_efficacy_verdict(per_task_results: list[dict[str, Any]]) -> dict[str, Any]:
    """Classify the efficacy run into PASS / FAIL / UNDERPOWERED / INSTRUMENT-FAILURE.

    Implements EXACTLY the LOCKED pre-registered semantics from T14:

    INSTRUMENT-FAILURE: any task where ON fails AND instrument_failure=True
      (i.e. attribution confirms the invented-rule skill was injected). This
      blocks all other verdicts.

    PASS: ON wins ≥ PASS_THRESHOLD (7) of N (10) paired tasks by sign test,
      AND no catastrophic_regression on any single task.

    UNDERPOWERED: ON direction is positive (more ON wins than OFF wins among
      discordant pairs) but below the threshold, OR the sign test cannot
      distinguish at N.  A null result (all ties) is UNDERPOWERED — not
      spun as "no effect."

    FAIL: ON ≤ OFF overall (OFF wins more discordant pairs than ON).

    Args:
        per_task_results: list of per-task result dicts, each with keys:
          task_id, on_outcome, off_outcome, placebo_outcome,
          catastrophic_regression (bool), attribution (list),
          instrument_failure (bool, optional).

    Returns:
        Dict with keys: verdict, pre_registered_criterion, sign_test_p_value,
        on_wins, off_wins, n_ties, n_tasks, catastrophic_regression_tasks.
    """
    n_tasks = len(per_task_results)

    # ── INSTRUMENT-FAILURE check (blocks all other verdicts) ──────────────────
    instrument_failures = [
        r["task_id"]
        for r in per_task_results
        if r.get("instrument_failure", False)
    ]
    if instrument_failures:
        return {
            "verdict": "INSTRUMENT-FAILURE",
            "pre_registered_criterion": PRE_REGISTERED_CRITERION,
            "instrument_failure_tasks": instrument_failures,
            "sign_test_p_value": None,
            "on_wins": None,
            "off_wins": None,
            "n_ties": None,
            "n_tasks": n_tasks,
            "catastrophic_regression_tasks": [],
            "detail": (
                "INSTRUMENT-FAILURE: the invented-rule skill was attribution-confirmed injected "
                "but ON failed the task(s). The injection path is broken. No efficacy verdict "
                "may be claimed until the path is fixed."
            ),
        }

    # ── Catastrophic regression check ─────────────────────────────────────────
    catastrophic_tasks = [
        r["task_id"]
        for r in per_task_results
        if r.get("catastrophic_regression", False)
    ]

    # ── Paired win counts (discordant pairs only) ──────────────────────────────
    # A "win" for ON in the paired design: ON verifier exit 0, OFF verifier non-0.
    # Ties (both win or both fail) are excluded from the sign test per CONTRACT.
    on_wins = 0
    off_wins = 0
    n_ties = 0
    for r in per_task_results:
        on_w = r["on_outcome"] == "win"
        off_w = r["off_outcome"] == "win"
        if on_w and not off_w:
            on_wins += 1
        elif off_w and not on_w:
            off_wins += 1
        else:
            n_ties += 1

    # Reuse sign_test from the shared measurement lib (T20 mandate).
    # sign_test(n_a_better, n_b_better) — here A=ON, B=OFF.
    sign_test_p = _metrics.sign_test(on_wins, off_wins)

    # ── Verdict classification (pre-registered order) ──────────────────────────
    # Per LOCKED pre-registration (T14 ticket):
    #   UNDERPOWERED: positive direction but < bar, OR sign test cannot distinguish at N.
    #   FAIL: ON ≤ OFF (strictly fewer discordant pairs won by ON than OFF).
    #
    # A perfectly balanced result (on_wins == off_wins, p=1.0) is UNDERPOWERED:
    # the sign test cannot distinguish the arms — this is a null result, not a
    # directional failure.  "No effect" is UNDERPOWERED, not FAIL.
    # FAIL requires OFF to strictly beat ON (off_wins > on_wins).
    if off_wins > on_wins:
        verdict = "FAIL"
    elif on_wins >= PASS_THRESHOLD and not catastrophic_tasks:
        verdict = "PASS"
    elif on_wins >= PASS_THRESHOLD and catastrophic_tasks:
        # Would be PASS on count, but catastrophic regression prevents it.
        verdict = "FAIL"
    else:
        # on_wins >= off_wins but below PASS_THRESHOLD, OR tied (cannot distinguish).
        verdict = "UNDERPOWERED"

    return {
        "verdict": verdict,
        "pre_registered_criterion": PRE_REGISTERED_CRITERION,
        "sign_test_p_value": sign_test_p,
        "on_wins": on_wins,
        "off_wins": off_wins,
        "n_ties": n_ties,
        "n_tasks": n_tasks,
        "catastrophic_regression_tasks": catastrophic_tasks,
        "detail": _build_verdict_detail(verdict, on_wins, off_wins, n_ties, sign_test_p, catastrophic_tasks),
    }


def _build_verdict_detail(
    verdict: str,
    on_wins: int,
    off_wins: int,
    n_ties: int,
    sign_test_p: float,
    catastrophic_tasks: list[str],
) -> str:
    """Build a human-readable detail string for the verdict.

    Args:
        verdict:           the gate verdict string.
        on_wins:           discordant pairs where ON won.
        off_wins:          discordant pairs where OFF won.
        n_ties:            tied pairs.
        sign_test_p:       two-sided exact binomial sign test p-value.
        catastrophic_tasks: list of task IDs with catastrophic regression.
    """
    base = (
        f"{verdict}: ON wins {on_wins}, OFF wins {off_wins}, ties {n_ties}; "
        f"sign test p={sign_test_p:.4f}"
    )
    if catastrophic_tasks:
        base += f"; CATASTROPHIC REGRESSION on task(s): {', '.join(catastrophic_tasks)}"
    if verdict == "UNDERPOWERED":
        base += (
            " — positive direction but below the pre-registered bar of "
            f"{PASS_THRESHOLD}/{EXPECTED_TASK_COUNT}. "
            "This is a null result: UNDERPOWERED, not 'no effect'."
        )
    return base


# ── Report emitter ─────────────────────────────────────────────────────────────

def render_efficacy_report(run_summary: dict[str, Any]) -> dict[str, Any]:
    """Render a run summary into a per-run JSON data dict + human text.

    The human text includes the pre-registered criterion verbatim, per the
    CONTRACT requirement.  Null/negative/underpowered results are documented
    honestly with raw data.

    Args:
        run_summary: dict with keys: run_id, per_task_results,
                     verdict_summary, arms_used, max_turns.

    Returns:
        Dict with keys:
          json_data   — structured report dict (serializable to JSON)
          human_text  — multi-line string for the .txt report file
    """
    run_id = run_summary["run_id"]
    per_task = run_summary["per_task_results"]
    verdict = run_summary["verdict_summary"]
    arms_used = run_summary.get("arms_used", ["on", "off", "placebo"])
    max_turns = run_summary.get("max_turns", "N/A")

    # Build ON-vs-PLACEBO comparison.
    on_vs_placebo = _compute_on_vs_placebo(per_task)

    # Build per-task paired win/loss/tie table.
    per_task_table = [
        {
            "task_id": r["task_id"],
            "on_outcome": r["on_outcome"],
            "off_outcome": r["off_outcome"],
            "placebo_outcome": r["placebo_outcome"],
            "paired": _paired_label(r["on_outcome"], r["off_outcome"]),
            "catastrophic_regression": r.get("catastrophic_regression", False),
            "attribution": r.get("attribution", []),
        }
        for r in per_task
    ]

    json_data: dict[str, Any] = {
        "run_id": run_id,
        "verdict": verdict["verdict"],
        "pre_registered_criterion": PRE_REGISTERED_CRITERION,
        "sign_test_p_value": verdict.get("sign_test_p_value"),
        "on_wins": verdict.get("on_wins"),
        "off_wins": verdict.get("off_wins"),
        "n_ties": verdict.get("n_ties"),
        "n_tasks": verdict.get("n_tasks"),
        "arms_used": arms_used,
        "max_turns": max_turns,
        "per_task_table": per_task_table,
        "on_vs_placebo": on_vs_placebo,
        "attribution_per_task": {
            r["task_id"]: r.get("attribution", []) for r in per_task
        },
        "verdict_detail": verdict.get("detail", ""),
        "catastrophic_regression_tasks": verdict.get("catastrophic_regression_tasks", []),
    }

    human_lines = [
        f"=== T14 Efficacy A/B Run Report ===",
        f"Run ID:   {run_id}",
        f"Arms:     {', '.join(arms_used)}",
        f"Max turns (stuck-detector, not a cap): {max_turns}",
        "",
        f"Pre-registered criterion (verbatim):",
        f"  \"{PRE_REGISTERED_CRITERION}\"",
        "",
        f"VERDICT: {verdict['verdict']}",
        f"  {verdict.get('detail', '')}",
        "",
        "Per-task paired win/loss/tie table:",
    ]
    for row in per_task_table:
        cat = ""
        if row.get("catastrophic_regression"):
            cat = " *** CATASTROPHIC REGRESSION ***"
        human_lines.append(
            f"  {row['task_id']:40s}  ON={row['on_outcome']:4s}  OFF={row['off_outcome']:4s}"
            f"  PLACEBO={row['placebo_outcome']:4s}  paired={row['paired']}{cat}"
        )
    human_lines += [
        "",
        f"Sign test p-value: {verdict.get('sign_test_p_value')}",
        "",
        "ON vs PLACEBO comparison:",
        f"  ON wins vs PLACEBO:  {on_vs_placebo['on_beats_placebo']}",
        f"  PLACEBO wins vs ON:  {on_vs_placebo['placebo_beats_on']}",
        f"  Ties:                {on_vs_placebo['ties']}",
        "",
        "Attribution per task (retrieval pulls labeled by trigger):",
    ]
    attr_by_task = json_data["attribution_per_task"]
    for task_id, pulls in attr_by_task.items():
        human_lines.append(f"  {task_id}:")
        if not pulls:
            human_lines.append("    (no retrieval pulls recorded)")
        for pull in pulls:
            label = pull.get("label", "?")
            skills = pull.get("skills_returned", [])
            human_lines.append(f"    [{label}] skills={skills}")
    human_lines.append("")

    return {
        "json_data": json_data,
        "human_text": "\n".join(human_lines),
    }


def _paired_label(on_outcome: str, off_outcome: str) -> str:
    """Return a short label for the paired outcome of one task."""
    on_w = on_outcome == "win"
    off_w = off_outcome == "win"
    if on_w and not off_w:
        return "ON>OFF"
    elif off_w and not on_w:
        return "OFF>ON"
    else:
        return "tie"


def _compute_on_vs_placebo(per_task: list[dict[str, Any]]) -> dict[str, int]:
    """Compute the ON-vs-PLACEBO paired comparison across all tasks."""
    on_beats = 0
    placebo_beats = 0
    ties = 0
    for r in per_task:
        on_w = r["on_outcome"] == "win"
        pl_w = r["placebo_outcome"] == "win"
        if on_w and not pl_w:
            on_beats += 1
        elif pl_w and not on_w:
            placebo_beats += 1
        else:
            ties += 1
    return {"on_beats_placebo": on_beats, "placebo_beats_on": placebo_beats, "ties": ties}


def write_efficacy_report(report: dict[str, Any], out_dir: Path) -> None:
    """Write the rendered efficacy report to disk.

    Creates ``out_dir`` (including parents) if it doesn't exist.

    Writes:
      <out_dir>/report.json  — the structured JSON report data
      <out_dir>/report.txt   — the human-readable text report

    Args:
        report:  output of render_efficacy_report().
        out_dir: target directory (created if absent).
    """
    out_dir.mkdir(parents=True, exist_ok=True)
    json_path = out_dir / "report.json"
    txt_path = out_dir / "report.txt"
    json_path.write_text(json.dumps(report["json_data"], indent=2))
    txt_path.write_text(report["human_text"])
    print(f"  report written → {out_dir}/report.json + report.txt", flush=True)


# ── Dry-run plan builder ───────────────────────────────────────────────────────

def build_dry_run_plan(
    task_spec: dict[str, Any],
    arms: list[str],
    max_turns: int,
    on_settings: Path,
    placebo_settings: Path,
    workspace_base: Path,
) -> dict[str, dict[str, Any]]:
    """Build the per-arm claude-code command plans WITHOUT running anything.

    The three arms are byte-identical except for the --settings flag:
      ON      — --settings <on_settings>
      OFF     — no --settings
      PLACEBO — --settings <placebo_settings>

    The command shape mirrors scripts/swebench/README.md:
      claude --settings <f> --print --dangerously-skip-permissions
             --max-turns N --add-dir <workspace> "<prompt>"

    Args:
        task_spec:        validated task spec dict.
        arms:             list of arm names to include ("on", "off", "placebo").
        max_turns:        number of claude-code turns (sized to let solve finish).
        on_settings:      path to ON arm settings JSON.
        placebo_settings: path to PLACEBO arm settings JSON.
        workspace_base:   base directory for workspace materialization.

    Returns:
        Dict keyed by arm name, each value is a dict with:
          claude_code_command — the shell command string that WOULD be run
          workspace_dir       — the workspace directory path
          verifier            — the verifier command and contract
    """
    task_id = task_spec["task_id"]
    prompt = task_spec["prompt"]
    verifier = task_spec["verifier"]

    plans: dict[str, dict[str, Any]] = {}
    for arm in arms:
        workspace_dir = workspace_base / f"{task_id}__{arm}"

        if arm == "on":
            settings_arg = f"--settings {on_settings}"
        elif arm == "placebo":
            settings_arg = f"--settings {placebo_settings}"
        else:
            # OFF arm: no --settings, no MCP.
            settings_arg = ""

        cmd_parts = ["claude"]
        if settings_arg:
            cmd_parts.extend(settings_arg.split())
        cmd_parts += [
            "--print",
            "--dangerously-skip-permissions",
            "--max-turns", str(max_turns),
            "--add-dir", str(workspace_dir),
            f'"{prompt}"',
        ]
        cmd = " ".join(cmd_parts)

        plans[arm] = {
            "claude_code_command": cmd,
            "workspace_dir": str(workspace_dir),
            "verifier": {
                "command": verifier["command"],
                "contract": verifier.get("contract", "Exit 0 == rule obeyed."),
            },
            "arm": arm,
            "task_id": task_id,
        }

    return plans


# ── Main CLI ───────────────────────────────────────────────────────────────────

def _dry_run(args: argparse.Namespace) -> int:
    """Execute the --dry-run path: validate specs, print plans, exercise gate code.

    Does NOT spend model calls.  Exercises the scoring/gate/attribution code paths
    on synthetic per-task results to prove they work before a live run.

    Returns:
        0 on success, 1 on failure.
    """
    tasks_dir = Path(args.tasks)
    if not tasks_dir.is_dir():
        print(f"ERROR: --tasks directory not found: {tasks_dir}", file=sys.stderr)
        return 1

    print(f"\n=== T14 Efficacy A/B — DRY RUN ===")
    print(f"Tasks dir:  {tasks_dir}")
    print(f"Run ID:     {args.run_id}")
    print(f"Arms:       {', '.join(args.arms)}")
    print(f"Max turns:  {args.max_turns} (sized to let a real solve finish — NOT a work cap)")
    print(f"ON settings:      {args.on_settings}")
    print(f"PLACEBO settings: {args.placebo_settings}")
    print()

    # Load and validate specs — fails loud on any schema violation.
    specs = load_task_specs(tasks_dir)
    print(f"Loaded {len(specs)} task spec(s); all pass CONTRACT validation.")
    print()

    # Print per-task per-arm plan.
    for spec in specs:
        print(f"── Task: {spec['task_id']} ──────────────────────────────────────────────")
        print(f"  Title:    {spec['title']}")
        print(f"  Rule:     {spec['invented_rule']['summary']}")
        print(f"  Prompt:   {spec['prompt'][:80]}{'...' if len(spec['prompt']) > 80 else ''}")
        with _temp_workspace_dir() as ws_base:
            plan = build_dry_run_plan(
                task_spec=spec,
                arms=args.arms,
                max_turns=args.max_turns,
                on_settings=Path(args.on_settings),
                placebo_settings=Path(args.placebo_settings),
                workspace_base=ws_base,
            )
        for arm_name, arm_plan in plan.items():
            print(f"  [{arm_name.upper():8s}] {arm_plan['claude_code_command']}")
        print(f"  Verifier: {spec['verifier']['command']} <workspace>")
        print()

    # Exercise gate/scoring code paths on synthetic results.
    print("── Exercising gate/scoring code paths on synthetic results ──────────────")
    synthetic_results = _build_synthetic_dry_run_results(specs)
    verdict = classify_efficacy_verdict(synthetic_results)
    print(f"  Synthetic verdict: {verdict['verdict']}")
    print(f"  Pre-registered criterion: \"{PRE_REGISTERED_CRITERION}\"")
    print(f"  ON wins: {verdict['on_wins']}, OFF wins: {verdict['off_wins']}, "
          f"ties: {verdict['n_ties']}")
    print(f"  Sign test p-value: {verdict['sign_test_p_value']}")
    print()

    print("── Dry-run complete: no model calls spent ───────────────────────────────")
    return 0


def _build_synthetic_dry_run_results(specs: list[dict]) -> list[dict]:
    """Build synthetic per-task results for dry-run gate exercise.

    Uses a predictable pattern: first half of tasks have ON winning, second half
    have OFF winning.  This exercises both PASS/FAIL paths in the gate code.
    """
    results = []
    n = len(specs)
    for i, spec in enumerate(specs):
        on_wins = i < (n // 2 + 1)
        results.append({
            "task_id": spec["task_id"],
            "on_outcome": "win" if on_wins else "loss",
            "off_outcome": "loss" if on_wins else "win",
            "placebo_outcome": "loss",
            "catastrophic_regression": False,
            "attribution": [],
        })
    return results


@contextlib.contextmanager
def _temp_workspace_dir():
    """Context manager yielding a temp directory Path for dry-run workspace planning."""
    with tempfile.TemporaryDirectory() as tmpdir:
        yield Path(tmpdir)


def _self_test() -> int:
    """Run the module's own self-tests.  Returns 0 if all pass, 1 if any fail."""
    print("=== efficacy_ab self-test ===")
    failures = 0

    def _assert(cond: bool, label: str, detail: str = "") -> bool:
        status = "PASS" if cond else "FAIL"
        suffix = f"  [{detail}]" if detail else ""
        print(f"  {status}  {label}{suffix}")
        return cond

    # ── validate_task_spec ─────────────────────────────────────────────────
    print("\n-- validate_task_spec --")
    minimal = {
        "task_id": "t", "title": "T",
        "invented_rule": {
            "summary": "s", "corpus_skill_slug": "sl",
            "corpus_skill_id": "id", "absent_from_pretraining_rationale": "r",
        },
        "prompt": "p",
        "workspace": {"kind": "scratch", "base_ref": None, "setup": []},
        "verifier": {"command": "v.sh", "contract": "c"},
        "expected": {"on": "pass", "off": "fail", "placebo": "fail", "sensitivity_note": "n"},
    }
    ok = _assert(validate_task_spec(minimal) == [], "valid minimal spec → 0 issues")
    failures += 0 if ok else 1

    broken = {"task_id": "only"}
    issues = validate_task_spec(broken)
    ok = _assert(len(issues) > 0, "spec missing fields → issues", f"got {issues}")
    failures += 0 if ok else 1

    # ── map_verifier_exit_code_to_outcome ──────────────────────────────────
    print("\n-- map_verifier_exit_code_to_outcome --")
    r = map_verifier_exit_code_to_outcome(0, "obeyed")
    ok = _assert(r["outcome"] == "win", "exit 0 → win", f"got {r}")
    failures += 0 if ok else 1

    r = map_verifier_exit_code_to_outcome(1, "not obeyed")
    ok = _assert(r["outcome"] == "loss", "exit 1 → loss", f"got {r}")
    failures += 0 if ok else 1

    # ── parse_retrieval_attribution ────────────────────────────────────────
    print("\n-- parse_retrieval_attribution --")
    t = {"retrieval_pulls": [
        {"trigger": "SessionStart", "tool": "compile_context", "skills_returned": ["s1"], "timestamp": "T"},
        {"trigger": "UserPromptSubmit", "tool": "compile_context", "skills_returned": ["s2"], "timestamp": "T"},
    ]}
    labels = parse_retrieval_attribution(t)
    ok = _assert(labels[0]["label"] == "session_start_priming", "SessionStart → priming", f"got {labels[0]['label']}")
    failures += 0 if ok else 1
    ok = _assert(labels[1]["label"] == "mid_session_find_skill", "UserPromptSubmit → mid-session", f"got {labels[1]['label']}")
    failures += 0 if ok else 1

    # ── classify_efficacy_verdict ──────────────────────────────────────────
    print("\n-- classify_efficacy_verdict --")

    def _task(on_w, off_w, pl_w="loss", cat=False, inst=False):
        return {
            "task_id": "t", "on_outcome": "win" if on_w else "loss",
            "off_outcome": "win" if off_w else "loss",
            "placebo_outcome": pl_w,
            "catastrophic_regression": cat,
            "attribution": [],
            "instrument_failure": inst,
        }

    # 7/10 ON wins → PASS
    tasks = [_task(True, False)] * 7 + [_task(False, True)] * 3
    v = classify_efficacy_verdict(tasks)
    ok = _assert(v["verdict"] == "PASS", "7/10 ON wins → PASS", f"got {v['verdict']}")
    failures += 0 if ok else 1

    # 5/10 ON wins → UNDERPOWERED
    tasks = [_task(True, False)] * 5 + [_task(False, True)] * 5
    v = classify_efficacy_verdict(tasks)
    ok = _assert(v["verdict"] == "UNDERPOWERED", "5/10 → UNDERPOWERED", f"got {v['verdict']}")
    failures += 0 if ok else 1

    # 3/10 ON wins → FAIL
    tasks = [_task(True, False)] * 3 + [_task(False, True)] * 7
    v = classify_efficacy_verdict(tasks)
    ok = _assert(v["verdict"] == "FAIL", "3/10 → FAIL", f"got {v['verdict']}")
    failures += 0 if ok else 1

    # INSTRUMENT-FAILURE
    tasks = [_task(True, False)] * 9 + [_task(False, False, inst=True)]
    v = classify_efficacy_verdict(tasks)
    ok = _assert(v["verdict"] == "INSTRUMENT-FAILURE", "instrument_failure → INSTRUMENT-FAILURE", f"got {v['verdict']}")
    failures += 0 if ok else 1

    # Criterion string present
    tasks = [_task(True, False)] * 7 + [_task(False, True)] * 3
    v = classify_efficacy_verdict(tasks)
    ok = _assert(v["pre_registered_criterion"] == PRE_REGISTERED_CRITERION,
                 "criterion string verbatim", f"got '{v['pre_registered_criterion']}'")
    failures += 0 if ok else 1

    # ── render_efficacy_report ─────────────────────────────────────────────
    print("\n-- render_efficacy_report --")
    tasks_for_report = [_task(True, False)] * 7 + [_task(False, True)] * 3
    for r in tasks_for_report:
        r["task_id"] = f"task-{tasks_for_report.index(r)}"

    summary = {
        "run_id": "self-test-001",
        "per_task_results": tasks_for_report,
        "verdict_summary": classify_efficacy_verdict(tasks_for_report),
        "arms_used": ["on", "off", "placebo"],
        "max_turns": 40,
    }
    report = render_efficacy_report(summary)
    ok = _assert(PRE_REGISTERED_CRITERION in report["human_text"],
                 "criterion verbatim in human text")
    failures += 0 if ok else 1
    ok = _assert("pre_registered_criterion" in report["json_data"],
                 "pre_registered_criterion in json_data")
    failures += 0 if ok else 1

    print(f"\n{'=' * 40}")
    if failures == 0:
        print("ALL TESTS PASSED")
    else:
        print(f"{failures} TEST(S) FAILED", file=sys.stderr)
    return 0 if failures == 0 else 1


def main() -> None:
    """CLI entry point for the T14 efficacy A/B runner."""
    ap = argparse.ArgumentParser(
        description=(
            "T14 Efficacy A/B runner — 3-arm harness for the invented-rule task battery. "
            "Use --dry-run to validate specs and print the execution plan without model calls."
        )
    )
    ap.add_argument(
        "--dry-run",
        action="store_true",
        help=(
            "Validate task specs, print per-arm execution plans, and exercise the "
            "gate/scoring code on synthetic results — WITHOUT spending model calls."
        ),
    )
    ap.add_argument(
        "--run-id",
        default=f"efficacy-{int(time.time())}",
        help="Unique run identifier for output directory naming.",
    )
    ap.add_argument(
        "--tasks",
        default="tests/e2e/efficacy/tasks",
        help="Directory containing task spec JSON files.",
    )
    ap.add_argument(
        "--arms",
        default="on,off,placebo",
        type=lambda s: [a.strip() for a in s.split(",")],
        help="Comma-separated arms to run (default: on,off,placebo).",
    )
    ap.add_argument(
        "--max-turns",
        dest="max_turns",
        type=int,
        default=40,
        help=(
            "claude-code --max-turns value. Sized to let a real solve finish. "
            "This is NOT a work cap — it is a stuck-detector deadline only. "
            "Record the value used in the report."
        ),
    )
    ap.add_argument(
        "--on-settings",
        dest="on_settings",
        default="scripts/settings-efficacy-on.json",
        help="Path to the ON arm claude-code settings JSON.",
    )
    ap.add_argument(
        "--placebo-settings",
        dest="placebo_settings",
        default="scripts/settings-efficacy-placebo.json",
        help="Path to the PLACEBO arm claude-code settings JSON.",
    )
    ap.add_argument(
        "--output-dir",
        dest="output_dir",
        default=str(REPORT_DIR_DEFAULT),
        help="Base directory for run output (default: tests/e2e/reports/efficacy/).",
    )
    ap.add_argument(
        "--self-test",
        action="store_true",
        help="Run module self-tests and exit.",
    )

    args = ap.parse_args()

    if args.self_test:
        sys.exit(_self_test())

    if args.dry_run:
        sys.exit(_dry_run(args))

    # Live run path: validate args then hand off.
    # (The actual solve loop is run by the orchestrator in Unit 4, serialized.)
    tasks_dir = Path(args.tasks)
    if not tasks_dir.is_dir():
        print(f"ERROR: --tasks directory not found: {tasks_dir}", file=sys.stderr)
        sys.exit(1)
    specs = load_task_specs(tasks_dir)
    print(
        f"T14 efficacy run: {len(specs)} task(s), arms={args.arms}, "
        f"run-id={args.run_id}",
        flush=True,
    )
    print(
        "Live solve loop not yet implemented (Unit 4 orchestrator). "
        "Use --dry-run to validate the plan.",
        file=sys.stderr,
    )
    sys.exit(1)


if __name__ == "__main__":
    main()
