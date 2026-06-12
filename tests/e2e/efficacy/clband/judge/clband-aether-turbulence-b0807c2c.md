# Judge prompt (SECONDARY score) — clband-aether-turbulence-b0807c2c

The deterministic verifier decides pass/fail. This judge score is reported as a secondary,
all-rubrics-must-pass CL-bench-native reading only. Rubrics are VERBATIM from the pinned
dataset. Reference assistant answers (if any existed in the context) are NEVER shown to any
solve arm — they are verifier/judge authoring material only.

## Task prompt given to the agent
```
You are a coding assistant for the Aether programming language. Follow the Aether language's response-format and error-reporting rules. Review the following Aether code and report any errors it contains.

```aether
conduit CalculateAverage(~numbers) {
    ~count << Len(~numbers)
    fork (~count == 0) -> { flow 0 }
    
    ~total << 0
    
    swirl (~n in ~numbers) {
        outer ~total 
        ~total += ~n
    }

    ~average = ~total / ~count
    
    flow ~average
}

conduit Main() {
    ~data << [10, 20, 30]
    print(echo"Avg: ${CalculateAverage(~data)}")
}
```

Write your complete response to a file named `solution.md` in the current working directory. Output only that file.
```

## Rubrics (verbatim — ALL must pass for a judge WIN)
1. The response should include a "Turbulence Alert" when the provided Aether code contains an error without the user explicitly mentioning it. For example, it should mention the incorrect use of '='.
2. The response should include a "Cause" section within the "Turbulence Alert". For example, it should mention the incorrect use of '='.
3. The response should include a "Fix" section within the "Turbulence Alert". For example, it should describe how to fix the incorrect use of '=' by replacing it with '<<'.
4. The response should include a "Corrected Code" section within the "Turbulence Alert". For example, it should replace the line "~average = ~total / ~count" from the user's Aether code with "~average << ~total / ~count".
5. The response should not include any sections within the "Turbulence Alert" other than "Cause", "Fix" and "Corrected Code". For example, there should not be any additional comments or notes.
6. The response should not remove the use of "outer" within the line "outer ~total". This is because "outer" allows the use of a non-local variable and since "~total" is not local to the "swirl" function.
7. The response should not state that a fork statement requires a path given that in its grammar specification, "fork_stmt ::= "fork" "(" expression ")" "->" block ("path" ["(" expression ")"] "->" block)*", the '*' means "0 or more". Thus, the specification of a path is optional.
8. The response should render Aether-specific terminology in boldface. For example, fork, outer, and path should be in bold text.

## Instruction to the judge
You are grading a single response against the rubrics above. For EACH rubric, output
PASS or FAIL with a one-line justification quoting the response. Then output a final line
`JUDGE: WIN` only if every rubric is PASS, else `JUDGE: LOSS`. Judge knowledge only from
the rubrics; do not invent requirements.