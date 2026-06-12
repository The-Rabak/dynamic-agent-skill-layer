# Judge prompt (SECONDARY score) — clband-quartermaster-hold-inventory-2db1bd9f

The deterministic verifier decides pass/fail. This judge score is reported as a secondary,
all-rubrics-must-pass CL-bench-native reading only. Rubrics are VERBATIM from the pinned
dataset. Reference assistant answers (if any existed in the context) are NEVER shown to any
solve arm — they are verifier/judge authoring material only.

## Task prompt given to the agent
```
You are the Hold Inventory Agent ("The Quartermaster") for a fictional climbing gym's wall-setting system. Follow that system's hold-inventory assessment protocols exactly.

[Turn 1 - 2025-12-02T08:45:01] Message from: Orchestrator
Kicking off this week's reset thread for Tuesday. Please align on the weekly plan using the latest wall status materials.

[Turn 2 - 2025-12-02T08:46:12] Message from: Setting Manager
We are live for this Tuesday's cycle. Use the Wall Status report as the basis for the plan. If anything looks off, call it out immediately. See Artifact 1 for the current report.

[... full question text as in task spec ...]

FINAL TASK
Using the transcript and artifacts above, generate this week's Wall Setting Plan for Tuesday. Validate the integrity of the Wall Status materials before selecting. If the inputs contain contradictions or anomalies that prevent a definitive selection, issue a clear PLAN_INCONSISTENCY_ALERT and halt. If and only if the inputs are valid, output a compact, deterministic instruction set containing the two selected wall IDs and the count of Main walls included.

Write your complete response to a file named `solution.md` in the current working directory. Output only that file.
```

## Rubrics (verbatim — ALL must pass for a judge WIN)
1. Does the agent restrict its evaluation strictly to whether the plan is physically possible? Fail if the agent considers desirability, convenience or any factor other than physical possibility when assessing a plan.
2. Does the agent explicitly reject optimistic planning? Fail if the agent does not reject optimistic planning
3. Does the agent stop the workflow immediately if the plan demands more than is available? Fail if the agent does not stop the workflow when the plan demands more than is available.
4. Does the agent use exactly 500 holds for the Main Wall calculation? Fail if the agent uses any value other than 500 holds for a Main Wall.
5. Does the agent use exactly 100 holds for the Wall 5 calculation? Fail if the agent uses any value other than 100 for Wall 5.
6. Does the agent assign 'HOLD_LOW' when inventory is between 100% and 149% of requirement? Fail if the agent fails to assign 'HOLD_LOW' when inventory is in the 100-149% range.
7. Does the agent assign 'HOLD_CRITICAL_LOW' immediately when inventory is below requirement? Fail if the agent fails to assign 'HOLD_CRITICAL_LOW' when inventory is < 100%.
8. Does the agent treat 'HOLD_CRITICAL_LOW' as a hard stop condition? Fail if the agent does not treat 'HOLD_CRITICAL_LOW' as a hard stop condition
9. Does the agent refuse to authorize any plan when inventory is < 100%? Fail if the agent authorizes a plan when inventory is below 100 percent of requirement.
10. Does the agent halt the process if counts are ambiguous? Fail if the agent proceeds despite ambiguous counts.
11. Does the agent halt the process if wall identifiers are mismatched? Fail if the agent does not halt the process if wall identifiers are mismatched
12. Does the agent produce an "Inventory Status report" for the Setting Manager? Fail if the agent does not produce an "Inventory Status report" for the Setting Manager
13. Does the report include the specific assigned status code? Fail if the report does not include the assigned status code.
14. Does the report include a numerical breakdown showing exactly how many holds are required versus how many are available? Fail if the report does not include a numerical breakdown showing exactly how many holds are required versus how many are available.
15. Does the agent correctly classify exactly 150% as 'HOLD_OK'? Fail if the agent does not classify exactly 150% available as HOLD_OK.
16. Does the agent correctly classify exactly 100% as 'HOLD_LOW'? Fail if the agent does not classify exactly 100% available as HOLD_LOW.
17. Does the agent recognize the plan always includes Wall 5 plus one Main Wall? Fail if the agent assumes a different plan composition unless specified.
18. Is the *foundation* of the response built on verifiable facts and sound, logical reasoning, with all domain knowledge applied correctly? Fail if the core of the response is built on a "hallucination" a fundamental logical fallacy, or a critical error in domain knowledge.
19. When faced with conflicting, ambiguous, or impossible constraints, did the model *identify* the conflict, *prioritize* correctly (e.g., Safety > Style), and *propose* a viable solution or path forward? Fail if the model ignores a core conflict, compromises a primary constraint, or halts without offering a solution.
20. Is the final output (whether text, plan, or interface) presented in a clear, unambiguous, and effective manner that the user can *actually use* to achieve their goal? Fail if the response is technically correct but is a jumbled, disorganized, or poorly-formatted mess that the user cannot reasonably act upon.

## Instruction to the judge
You are grading a single response against the rubrics above. For EACH rubric, output
PASS or FAIL with a one-line justification quoting the response. Then output a final line
`JUDGE: WIN` only if every rubric is PASS, else `JUDGE: LOSS`. Judge knowledge only from
the rubrics; do not invent requirements.
