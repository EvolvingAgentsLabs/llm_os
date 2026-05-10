# LLM-OS v2

The LLM **is** the CPU. No Program layer — the model reasons from raw state and writes bytecode programs.

v2 replaces the v1 architecture (350M selector model, browser-side token trie, enumerated opcodes) with a server-side kernel that talks to any model via OpenRouter and implements the full 14-opcode ISA.

## Quick start

```bash
export OPENROUTER_API_KEY=sk-or-v1-...

# Play tetris (Gemma 4 31B reasons from raw board state)
cd v2
node bin/llm-os.js --cart cart/game/tetris \
  --task "Play tetris. Observe the board, reason about piece placement, clear lines." \
  --verbose

# Print to a virtual terminal (microcomputer-style I/O)
node bin/llm-os.js --cart cart/terminal \
  --task "Print HELLO WORLD to the center of the screen, then halt."

# Or use npm scripts
npm run tetris
npm run terminal
```

## Architecture

```
                     ┌──────────────────────────────────┐
                     │         OpenRouter API            │
                     │   (Gemma 4 31B / any model)       │
                     └──────────┬───────────────────────┘
                                │ chat completions
                                ▼
┌─────────────────────────────────────────────────────────┐
│  KERNEL (kernel/executor.js)                            │
│                                                         │
│  Execution loop:                                        │
│    generate → parse opcode → dispatch → inject result   │
│                                                         │
│  Full ISA: call, read, write, loop, break, halt,        │
│            commit, think, fork, yield, wait, fault,     │
│            policy                                       │
│                                                         │
│  State: persistent commit store, loop stack,            │
│         conversation history with compaction            │
└──────────────┬──────────────────────────────────────────┘
               │ dispatch
               ▼
┌─────────────────────────────────────────────────────────┐
│  CARTRIDGES (cart/)                                      │
│                                                         │
│  game/tetris/  — 10x20 board, raw state, no hints       │
│  terminal/     — 80x24 char screen + keyboard (fd 0/1)  │
│                                                         │
│  Each cartridge = manifest.json + argument schemas      │
│  + engine (JS module that handles dispatched calls)     │
└─────────────────────────────────────────────────────────┘
```

## v1 vs v2

| | v1 | v2 |
|---|---|---|
| Model | LFM 2.5 350M (browser) | Any model via OpenRouter |
| LLM role | Selector (picks from pre-computed options) | Programmer (reasons and writes bytecode) |
| Program layer | Required (compileResult, bestActions) | Eliminated |
| ISA opcodes | 2 of 14 implemented (call, halt) | All 14 implemented |
| Arguments | Enumerated strings in trie | JSON schema constrained |
| Context | ~3800 tokens | 128K-256K |
| Runtime | wllama in browser (WebAssembly) | Node.js + OpenRouter HTTP |
| Grammar | Client-side token trie | Server-side constraints |

## Cartridge format

v2 cartridges declare methods with JSON schemas:

```json
{
  "name": "tetris",
  "methods": {
    "move": {
      "summary": "Move the active piece.",
      "args_schema": "schemas/move.args.schema.json"
    }
  },
  "ports": {
    "stdin": {"fd": 0, "type": "keyboard"}
  },
  "halt": ["success", "failure", "partial"]
}
```

The kernel generates the ISA description for the system prompt automatically.

## Adding a new cartridge

1. Create `cart/<name>/manifest.json` with methods and schemas
2. Create argument schemas in `cart/<name>/schemas/`
3. Wire the engine in `bin/llm-os.js` (or register an I/O handler)
4. Run: `node bin/llm-os.js --cart cart/<name> --task "..."`

## I/O ports (microcomputer model)

Cartridges can declare I/O ports that map to file descriptors:

- `fd=0` — stdin (keyboard, sensor input)
- `fd=1` — stdout (screen, actuator output)
- `fd=2` — stderr (logs)

The LLM reads from ports with `<|read|>fd=0 len=1` and writes with
`<|write|>fd=1 {"x":10,"y":5,"char":"*"}`. This is the same closed-loop
model as a Z80 reading a keyboard port and writing to video RAM.

## ISA reference

See [grammar/isa-spec.md](grammar/isa-spec.md) for the full 14-opcode spec.
