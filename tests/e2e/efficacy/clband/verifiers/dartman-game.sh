#!/usr/bin/env bash
# Deterministic verifier for the dartman-game measured sibling (T23 clband Unit A).
#
# WHY: "Dartman" is a fully invented two-player board game. Its pieces — Dartman (P1/P2),
# 1x1 Deflector (X1/Y1), and 1x2 Deflector (X2/Y2) — use naming conventions and movement
# rules that have no equivalent in chess or any real game. The depth-8 task provides a custom
# board and asks for a strategic analysis (winning move + two blocking moves + pros/cons +
# diagrams). The OFF arm (bare agent, no Dartman knowledge) defaults to chess terminology and
# is unable to correctly apply the invented piece notation, goal layout, coordinate system, or
# momentum-based Dartman movement rules. These checks are compiled VERBATIM from the CL-bench
# rubrics in tasks.json (depth-8, task_id 5be8df73). Knowledge rubrics only — persona/tone
# rubrics are dropped from the deterministic core (handled by the judge prompt).
#
# Contract: invoked as `<this> <workspace_dir>`.
# Exit 0 == invented rules OBEYED (task win). Non-zero == loss. One-line reason to stdout.
# Pure deterministic inspection of solution.md; no network, no LLM.
set -uo pipefail

ws="${1:?usage: dartman-game.sh <workspace_dir>}"

sol="$ws/solution.md"
if [[ -f "$sol" ]]; then
  text="$(cat "$sol")"
else
  text="$(find "$ws" -type f \( -name '*.md' -o -name '*.txt' \) -not -path '*/.git/*' -exec cat {} + 2>/dev/null)"
fi
if [[ -z "${text// /}" ]]; then
  echo "LOSS: no answer text found in workspace ($ws) — agent produced no solution.md/.txt"
  exit 1
fi
lc="$(printf '%s' "$text" | tr '[:upper:]' '[:lower:]')"

fail() { echo "LOSS: $1"; exit 1; }

# Check 1 — INITIAL BOARD uses Dartman piece labels and the task-provided layout.
# Rubric 1: "start the new game of Dartman with a new board according to the layout given by the user:
#   P1:(a,2), X1:(e,2), X2:(h,2)+(h,1), P2:(f,7), Y1:(b,8), Y2:(a,8)+(a,7)."
# Checks that the response references the Dartman piece notation (P1/X1/X2/P2/Y1/Y2) and key
# layout coordinates. An OFF/chess-prior answer uses king/queen/rook etc. and cannot produce
# these invented labels and exact starting coordinates.
echo "$lc" | grep -iEq 'p2|y1|y2' \
  || fail "missing Dartman piece labels (P2/Y1/Y2) — response likely uses chess notation instead"
echo "$lc" | grep -iEq '\(f,7\)|\(a,8\)|\(a,7\)|\(b,8\)|\(a,2\)|\(e,2\)' \
  || fail "missing task-provided board coordinates — board layout not reproduced in Dartman notation"

# Check 2 — WINNING MOVE for Player 2 identified.
# Rubric 2: "The response should state a winning move for Player 2."
# Rubric 7: "present the winning move by specifying the moving marker's label and both origin
#   and destination coordinates. For example, 'Marker [label] moves from (file,rank) to (file,rank).'"
# An ON answer names P2 with a coordinate pair; an OFF answer names a chess piece.
echo "$lc" | grep -iEq 'p2.{0,60}(win|winning|victory|winning move)|(win|winning|victory).{0,60}p2' \
  || fail "winning move not attributed to P2 — response does not identify Player 2's winning move"
echo "$lc" | grep -iEq 'from \([a-h],[1-8]\) to \([a-h],[1-8]\)|p2.{0,40}from.{0,20}to' \
  || fail "winning move not presented with origin-and-destination coordinate format (invented Dartman notation)"

# Check 3 — EXACTLY TWO BLOCKING MOVES for Player 1 identified.
# Rubric 3: "identify two blocking moves for Player 1."
# Rubric 9: "present exactly two blocking moves for Player 1, each clearly tied to blocking
#   the specific stated winning move for Player 2."
# An OFF answer may describe chess moves or fail to enumerate exactly two P1 responses.
echo "$lc" | grep -iEq '\bp1\b.{0,120}block|block.{0,120}\bp1\b' \
  || fail "no blocking moves attributed to Player 1 (P1) — invented blocking-move analysis absent"
# Two distinct blocking moves must be enumerated (labeled A/B or 1/2 or "first"/"second").
blocking_count=$(echo "$lc" | grep -ioE 'blocking move [ab12]|block(ing)? option [ab12]' | sort -u | wc -l)
[[ "$blocking_count" -ge 2 ]] \
  || fail "two distinct labeled blocking moves not found — response does not enumerate exactly two P1 options"

# Check 4 — PROS AND CONS present for both blocking moves.
# Rubric 4: "list the pros and cons list of the two blocking moves for Player 1."
# Rubric 8: "provide at least one pro and one con for each blocking move, presented separately
#   per move in a concise bullet list. For example, 'Blocking move A — Pros: …; Cons: …'"
# A chess-default answer that does not know the invented rules cannot frame pros/cons in
# Dartman's strategic context; an ON answer applies the momentum/line-of-fire rules.
echo "$lc" | grep -iEq '\bpro(s)?\b' \
  || fail "missing pros/cons — 'pro' keyword absent (rubric requires per-blocking-move pros and cons)"
echo "$lc" | grep -iEq '\bcon(s)?\b' \
  || fail "missing pros/cons — 'con' keyword absent (rubric requires per-blocking-move pros and cons)"

# Check 5 — BOARD DIAGRAMS in Dartman grid format.
# Rubric 5: "include diagrams showcasing the winning move and the two blocking moves in
#   Dartman game board format." The format is a table with piece labels (P1/P2/X1/X2/Y1/Y2)
#   and periods for empty squares — NOT standard chess diagrams with piece symbols.
# An OFF/chess answer shows chess symbols or no board at all.
echo "$lc" | grep -iEq '\|.*\|.*\|.*\|' \
  || fail "missing board diagrams — no table/grid format found (Dartman board required for winning + blocking moves)"
echo "$lc" | grep -iEq '\|\s*\.\s*\||\|\s*p[12]\s*\||\|\s*[xy][12]\s*\|' \
  || fail "board diagrams do not use Dartman piece notation (P1/P2/X1/X2/Y1/Y2 with period empty squares)"

# Check 6 — PROS AND CONS for the winning move itself (Player 2's perspective).
# Rubric 6: "include the pros and cons list of the winning move for Player 2."
# This is a separate requirement from the blocking-move pros/cons.
# An OFF answer may cover blocking moves but omit P2 pros/cons, or have no pros/cons at all.
echo "$lc" | grep -iEq 'pro(s)?.{0,60}p2|p2.{0,60}pro(s)?|pro(s)?.{0,60}win(ning)?|win(ning)?.{0,60}pro(s)?' \
  || fail "missing pros/cons for the winning move — response does not analyze Player 2's winning move pros/cons"

echo "WIN: all 6 invented Dartman rules obeyed (labels+coords, P2-winning-move, two-P1-blocks, pros/cons, board-diagrams, P2-pros-cons)"
exit 0
