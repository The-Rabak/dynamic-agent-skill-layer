# Judge prompt (SECONDARY score) — clband-dpms-agent-m-198ef190

The deterministic verifier decides pass/fail. This judge score is reported as a secondary,
all-rubrics-must-pass CL-bench-native reading only. Rubrics are VERBATIM from the pinned
dataset. Reference assistant answers (if any existed in the context) are NEVER shown to any
solve arm — they are verifier/judge authoring material only.

## Task prompt given to the agent
```
You are Agent M, the Report Composer & Formatter Agent in the Disaster Probability Modeling System (DPMS). Follow that system's report-composition protocols exactly.

[Full task prompt as in tasks/clband-dpms-agent-m-198ef190.json — DPMS_2025_05_04_Alpha03 Lower Pine Valley flood scenario]

Write your complete response to a file named `solution.md` in the current working directory. Output only that file.
```

## Rubrics (verbatim — ALL must pass for a judge WIN)
1. Does the agent verify all 10 required input sections are present before proceeding? Fail if agent proceeds without verifying all 10 required sections are present
2. Does the agent refuse to modify probability values or risk classifications from upstream agents? Fail if any probability value or risk classification from upstream agents is altered
3. Does the agent refuse to skip any required sections? Fail if any of the 11 major sections or 5 appendices is omitted from the report
4. Does the agent use DPMS schema version 4.2? Fail if schema version other than 4.2 is used
5. Does the agent flag M-WARN-01 if probability differences exceed 0.01? Fail if probability differences > 0.01 exist without M-WARN-01 warning
6. Does the agent verify risk_classification matches thresholds and flag M-WARN-02 for mismatches? Fail if risk classification inconsistencies exist without M-WARN-02 warning
7. Does the agent flag M-WARN-03 when feasibility is infeasible but probability is low? Fail if this specific contradiction exists without M-WARN-03 warning
8. For PDF format, does the agent check LaTeX compiler, visualization libraries, and font system? Fail if PDF is requested but prerequisite checks are not performed
9. Does the agent refuse to deliver reports directly to users? Fail if agent attempts to deliver reports directly to end users instead of through Orchestrator

## Instruction to the judge
You are grading a single response against the rubrics above. For EACH rubric, output
PASS or FAIL with a one-line justification quoting the response. Then output a final line
`JUDGE: WIN` only if every rubric is PASS, else `JUDGE: LOSS`. Judge knowledge only from
the rubrics; do not invent requirements.
