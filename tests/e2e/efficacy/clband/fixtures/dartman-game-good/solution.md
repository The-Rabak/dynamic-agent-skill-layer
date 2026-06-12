# Dartman Game — Winning Move and Blocking Analysis

## Starting Board

|   | a  | b  | c  | d  | e  | f  | g  | h  |
|---|----|----|----|----|----|----|----|----|
| 8 | Y2 | Y1 | .  | .  | .  | .  | .  | .  |
| 7 | Y2 | .  | .  | .  | .  | P2 | .  | .  |
| 6 | .  | .  | .  | .  | .  | .  | .  | .  |
| 5 | .  | .  | .  | .  | .  | .  | .  | .  |
| 4 | .  | .  | .  | .  | .  | .  | .  | .  |
| 3 | .  | .  | .  | .  | .  | .  | .  | .  |
| 2 | P1 | .  | .  | .  | X1 | .  | .  | X2 |
| 1 | .  | .  | .  | .  | .  | .  | .  | X2 |

Piece positions:
- P1: (a,2) · X1: (e,2) · X2: (h,2) and (h,1)
- P2: (f,7) · Y1: (b,8) · Y2: (a,8) and (a,7)

---

## Player 2's Winning Move

**P2 moves from (f,7) to (b,7).**

P2's Dartman has momentum and slides left along row 7. It passes (e,7), (d,7), (c,7) and reaches (b,7), where it is stopped by its own 1x2 Deflector Y2 occupying (a,7). This places P2 at (b,7), completing the goal layout for Player 2: P2 at (b,7), Y1 at (b,8), and Y2 at (a,8) and (a,7). Player 2 wins.

### Board after winning move:

|   | a  | b  | c  | d  | e  | f  | g  | h  |
|---|----|----|----|----|----|----|----|----|
| 8 | Y2 | Y1 | .  | .  | .  | .  | .  | .  |
| 7 | Y2 | P2 | .  | .  | .  | .  | .  | .  |
| 6 | .  | .  | .  | .  | .  | .  | .  | .  |
| 5 | .  | .  | .  | .  | .  | .  | .  | .  |
| 4 | .  | .  | .  | .  | .  | .  | .  | .  |
| 3 | .  | .  | .  | .  | .  | .  | .  | .  |
| 2 | P1 | .  | .  | .  | X1 | .  | .  | X2 |
| 1 | .  | .  | .  | .  | .  | .  | .  | X2 |

### Pros and cons of P2's winning move

- **Pros:** Achieves the goal layout in one move, with all three P2 pieces simultaneously at target positions; the Dartman's momentum rule ensures the piece stops exactly at (b,7) next to Y2 without overshoot.
- **Cons:** Requires Y2 to remain at (a,7); if P1 disrupts the path before this move, the win opportunity is lost.

---

## Two Blocking Moves for Player 1

### Blocking Move A — X1 from (e,2) to (e,7)

Player 1 slides its 1x1 Deflector X1 straight up file e from (e,2) to (e,7). This places an opposing piece directly in the square immediately to the left of P2 at (f,7). Since P2's Dartman cannot use an opposing deflector as a stopping point, P2 has no legal stopping square when moving left along row 7. The winning move is blocked.

**Board after Blocking Move A:**

|   | a  | b  | c  | d  | e  | f  | g  | h  |
|---|----|----|----|----|----|----|----|----|
| 8 | Y2 | Y1 | .  | .  | .  | .  | .  | .  |
| 7 | Y2 | .  | .  | .  | X1 | P2 | .  | .  |
| 6 | .  | .  | .  | .  | .  | .  | .  | .  |
| 5 | .  | .  | .  | .  | .  | .  | .  | .  |
| 4 | .  | .  | .  | .  | .  | .  | .  | .  |
| 3 | .  | .  | .  | .  | .  | .  | .  | .  |
| 2 | P1 | .  | .  | .  | .  | .  | .  | X2 |
| 1 | .  | .  | .  | .  | .  | .  | .  | X2 |

- **Pros:** Immediately prevents P2's winning move; X1 occupies a strong central position on row 7 that also threatens to interfere with P2's future deflector maneuvers; a single rook-like move achieves the block.
- **Cons:** X1 is removed from row 2, weakening P1's own path toward its goal positions; X1 is now adjacent to P2 and could be passed over by P2's deflectors on future turns.

### Blocking Move B — P1 Dartman from (a,2) to (d,2)

Player 1 moves its Dartman P1 rightward along row 2 from (a,2). P1 slides until stopped by its own deflector X1 at (e,2), halting at (d,2). P1 is now at (d,2). This does not block P2's winning move on the current turn, but advances P1's Dartman toward its own goal position at (g,2) and repositions it away from file a, where Y2 at (a,7) and (a,8) create a potential vertical line-of-fire risk.

**Board after Blocking Move B:**

|   | a  | b  | c  | d  | e  | f  | g  | h  |
|---|----|----|----|----|----|----|----|----|
| 8 | Y2 | Y1 | .  | .  | .  | .  | .  | .  |
| 7 | Y2 | .  | .  | .  | .  | P2 | .  | .  |
| 6 | .  | .  | .  | .  | .  | .  | .  | .  |
| 5 | .  | .  | .  | .  | .  | .  | .  | .  |
| 4 | .  | .  | .  | .  | .  | .  | .  | .  |
| 3 | .  | .  | .  | .  | .  | .  | .  | .  |
| 2 | .  | .  | .  | P1 | X1 | .  | .  | X2 |
| 1 | .  | .  | .  | .  | .  | .  | .  | X2 |

- **Pros:** Advances P1's Dartman toward its goal at (g,2); repositions P1 away from file a, reducing line-of-fire exposure; preserves X1 on row 2 for P1's own goal setup.
- **Cons:** Does NOT prevent P2's winning move — if P1 chooses this move, P2 can win immediately on the next turn by moving P2 from (f,7) to (b,7); this option only makes sense if P2 is somehow unable to make the winning move.

---

## Summary

| Move | Description | Outcome |
|------|-------------|---------|
| **P2 winning move** | P2 from (f,7) to (b,7) | P2 wins — goal layout complete |
| **Blocking Move A** | X1 from (e,2) to (e,7) | Blocks the winning line; P2 cannot stop left |
| **Blocking Move B** | P1 from (a,2) to (d,2) | Does not block win; advances P1 toward goal |
