# LLM-OS

An operating system where the LLM **is** the CPU. Pure WebAssembly — runs in your browser, no server, no install.

A 350M-parameter model plays Tetris by emitting grammar-constrained ISA opcodes. Wrong opcode sequences are physically impossible to emit — the OS layer guarantees output validity at every token, the way a CPU's instruction decoder rejects malformed instructions.

Part of the [Evolving Agents](https://github.com/EvolvingAgentsLabs) ecosystem.

## Demos

Two browser demos, both running on the same kernel + same model. Each downloads ~230 MB on first load (the LiquidAI LFM 2.5 350M weights), then plays entirely offline via [@wllama/wllama](https://github.com/ngxson/wllama).

**Tetris** — single-loop arcade. The LLM observes the board, decides a move, sees the consequence.

```bash
cd demo/tetris-browser && python3 serve.py    # http://localhost:8888
```

**Scavenger** — multi-step quest. The LLM navigates a grid, finds a red cube, picks it up, delivers it to a blue square. Compiled-state shape (labeled objects with bearings + distances) deliberately mirrors what `skillos_robot`'s SceneGraph emits from a real camera — the same prompt and opcode set transfer to a real-world robot challenge.

```bash
cd demo/scavenger-browser && python3 serve.py    # http://localhost:8889
```

Both demos require COOP/COEP headers (the `serve.py` script provides them) so SharedArrayBuffer works. After model download, no network traffic is needed.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  PROGRAM LAYER (domain-specific intelligence)                   │
│    analyzeBoard()   → metrics: heights, holes, roughness        │
│    generateHint()   → strategy: "fill left well", "clear"       │
│    compileResult()  → 60-80 token compiled state for the LLM    │
└────────────────────────────────┬────────────────────────────────┘
                                 │ injected as <|result|>…<|/result|>
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│  OS LAYER (domain-agnostic infrastructure)                      │
│    Token-trie grammar → only valid opcodes can be emitted       │
│    KV cache manager   → incremental decode + sliding window     │
│    Phase controller   → restricts opcodes by game state         │
│    Dispatch loop      → parse opcode, execute, inject result    │
└────────────────────────────────┬────────────────────────────────┘
                                 │ opcode executed
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│  GAME ENGINE (the cartridge)                                    │
│    Tetris rules: collision, rotation, line clearing, gravity    │
└─────────────────────────────────────────────────────────────────┘
```

The OS guarantees every output is a valid `<|call|>tetris.move {...}<|/call|>`. The Program decides *what information* the LLM-CPU sees — not raw board strings, but compiled hints. A 350M model parsing a 400-token raw board makes random moves; the same model receiving a 70-token compiled state plays competent Tetris.

Full design: [docs/tetris-architecture.md](docs/tetris-architecture.md).

## Why this works on a 350M model

1. **Grammar enforcement** eliminates malformed output — invalid tokens are never sampled (OS layer). No retries, no rejection — physically prevented.
2. **State compilation** reduces a 400-token raw board to a 70-token compiled state with column profile + strategic hints (Program layer, input-side).
3. **Phase control** restricts which opcodes are available based on game state — the model can't `<|halt|>` mid-game.
4. **Compiled hints** carry the strategic reasoning the small model can't produce on its own.

These compensate for what the model lacks. Without this stack, even a 3B model produces garbage on raw board state.

## ISA

The full 14-opcode ISA spec is in [grammar/isa-spec.md](grammar/isa-spec.md). The browser demo currently exercises a subset — `<|call|>tetris.{move,observe,reset}` and `<|halt|>status=...` — sufficient to demonstrate the OS/Program separation. Extending to other cartridges is a matter of writing a new `compileResult()` and adding the cartridge's opcode strings to the trie.

## Repo layout

```
kernel/                # reusable JS kernel (token-trie, Sampler, dispatch)
                       # — embed in any wllama-driven frontend
demo/tetris-browser/   # arcade demo — single self-contained HTML
demo/scavenger-browser/# quest demo — same kernel, different cartridge
cart/game/tetris/      # tetris cartridge manifest + schemas
cart/game/scavenger/   # scavenger cartridge manifest + schemas
cart/system/echo/      # smoke-test cartridge (kernel generalization)
grammar/isa-spec.md    # 14-opcode ISA reference
docs/tetris-architecture.md   # OS/Program layer thesis
```

## License

Apache 2.0.
