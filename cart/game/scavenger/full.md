# scavenger cartridge

Grid-world quest game. The agent (LLM-CPU) must find a target object, pick it up, and deliver it to a destination — through walls, around pits, with a finite move budget. The state of every gameplay-relevant fact is compiled into a JSON blob the LLM consumes; the LLM never sees the raw grid.

## Why this game exists

Two reasons:

1. **Kernel generalization beyond Tetris.** Tetris validated the kernel for a single game type with five enumerable actions. Scavenger validates it with a different game shape — multi-object scene, multi-step quest, spatial reasoning around obstacles. Same kernel, different cartridge, no code changes.

2. **Direct path to real-world robot test.** The compiled-state shape Scavenger emits — labeled objects with bearings, distances, relative position — is exactly what `skillos_robot`'s SemanticLoop produces from a real camera frame via SceneGraph. The LLM doesn't care that one source is a JS grid and the other is a VLM; the prompt, opcode set, and decision logic transfer 1:1.

## Methods

| Method | Args | Effect |
|---|---|---|
| `move` | `{dir: north\|south\|east\|west}` | Move one cell. Blocked by walls. Stepping on a pit ends the game (failure). Decrements move budget. |
| `pickup` | `{}` | Pick up object at current cell. Fails if cell is empty or already carrying. |
| `drop` | `{}` | Drop carried object on current cell. Fails if not carrying. |
| `look` | `{}` | Observe (no move-budget cost). Returns full compiled scene state. |

## Compiled state (what the LLM-CPU sees)

```json
{
  "pos": [3, 5],
  "carrying": null,
  "step": 12,
  "moves_left": 18,
  "objects": [
    { "label": "red_cube",     "at": [5, 2], "bearing": "NE", "dist": 4 },
    { "label": "blue_square",  "at": [2, 7], "bearing": "SW", "dist": 3 },
    { "label": "green_cube",   "at": [6, 6], "bearing": "SE", "dist": 3 },
    { "label": "wall",         "at": [3, 3], "bearing": "N",  "dist": 2 },
    { "label": "wall",         "at": [4, 3], "bearing": "NE", "dist": 3 },
    { "label": "pit",          "at": [1, 4], "bearing": "W",  "dist": 2 }
  ],
  "quest": "pickup red_cube, then drop on blue_square",
  "hint": "approach red_cube — move N twice then E twice"
}
```

The Program layer (in `demo/scavenger-browser/index.html`) computes:
- **bearings** (N/NE/E/SE/S/SW/W/NW) from agent to each object
- **Manhattan distance** to each object
- **hint** that suggests the next strategic move based on quest phase
- **moves_left** to enforce time pressure

This compilation reduces a raw 64-cell grid (~400 tokens of JSON) to ~80 tokens of decision-relevant state — same compression ratio as Tetris's column-profile pattern.

## Phase control

Same OS-layer pattern as Tetris. The kernel restricts which opcodes are legal based on game state:

| State | Allowed opcodes |
|---|---|
| First step | `look` only — agent must observe before acting |
| Active gameplay | `move`, `pickup`, `drop`, `look` |
| Quest complete | `<\|halt\|>status=success` |
| Out of moves | `<\|halt\|>status=partial` |
| Stepped on pit | `<\|halt\|>status=failure` |

Phase control is enforced at sample time via `cartridge.methodIndices(...)` and `cartridge.haltIndices()` — the model literally cannot emit a `pickup` opcode when in halt-required state.

## Halt outcomes

- `success` — red cube picked up AND delivered to blue square
- `failure` — agent stepped on pit
- `partial` — out of moves with quest incomplete

## Mapping to skillos_robot

The cartridge interface translates almost directly:

| Scavenger (JS grid) | skillos_robot (real world) |
|---|---|
| `objects: [{label, at, bearing, dist}]` from `analyzeScene()` | `objects: [...]` from `SceneGraph.toJSON()` |
| `move({dir})` → grid update | `move({dir})` → `ReactiveController.decide()` → bytecode |
| `pickup({})` → inventory update | `pickup({})` → `robot_arm.grasp()` (future cartridge) |
| `look({})` → grid snapshot | `look({})` → fresh VLM call or cached SceneGraph |
| Pit → game over | Reflex guard veto + safety layer halt |

The same prompt, the same opcode set, the same compiled-state shape. The kernel and the LLM are entirely unaware of the swap.
