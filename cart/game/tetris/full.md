# cart/game/tetris — Turn-Based Tetris

A classic Tetris game driven entirely by LLM reasoning. The model observes the board, decides moves, and clears lines. No real-time gravity — the LLM controls every action.

## Methods

### `tetris.move({"action": "..."})`

Execute one action. Actions: `left`, `right`, `down`, `rotate`, `drop`.

- `left` / `right` — shift the falling piece horizontally (no-op if blocked)
- `down` — shift piece down one row; if blocked, locks piece and spawns next
- `rotate` — clockwise rotation (no-op if blocked, no wall kicks)
- `drop` — hard drop: piece falls to lowest valid position, locks, spawns next

Returns the full board state after the action.

### `tetris.observe({})`

Returns the current board state without changing anything.

### `tetris.reset({})`

Resets to a fresh game with an empty board and a new piece sequence.

## Board Encoding

The board is 10 columns × 20 rows, encoded as an array of 20 strings (top row first):

```json
{
  "board": [
    "..........",
    "..........",
    "....T.....",
    "...TTT....",
    "LLLL......",
    "LLLLSSZZII"
  ]
}
```

- `.` = empty cell
- `I`, `O`, `T`, `S`, `Z`, `J`, `L` = locked piece cells (letter = piece type that placed it)

The **falling piece** is NOT included in the board array — it appears in the `current` object.

## State Response

All three methods return the same JSON structure:

```json
{
  "board": ["..........","..........","...","..IIIILLLL"],
  "current": {"type": "T", "x": 4, "y": 0, "rotation": 0},
  "next": "I",
  "score": 0,
  "lines": 0,
  "level": 1,
  "game_over": false
}
```

| Field | Description |
|-------|-------------|
| `board` | 20 row strings (top to bottom), locked cells only |
| `current` | Falling piece: type, column (x), row (y), rotation (0-3) |
| `next` | Next piece type letter |
| `score` | Current score |
| `lines` | Total lines cleared |
| `level` | Current level (= lines / 10 + 1) |
| `game_over` | True if spawn was blocked |

## Scoring

| Lines Cleared | Points (× level) |
|---------------|-------------------|
| 1 (Single) | 100 |
| 2 (Double) | 300 |
| 3 (Triple) | 500 |
| 4 (Tetris) | 800 |

## Piece Types

Standard 7 tetrominoes: I, O, T, S, Z, J, L. Pieces are drawn from a 7-bag randomizer (each bag contains all 7 pieces shuffled).

## Example ISA Session

```
<|call|>tetris.reset {}<|/call|>
<|result|>{"board":["..........","..........","..........","..........","..........","..........","..........","..........","..........","..........","..........","..........","..........","..........","..........","..........","..........","..........","..........",".........."],"current":{"type":"L","x":4,"y":0,"rotation":0},"next":"J","score":0,"lines":0,"level":1,"game_over":false}<|/result|>

<|call|>tetris.move {"action":"rotate"}<|/call|>
<|result|>{"board":[...],"current":{"type":"L","x":4,"y":0,"rotation":1},...}<|/result|>

<|call|>tetris.move {"action":"drop"}<|/call|>
<|result|>{"board":[...,"....LL....","....L.....","....L....."],"current":{"type":"J","x":4,"y":0,"rotation":0},...}<|/result|>

<|call|>tetris.observe {}<|/call|>
<|result|>{"board":[...],"current":{"type":"J","x":4,"y":0,"rotation":0},...}<|/result|>
```
