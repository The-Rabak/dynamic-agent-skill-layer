#!/usr/bin/env bash
# Deterministic verifier — ezlang-language depth-4 sibling (explain program + write to file).
#
# WHY: "EZLang" is a fully invented minimal language (context.md). Its rules diverge from Python
# and JavaScript in ways a model WITHOUT the spec will get wrong:
#   - function calls use `func name with param = { ... }` (not `function`/`def`)
#   - file I/O uses `writeFile with filePath and content` (not open()/fs.writeFile)
#   - the global functions are named `split`, `toNumber`, `now`, `sleep` — all called with
#     `funcName with arg` syntax (not method calls or stdlib imports)
#   - `break` is NOT a keyword; while loops must exit via a flag variable
# An agent WITHOUT the EZLang spec defaults to Python/JS idioms and fails these checks.
# Checks compiled from CL-bench rubrics for tasks b2628a97 (depth-4) and be956d3a (depth-2).
#
# Contract: `<this> <workspace_dir>`; exit 0 == invented rules OBEYED; one-line reason to stdout;
# no network, no LLM.
set -uo pipefail

ws="${1:?usage: ezlang-language.sh <workspace_dir>}"
sol="$ws/solution.md"
if [[ -f "$sol" ]]; then
  text="$(cat "$sol")"
else
  text="$(find "$ws" -type f \( -name '*.md' -o -name '*.txt' -o -name '*.ez' \) \
           -not -path '*/.git/*' -exec cat {} + 2>/dev/null)"
fi
[[ -n "${text// /}" ]] || { echo "LOSS: no answer text found in workspace ($ws)"; exit 1; }
lc="$(printf '%s' "$text" | tr '[:upper:]' '[:lower:]')"

fail() { echo "LOSS: $1"; exit 1; }

# Check 1 — 'split' named as a global function (depth-4 rubric #1: "return the function names
# from the documentation used within global functions... timeToSeconds uses split").
echo "$lc" | grep -Eq '\bsplit\b' \
  || fail "missing 'split' — depth-4 rubric requires naming global functions used (split is used in timeToSeconds)"

# Check 2 — 'toNumber' named as a global function (depth-4 rubric #1: same rubric; timeToSeconds
# also uses toNumber to convert text parts to numbers).
echo "$lc" | grep -Eq '\btonumber\b' \
  || fail "missing 'toNumber' — depth-4 rubric requires naming global functions used (toNumber is used in timeToSeconds)"

# Check 3 — EZLang file write uses 'writeFile with' syntax (depth-4 rubric #2: "provide a
# solution that writes the program's relevant output or content to a text file in the user's
# specified language" — EZLang's writeFile global function is the only correct path).
echo "$lc" | grep -Eq 'writefile with' \
  || fail "missing 'writeFile with' — depth-4 rubric requires a file-writing solution in EZLang syntax"

# Check 4 — EZLang function definition syntax 'func' (depth-2 rubric #5: "implementations of
# EZ lang's language constructs should use the correct syntax as stated in the documentation").
# EZLang uses 'func name with param =' not Python 'def' or JS 'function'.
echo "$lc" | grep -Eq '\bfunc\b' \
  || fail "missing EZLang 'func' keyword — correct EZLang syntax uses 'func' for function definitions"

# Check 5 — EZLang variable declaration uses 'var' (depth-2 rubric #5: correct EZLang syntax;
# 'var name = value' is the only EZLang way to declare a new variable).
echo "$lc" | grep -Eq '\bvar\b' \
  || fail "missing EZLang 'var' keyword — correct EZLang syntax uses 'var' for variable declarations"

# Check 6 — No 'break' keyword (depth-2 rubric #5 + depth-6 rubric #2 + #3: "break must not be
# used with the while construct" — break is undefined in EZLang; while loops exit via flag
# variable). OFF defaults to Python/JS habit of writing break statements.
echo "$lc" | grep -Eq '\bbreak\b' \
  && fail "EZLang 'break' is undefined — depth-2 rubric: 'break must not be used with the while construct'"

echo "WIN: all 6 EZLang invented-rule checks pass (split/toNumber named, writeFile with, func/var syntax, no break)"
exit 0
