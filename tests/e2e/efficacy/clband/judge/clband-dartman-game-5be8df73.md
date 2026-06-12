# Judge prompt (SECONDARY score) — clband-dartman-game-5be8df73

The deterministic verifier decides pass/fail. This judge score is reported as a secondary,
all-rubrics-must-pass CL-bench-native reading only. Rubrics are VERBATIM from the pinned
dataset. Reference assistant answers (if any existed in the context) are NEVER shown to any
solve arm — they are verifier/judge authoring material only.

## Task prompt given to the agent
```
You are an expert assistant for the game of Dartman. Apply the rules of the Dartman game exactly.

Ignore previous moves, let's start a new game of Dartman. In this board, Player 2 is one move away from winning. What is the winning move, and what are the two moves that Player 1 could make to block the winning move? What are the pros and cons of each of the two options? Show diagrams for the starting point, the winning move, and the two moves that could block it.
P1: (a,2)
X1: (e,2)
X2: (h,2) and (h,1)
P2: (f,7)
Y1: (b,8)
Y2: (a,8) and (a,7)

Write your complete response to a file named `solution.md` in the current working directory. Output only that file.
```

## Rubrics (verbatim — ALL must pass for a judge WIN)
1. The response should start the new game of Dartman with a new board according to the layout given by the user. 
P1: (a,2)
X1: (e,2)
X2: (h,2) and (h,1)
P2: (f,7)
Y1: (b,8)
Y2: (a,8) and (a,7)
2. The response should state a winning move for Player 2.
3. The response should identify two blocking moves for Player 1
4. The response should list the pros and cons list of the two blocking moves for Player 1
5. The response should include diagrams showcasing the winning move and the two blocking moves in Dartman game board format.
6. The response should include the pros and cons list of the winning move for Player 2.
7. The response should present the winning move by specifying the moving marker's label and both origin and destination coordinates. For example, "Marker [label] moves from (file,rank) to (file,rank)."
8. The response should provide at least one pro and one con for each blocking move, presented separately per move in a concise bullet list. For example, "Blocking move A — Pros: …; Cons: …"
9. The response should present exactly two blocking moves for Player 1, each clearly tied to blocking the specific stated winning move for Player 2. For example, it should state how each move prevents that win.

## Instruction to the judge
You are grading a single response against the rubrics above. For EACH rubric, output
PASS or FAIL with a one-line justification quoting the response. Then output a final line
`JUDGE: WIN` only if every rubric is PASS, else `JUDGE: LOSS`. Judge knowledge only from
the rubrics; do not invent requirements.