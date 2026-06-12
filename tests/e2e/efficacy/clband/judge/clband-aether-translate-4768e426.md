# Judge prompt (SECONDARY score) — clband-aether-translate-4768e426

The deterministic verifier decides pass/fail. This judge score is reported as a secondary,
all-rubrics-must-pass CL-bench-native reading only. Rubrics are VERBATIM from the pinned
dataset. Reference assistant answers (if any existed in the context) are NEVER shown to any
solve arm — they are verifier/judge authoring material only.

## Task prompt given to the agent
```
You are a coding assistant for the Aether programming language. Translate the following Aether code into Python, following the Aether language's translation conventions.

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
1. The response should include an "Analysis" section that provides a technical explanation of the logic underpinning the implementation.
2. The response should include an "Implementation" section. For example, the Python code in this section should replicate the Aether code in the user prompt.
3. The response should not include any other sections beyond "Analysis" and "Implementation". For example, the response should not include any notes.
4. In the Python code, the response should translate "conduit" in the Aether code's functions as "def".
5. The response should remove all '~' in the Aether code when providing the Python code.
6. In the Python code, the response should translate "flow" in the Aether code as "return".
7. In the Python code, the response should translate "<<" in the Aether code as '='.
8. In the Python code, the response should translate "fork" in the Aether code as "if".
9. In the Python code, the response should translate "swirl (n in numbers)" in the Aether code as "for n in numbers:"
10. In the Python code, the response should replace "{}" from the Aether code with a colon after the statement immediately preceding the opening bracket. For example, "swirl (n in numbers) {total += n}" in the Aether code be converted to "swirl (n in numbers): total += n" in the Python code.
11. In the Python code, the response should remove all instances of "->" from the Aether code.
12. In the Python code, the response should translate "Len(numbers)" from the Aether code as "len(numbers)"
13. In the Python code, the response should replace "echo" from "print(echo"Avg: ${CalculateAverage(data)}")" in the Aether code with 'f'.

## Instruction to the judge
You are grading a single response against the rubrics above. For EACH rubric, output
PASS or FAIL with a one-line justification quoting the response. Then output a final line
`JUDGE: WIN` only if every rubric is PASS, else `JUDGE: LOSS`. Judge knowledge only from
the rubrics; do not invent requirements.