#!/usr/bin/env bash
# Deterministic verifier — aether-language depth-6 sibling (Aether -> Python translation).
#
# WHY: Translating the invented Aether snippet to Python requires the invented keyword mapping from
# the Aether spec: conduit->def, flow->return, "<<"->"=", fork->if, swirl->for, echo->f, Len->len,
# and dropping "~" sigils and "->". The semantic keywords (def/return/if/for) are partly guessable,
# but the Aether-specific tokens ("<<", "~", "echo", "conduit", "swirl", "fork", "flow") must be
# REMOVED from the output — an agent that doesn't know they are Aether keywords tends to leave them
# in or mistranslate. The verifier requires a real Python translation with every Aether token gone.
# Checks compiled from the CL-bench rubrics for task 4768e426 (deterministic mapping rubrics).
#
# Contract: `<this> <workspace_dir>`; exit 0 == rules obeyed; one-line reason to stdout; no net/LLM.
set -uo pipefail

ws="${1:?usage: aether-python-translate.sh <workspace_dir>}"
sol="$ws/solution.md"
if [[ -f "$sol" ]]; then text="$(cat "$sol")"; else
  text="$(find "$ws" -type f \( -name '*.md' -o -name '*.txt' -o -name '*.py' \) -not -path '*/.git/*' -exec cat {} + 2>/dev/null)"
fi
[[ -n "${text// /}" ]] || { echo "LOSS: no answer text found in workspace ($ws)"; exit 1; }
fail() { echo "LOSS: $1"; exit 1; }

# Positive Python tokens that MUST appear (the translation actually happened).
grep -Eq '\bdef\b'    <<<"$text" || fail "no Python 'def' — conduit->def not performed"
grep -Eq '\breturn\b' <<<"$text" || fail "no Python 'return' — flow->return not performed"
grep -Eq '\bfor\b'    <<<"$text" || fail "no Python 'for' — swirl->for not performed"
grep -Eq '\bif\b'     <<<"$text" || fail "no Python 'if' — fork->if not performed"
grep -Eq 'len\('      <<<"$text" || fail "no lowercase 'len(' — Len->len not performed"

# Aether tokens that MUST be GONE from the translated output.
grep -Eq '\bconduit\b' <<<"$text" && fail "Aether 'conduit' still present — not translated to def"
grep -Eq '\bflow\b'    <<<"$text" && fail "Aether 'flow' still present — not translated to return"
grep -Eq '\bswirl\b'   <<<"$text" && fail "Aether 'swirl' still present — not translated to for"
grep -Eq '\bfork\b'    <<<"$text" && fail "Aether 'fork' still present — not translated to if"
grep -q  '<<'          <<<"$text" && fail "Aether '<<' assignment still present — not translated to '='"
grep -q  '~'           <<<"$text" && fail "Aether '~' sigils still present — must be removed in Python"
grep -Eq '\becho\b'    <<<"$text" && fail "Aether 'echo' still present — echo-string not translated to f-string"

echo "WIN: real Python (def/return/for/if/len) with every Aether token removed (conduit/flow/swirl/fork/<</~/echo)"
exit 0
