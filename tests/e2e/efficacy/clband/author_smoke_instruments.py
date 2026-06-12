#!/usr/bin/env python3
"""Author the T14 clband SMOKE measured instruments from the pinned contexts (Unit 1).

Generates, for each chosen measured sibling of the two smoke contexts:
  tasks/<slug>.json   - CONTRACT-shaped task spec with the DE-REFERENCED prompt (question
                        verbatim from the pinned dataset; only a system-naming frame + a
                        "write to solution.md" instruction are added — NEVER rule content).
  judge/<slug>.md     - the claude-CLI judge prompt embedding the VERBATIM CL-bench rubrics
                        (secondary score only; the deterministic verifier decides pass/fail).

Provenance fence: the measured question is the sibling's own user turn, copied byte-for-byte
from contexts/<name>/tasks.json (itself re-materialized from the pinned dataset sha). We add
ONLY: (a) a frame naming the invented system so the ON arm's focused retrieval can find the
extracted skill, and (b) the workspace output instruction. Nested-sibling prior_turns are
DROPPED — both aether measured siblings are self-contained once their snippet is inlined, so
we measure without prior_turns (plan §4 rule), eliminating reference-answer leakage.

corpus_skill_slug / corpus_skill_id are PENDING until Unit 4 (extraction → human gate); the
OFF pre-gate (Unit 2) needs only prompt + verifier. Re-run after Unit 4 to fill the ids.
"""
from __future__ import annotations
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
CTX = ROOT / "contexts"
TASKS_OUT = ROOT / "tasks"
JUDGE_OUT = ROOT / "judge"
TASKS_OUT.mkdir(exist_ok=True)
JUDGE_OUT.mkdir(exist_ok=True)

WORKSPACE_INSTR = (
    "\n\nWrite your complete response to a file named `solution.md` in the current working "
    "directory. Output only that file."
)

# Per-context de-reference frame (names the invented system; adds NO rule content).
FLYWHEEL_FRAME = (
    "You are Agent B, the Assembly Technician Agent in the Flywheel Manufacturing Multi-Agent "
    "System. Follow that system's operational protocols, assembly workflow, and workaround "
    "procedures exactly.\n\n"
)
AETHER_REVIEW_FRAME = (
    "You are a coding assistant for the Aether programming language. Follow the Aether language's "
    "response-format and error-reporting rules. Review the following Aether code and report any "
    "errors it contains.\n\n```aether\n{snippet}\n```"
)
AETHER_TRANSLATE_FRAME = (
    "You are a coding assistant for the Aether programming language. Translate the following "
    "Aether code into Python, following the Aether language's translation conventions.\n\n"
    "```aether\n{snippet}\n```"
)


def load(ctx: str) -> dict:
    return json.loads((CTX / ctx / "tasks.json").read_text())


def task_by_id(data: dict, short: str) -> dict:
    for t in data["tasks"]:
        if t["task_id"].startswith(short):
            return t
    raise SystemExit(f"task {short} not found")


def write_spec(slug, title, summary, rationale, prompt, verifier, sibling_id, context_name):
    spec = {
        "task_id": slug,
        "title": title,
        "_clband": {
            "context": context_name,
            "sibling_task_id": sibling_id,
            "measured_without_prior_turns": True,
            "note": "smoke = pipeline validation, NOT efficacy data; no ON/PLACEBO until Unit 4.",
        },
        "invented_rule": {
            "summary": summary,  # ALSO the focused ON inject-query (--inject-query summary), Unit 5.
            "corpus_skill_slug": "PENDING-UNTIL-UNIT-4-EXTRACTION",
            "corpus_skill_id": "PENDING-UNTIL-UNIT-4-EXTRACTION",
            "absent_from_pretraining_rationale": rationale,
        },
        "prompt": prompt,
        "workspace": {"kind": "scratch", "base_ref": None, "setup": []},
        "verifier": {
            "command": f"tests/e2e/efficacy/clband/verifiers/{verifier}",
            "contract": "Invoked as `<command> <workspace_dir>`. Exit 0 == invented rule OBEYED. "
                        "Pure deterministic inspection of solution.md; no network, no LLM.",
        },
        "expected": {
            "on": "pass", "off": "fail", "placebo": "fail",
            "sensitivity_note": "If ON fails with the rule-bearing skill attribution-confirmed "
                                "injected, that is INSTRUMENT-FAILURE(injection/obedience).",
        },
    }
    (TASKS_OUT / f"{slug}.json").write_text(json.dumps(spec, indent=2))
    return spec


def write_judge(slug, rubrics, prompt):
    lines = [
        f"# Judge prompt (SECONDARY score) — {slug}",
        "",
        "The deterministic verifier decides pass/fail. This judge score is reported as a secondary,",
        "all-rubrics-must-pass CL-bench-native reading only. Rubrics are VERBATIM from the pinned",
        "dataset. Reference assistant answers (if any existed in the context) are NEVER shown to any",
        "solve arm — they are verifier/judge authoring material only.",
        "",
        "## Task prompt given to the agent",
        "```",
        prompt,
        "```",
        "",
        "## Rubrics (verbatim — ALL must pass for a judge WIN)",
    ]
    for i, r in enumerate(rubrics, 1):
        lines.append(f"{i}. {r}")
    lines += [
        "",
        "## Instruction to the judge",
        "You are grading a single response against the rubrics above. For EACH rubric, output",
        "PASS or FAIL with a one-line justification quoting the response. Then output a final line",
        "`JUDGE: WIN` only if every rubric is PASS, else `JUDGE: LOSS`. Judge knowledge only from",
        "the rubrics; do not invent requirements.",
    ]
    (JUDGE_OUT / f"{slug}.md").write_text("\n".join(lines))


def main():
    fw = load("flywheel-assembly-agent")
    ae = load("aether-language")

    # ---- Flywheel: two measured candidates that leak NEITHER workaround rule. ----
    fly_rationale = (
        "The 'Flywheel Manufacturing Multi-Agent System' is a fictional SOP. Its two signature "
        "WORKAROUND PROTOCOLS — wrench-not-found => use the NEXT SIZE UP and apply EXTRA torque; "
        "wobble => firm SHAKE and RETEST — are deliberately unusual procedures no competent model "
        "would independently recommend, and are absent from this sibling's scenario text. OFF "
        "cannot produce them; the extracted skill carries them."
    )
    fly_summary = ("Flywheel Manufacturing Multi-Agent System: Assembly Technician (Agent B) "
                   "workflow and workaround protocols — wrench next size up + extra torque; "
                   "wobble firm shake and retest; mandatory spin test; route to Agent C then Agent D")
    for short, slug in [("979ec26a", "clband-flywheel-979ec26a"),
                        ("46536e4a", "clband-flywheel-46536e4a")]:
        t = task_by_id(fw, short)
        prompt = FLYWHEEL_FRAME + t["question"] + WORKSPACE_INSTR
        write_spec(slug, f"Flywheel assembly sibling {short} (held-out)", fly_summary,
                   fly_rationale, prompt, "flywheel-assembly.sh", t["task_id"],
                   "flywheel-assembly-agent")
        write_judge(slug, t["rubrics"], prompt)

    # ---- Aether: both held-out siblings, measured WITHOUT prior_turns. ----
    # depth-4: review the buggy CalculateAverage snippet (the snippet is THIS sibling's question).
    d4 = task_by_id(ae, "b0807c2c")
    d4_prompt = AETHER_REVIEW_FRAME.format(snippet=d4["question"]) + WORKSPACE_INSTR
    write_spec(
        "clband-aether-turbulence-b0807c2c",
        "Aether Turbulence-Alert bug review (depth-4 held-out)",
        "Aether language syntax and the Turbulence Alert error-report format (Cause/Fix/Corrected "
        "Code); assignment operator is '<<' not '='; 'outer' enables non-local writes inside swirl",
        "Aether is a fully invented language. Assignment is '<<' (not '='), so the planted "
        "'~average = ...' is a bug only someone who knows Aether would flag; and the 'Turbulence "
        "Alert' (Cause/Fix/Corrected Code) report format is invented. OFF has no basis to flag '=' "
        "or to use that format.",
        d4_prompt, "aether-turbulence-review.sh", d4["task_id"], "aether-language")
    write_judge("clband-aether-turbulence-b0807c2c", d4["rubrics"], d4_prompt)

    # depth-6: translate the CalculateAverage Aether snippet to Python. The "previous code" the
    # original turn references IS the depth-4 snippet; inline it so the task is self-contained.
    d6 = task_by_id(ae, "4768e426")
    d6_snippet = d4["question"]  # the CalculateAverage Aether code the depth-6 turn refers to
    d6_prompt = AETHER_TRANSLATE_FRAME.format(snippet=d6_snippet) + WORKSPACE_INSTR
    write_spec(
        "clband-aether-translate-4768e426",
        "Aether -> Python translation (depth-6 held-out)",
        "Aether language keyword mapping to Python: conduit->def, flow->return, '<<'->'=', "
        "fork->if, swirl->for, echo->f-string, Len->len; drop '~' sigils and '->'",
        "The Aether keyword set (conduit/flow/fork/swirl/'<<'/'~'/echo) is invented. A correct "
        "Python translation requires removing every Aether-specific token via the spec's mapping; "
        "OFF, not knowing these are Aether keywords, tends to leave them in or mistranslate.",
        d6_prompt, "aether-python-translate.sh", d6["task_id"], "aether-language")
    write_judge("clband-aether-translate-4768e426", d6["rubrics"], d6_prompt)

    print("authored:")
    for p in sorted(TASKS_OUT.glob("*.json")):
        print("  task ", p.relative_to(ROOT))
    for p in sorted(JUDGE_OUT.glob("*.md")):
        print("  judge", p.relative_to(ROOT))


if __name__ == "__main__":
    main()
