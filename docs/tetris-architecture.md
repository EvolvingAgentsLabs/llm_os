# Tetris Architecture: OS vs Program Layer

The browser Tetris demo (`demo/tetris-browser/`) implements the two-layer architecture that defines LLM-OS: a clean separation between the **OS** (which guarantees valid execution) and the **Program** (which guides intelligent decisions).

## The Core Insight

An LLM used as a CPU has a fundamental limitation: it cannot parse complex raw state efficiently at small parameter counts. A 350M model receiving a 400-token raw board JSON will make random moves. The same model receiving a 70-token compiled state with strategic hints plays competent Tetris.

This is the same relationship between an operating system and a program in traditional computing — but inverted:

| Traditional OS | LLM-OS |
|---|---|
| OS manages hardware, program does logic | OS manages inference, **Program compiles input** |
| Program calls syscalls for I/O | Program compiles state for the LLM-CPU |
| OS enforces memory safety | OS enforces grammar (output safety) |
| Program decides what to compute | Program decides **what to show** the CPU |

The Program Layer is primarily an **input-side** mechanism. It doesn't constrain outputs (that's the OS's job via grammar) — it shapes the information the LLM-CPU receives to maximize decision quality.

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│  PROGRAM LAYER (domain-specific intelligence)                   │
│                                                                 │
│  analyzeBoard(state)  → metrics: heights, holes, roughness     │
│  generateHint(state)  → strategy: "fill left well", "clear"    │
│  compileResult(state) → 60-80 token compiled state for LLM     │
│                                                                 │
│  Input:  raw game state (400+ tokens of JSON)                   │
│  Output: compiled state (60-80 tokens, decision-relevant)       │
└────────────────────────────────┬────────────────────────────────┘
                                 │ compiled state injected as <|result|>
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│  OS LAYER (domain-agnostic infrastructure)                      │
│                                                                 │
│  Token-Trie Grammar    → only valid opcodes can be emitted      │
│  KV Cache Manager      → incremental decode + sliding window    │
│  Phase Controller      → allowed opcodes based on game state    │
│  Dispatch Loop         → parse opcode, execute, inject result   │
│                                                                 │
│  Input:  compiled state + model weights                         │
│  Output: grammar-valid ISA opcode (guaranteed)                  │
└────────────────────────────────┬────────────────────────────────┘
                                 │ opcode executed
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│  GAME ENGINE (environment / cartridge)                           │
│                                                                 │
│  Executes move → updates board → returns raw state              │
│  Tetris rules: collision, rotation, line clearing, gravity      │
└─────────────────────────────────────────────────────────────────┘
```

## The Program Layer in Detail

### `analyzeBoard(state)` — Metrics Extraction

Computes decision-relevant metrics from raw board state:

```javascript
function analyzeBoard(state) {
  const board = state.board;
  // Column heights (pile profile)
  const colHeights = Array(10).fill(0);
  for (let c = 0; c < 10; c++) {
    for (let r = 0; r < 20; r++) {
      if (board[r][c] !== '.') { colHeights[c] = 20 - r; break; }
    }
  }
  // Aggregate metrics
  const pileHeight = Math.max(...colHeights);
  const holes = countBuriedHoles(board, colHeights);
  const roughness = sumAdjacentHeightDiffs(colHeights);
  const fullRows = board.filter(r => !r.includes('.')).length;
  const wells = detectWells(colHeights);       // columns 2+ below neighbors
  const maxFlat = longestFlatSegment(colHeights);

  return { pileHeight, colHeights, holes, roughness, fullRows, maxFlat, wells };
}
```

**Why this matters for small models:** A 350M model cannot visually parse a 10x20 grid of characters. But it can reason about "pile: 8, holes: 3, rough: 12" — these are the kinds of structured inputs that instruction-tuned models handle well even at minimal parameter counts.

### `generateHint(state, analysis)` — Strategic Guidance

Produces a natural-language hint based on the current situation:

```javascript
function generateHint(state, analysis) {
  if (analysis.fullRows > 0) return "clear lines available";
  if (analysis.pileHeight > 14) return "danger: pile high, drop flat";
  if (analysis.holes > 4) return "reduce holes, avoid overhangs";
  if (analysis.wells.length > 0) return `fill well at col ${analysis.wells[0].col}`;
  if (analysis.roughness < 4) return "surface flat, build evenly";
  return "reduce roughness, keep flat";
}
```

**Design principle:** Hints are suggestions, not commands. The LLM-CPU still makes the final decision via its generative reasoning — but it reasons over pre-digested context rather than raw pixel-equivalent data.

### `compileResult(state, stepCount)` — The Bridge Function

The core interface between Program and OS:

```javascript
function compileResult(state, stepCount) {
  const a = analyzeBoard(state);
  const cur = state.current;
  // Column height profile as single-digit string (0-9 per column)
  const profile = a.colHeights.map(h => Math.min(h, 9)).join('');

  const compiled = {
    piece: cur.type,           // current piece letter
    x: cur.x,                  // horizontal position
    rot: cur.rotation,         // rotation state (0-3)
    next: state.next,          // next piece preview
    pile: a.pileHeight,        // max column height
    holes: a.holes,            // buried empty cells
    rough: a.roughness,        // sum of adjacent height differences
    profile: profile,          // "0012334210" — full height map in 10 chars
    score: state.score,
    lines: state.lines,
    step: stepCount,
    hint: generateHint(state, a),
  };

  // Conditional fields for critical situations
  if (a.pileHeight > 14) compiled.urgent = true;
  if (a.wells.length > 0) compiled.well = a.wells[0].col;

  return JSON.stringify(compiled);
}
```

**Token budget:** Raw `JSON.stringify(state)` produces ~400 tokens. `compileResult()` produces ~60-80 tokens — a 5-6x reduction that fits within the 100-token attention sweet spot for sub-1B models.

## The OS Layer in Detail

### Token-Trie Grammar Enforcement

The OS builds a trie of all valid ISA opcodes at model load time:

```javascript
const OPCODE_STRINGS = [
  '<|call|>tetris.move {"action":"left"}<|/call|>',
  '<|call|>tetris.move {"action":"right"}<|/call|>',
  '<|call|>tetris.move {"action":"down"}<|/call|>',
  '<|call|>tetris.move {"action":"rotate"}<|/call|>',
  '<|call|>tetris.move {"action":"drop"}<|/call|>',
  '<|call|>tetris.observe {}<|/call|>',
  '<|call|>tetris.reset {}<|/call|>',
  '<|halt|>status=success<|/halt|>',
  '<|halt|>status=done<|/halt|>',
  '<|halt|>lines_cleared<|/halt|>',
];
```

Each opcode is pre-tokenized into a sequence of token IDs. The trie enables efficient lookup: at each generation step, only token IDs that lead to a valid opcode continuation are allowed.

```javascript
class TokenTrie {
  getValidNextTokens(generatedTokens, allowedSet) {
    // Walk trie to current position
    let node = this.root;
    for (const tok of generatedTokens) {
      if (!node.children.has(tok)) return new Set();
      node = node.children.get(tok);
    }
    // Filter by allowed opcode indices
    if (!allowedSet) return new Set(node.children.keys());
    const result = new Set();
    for (const [tokId, child] of node.children) {
      if (this._leadsToAllowed(child, allowedSet)) result.add(tokId);
    }
    return result;
  }
}
```

**Key property:** The model physically cannot emit an invalid opcode. This is not filtering or rejection — invalid tokens are never sampled. This is the LLM-OS equivalent of hardware memory protection.

### Phase Controller

The OS restricts which opcodes are available based on game state:

```javascript
let allowed;
if (game.gameOver) {
  allowed = new Set([7, 8, 9]);   // must halt — game ended
} else if (stepCount === 0) {
  allowed = new Set([5, 6]);       // first step: observe or reset
} else {
  allowed = new Set([0, 1, 2, 3, 4]); // gameplay: moves only
}
```

This prevents the model from halting prematurely or resetting mid-game — behaviors that a 350M model would otherwise choose (halt is the shortest opcode, hence highest probability without constraints).

### KV Cache Management

The OS manages incremental KV cache to avoid re-processing the entire prompt each step:

```javascript
// Only decode NEW tokens (not the full prompt)
const newTokens = promptTokens.slice(kvCacheLen);
if (newTokens.length > 0) {
  // Sliding window: if approaching limit, reset and re-decode
  if (kvCacheLen + newTokens.length + maxTokens > 3800) {
    await wllama.kvClear();
    kvCacheLen = 0;
    await wllama.decode(promptTokens, {});
    kvCacheLen = promptTokens.length;
  } else {
    await wllama.decode(newTokens, {});
    kvCacheLen = promptTokens.length;
  }
}
```

## What the LLM-CPU Actually Sees

Instead of a 400-token raw board:

```json
{"board":["..........","..........","..........","..........","..........",
"..........","..........","..........","..........","..........",
"..........","..........","..........",".........","..........","....T.....",
"...TTT....","LLLL......","LLLLSSZZII","IIIILLLLSS"],
"current":{"type":"T","x":4,"y":0,"rotation":0},"next":"I",
"score":100,"lines":1,"level":1,"game_over":false}
```

The model receives a 70-token compiled state:

```json
{"piece":"T","x":4,"rot":0,"next":"I","pile":6,"holes":2,
"rough":8,"profile":"0000011264","score":100,"lines":1,
"step":15,"hint":"reduce roughness, keep flat"}
```

The `profile` field (`"0000011264"`) encodes column heights as a single 10-character string — the model can "see" the board shape without parsing a 200-character grid.

## Boundary Contract

The OS/Program boundary is defined by a single function signature:

```
compileResult(rawState: GameState, stepCount: number) -> string
```

**OS guarantees:**
- The string returned by `compileResult()` will be injected as `<|result|>...<|/result|>`
- Grammar enforcement will constrain the model's response to valid opcodes
- KV cache will be managed transparently
- Phase control will restrict available opcodes based on game state

**Program guarantees:**
- Output fits within ~100 tokens (attention budget)
- All decision-relevant information is included
- Strategic hints guide but don't override model reasoning
- The same interface works regardless of model size

## Extending to Other Domains

The OS/Program pattern generalizes to any cartridge:

| Domain | Program Layer compiles... |
|--------|--------------------------|
| Tetris | Board metrics + hints |
| Robot navigation | Sensor readings + map summary + obstacle alerts |
| Cooking | Recipe step + ingredient state + timer alerts |
| Code generation | AST context + variable scope + type constraints |

The OS layer remains unchanged — grammar enforcement, KV cache, dispatch loop. Only the Program Layer (the `compileResult()` function) changes per domain.

## Implications for Edge Deployment

This architecture directly enables sub-1B model deployment on $35 hardware:

1. **Grammar enforcement** eliminates malformed output (OS layer)
2. **State compilation** eliminates parsing failures (Program layer, input-side)
3. **Phase control** eliminates nonsensical decisions (OS layer)
4. **Compiled hints** reduce the reasoning burden (Program layer)

A 350M model with this full stack plays meaningful Tetris. Without it, even a 3B model produces garbage on raw board state. The verification/compilation layers are multiplicative — they compensate for what the model lacks.

## File Map

| File | Layer | Responsibility |
|------|-------|---------------|
| `demo/tetris-browser/index.html` | All | Complete browser implementation |
| `runtime/tetris.rs` | Engine | Rust game engine (14 tests) |
| `cart/game/tetris/manifest.json` | Engine | Cartridge manifest |
| `cart/game/tetris/full.md` | Spec | Method documentation |
| `grammar/isa.gbnf` | OS | Grammar definition |
| `runtime/dispatch.rs` | OS | Opcode routing |
| `runtime/parser.rs` | OS | ISA state machine |

## Related

- [README.md](../README.md) — Quick start and overview
- [grammar/isa-spec.md](../grammar/isa-spec.md) — Full ISA specification
- [docs/ARCHITECTURE.md](ARCHITECTURE.md) — Runtime internals
