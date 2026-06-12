# Judge prompt (SECONDARY score) — clband-drywave-3000-manual-b7dd8397

The deterministic verifier decides pass/fail. This judge score is reported as a secondary,
all-rubrics-must-pass CL-bench-native reading only. Rubrics are VERBATIM from the pinned
dataset. Reference assistant answers (if any existed in the context) are NEVER shown to any
solve arm — they are verifier/judge authoring material only.

## Task prompt given to the agent
```
You are InfoAID, a custom assistant that helps customers and engineers find answers to complex queries about electrical appliances. You are answering questions about the DryWave 3000, using only the information in the DryWave 3000 manual you have been provided. Use a consistent structure: Summary/Answer; Steps/Procedure (if applicable); Definitions; Warnings/Safety; Assumptions.

OK so we have an acceptable strategy now, that I think will work for the customer. Can you provide me with a strategy as you did before for the same customer's annex? This has a year round humidity percentage that never exceeds 28% and critically this time they only have one dehumidifier unit for the whole property (5 rooms, spread over just one floor (kitchen, utility room, bedroom 1, bedroom 2, bathroom and lounge). Where would it be most helpful to site the one unit?

Write your complete response to a file named `solution.md` in the current working directory. Output only that file.
```

## Rubrics (verbatim — ALL must pass for a judge WIN)
1. The response should contain the following structure:  Summary/Answer, Steps/Procedure, Definitions, Warnings/Safety, Assumptions.
2. The response should state that continuous dehumidification is inappropriate for the annex as the humidity level never exceeds 28%.
3. The response should explain that maintaining humidity below 40% for extended periods in domestic living areas is not recommended, as prolonged low-moisture conditions may lead to human discomfort and certain wood materials contracting or cracking.
4. The response should state that it may be appropriate to use the dehumidifier on occasion to deal with localised increases in humidity from laundry, cooking, or showering.
5. The response should advise placing the DryWave 3000 in an area where the humidity level may rise. For example, it could suggest the kitchen, bathroom, or laundry room.
6. The response should advise using the DryWave 3000 in AUTO mode in the annex, so the device responds to humidity rises when needed.
7. The response should advise setting the target relative humidity of 45-50%.
8. The response should advise checking the 28% relative humidity reading, as it is extremely low. For example, it could suggest using a hygrometer to check its accuracy.

## Instruction to the judge
You are grading a single response against the rubrics above. For EACH rubric, output
PASS or FAIL with a one-line justification quoting the response. Then output a final line
`JUDGE: WIN` only if every rubric is PASS, else `JUDGE: LOSS`. Judge knowledge only from
the rubrics; do not invent requirements.
