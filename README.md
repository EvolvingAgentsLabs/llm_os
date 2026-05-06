# LLM-OS

An operating system where the LLM **is** the CPU, demonstrated with games. Pure WebAssembly — runs in your browser, no server, no install.

A 350M-parameter model plays Tetris and solves a grid-world quest by emitting grammar-constrained ISA opcodes. Wrong opcode sequences are physically impossible to emit — the OS layer guarantees output validity at every token, the way a CPU's instruction decoder rejects malformed instructions.

Part of the [Evolving Agents](https://github.com/EvolvingAgentsLabs) ecosystem.

## What's here

Two browser demos, both running on the same kernel + same model. The kernel's job is grammar enforcement and dispatch; the per-game **Program layer** does the strategic lifting (pre-computes the best moves, scores placements, runs pathfinders) and hands the LLM a small list of ratifiable choices. The LLM's job is to pick from a pre-ranked menu and emit valid opcodes — not to plan from scratch.

This separation is the load-bearing claim: the LLM-CPU is the **decoder + selector**, not the **planner**. With a Program rich enough to enumerate options and rank them, even a 350M-param model plays competently. Without it, no model below ~7B does.

## Demos

Each downloads ~230 MB on first load (the LiquidAI LFM 2.5 350M weights), then plays entirely offline via [@wllama/wllama](https://github.com/ngxson/wllama).

**Tetris** — single-loop arcade.

```bash
python demo/tetris-browser/serve.py    # http://localhost:8888/demo/tetris-browser/
```

The Program layer enumerates every legal placement (4 rotations × 10 columns), simulates the post-placement metrics (height, holes, roughness, lines cleared), scores them with Dellacherie-style weights, and surfaces the top 3 as `best_actions` in the compiled state. The LLM picks `best_actions[0]` and emits its opcode sequence.

**Scavenger** — multi-step quest, mechanics inspired by `skillos_robot`.

```bash
python demo/scavenger-browser/serve.py # http://localhost:8889/demo/scavenger-browser/
```

The Program layer runs BFS over the grid respecting walls and pits, returns the first move of the shortest path to the current subgoal as `next_step`. The LLM ratifies a single direction. The compiled-state shape (labeled objects with bearings + distances) deliberately mirrors what `skillos_robot`'s SceneGraph emits from a real camera — the same prompt and opcode set transfer to a real-world robot challenge.

Both demos require COOP/COEP headers (the `serve.py` script provides them) so SharedArrayBuffer works. After model download, no network traffic is needed.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  PROGRAM LAYER (per-game intelligence — does the planning)      │
│    bestActions()       enumerate + simulate + score placements  │
│    findNextStep()      BFS around obstacles to subgoal          │
│    analyzeBoard()      metrics (heights, holes, roughness, …)   │
│    compileResult()     compiled state for the LLM (~80 tokens)  │
└────────────────────────────────┬────────────────────────────────┘
                                 │ injected as <|result|>…<|/result|>
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│  OS LAYER (kernel — domain-agnostic, ratifies and dispatches)   │
│    Token-trie grammar   only valid opcodes can be emitted       │
│    Sampler              picks highest-prob valid token at step  │
│    Phase controller     restricts opcode set by game state      │
│    parseOpcode/         dispatch + result formatting            │
│      formatResult                                               │
└────────────────────────────────┬────────────────────────────────┘
                                 │ opcode executed
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│  GAME ENGINE (the cartridge)                                    │
│    Tetris: collision, rotation, line clearing, gravity          │
│    Scavenger: grid moves, pickup/drop, walls, pits              │
└─────────────────────────────────────────────────────────────────┘
```

The OS guarantees every output is a valid opcode for the active cartridge. The Program decides *what information* and *what options* the LLM-CPU sees — not raw board strings, but compiled metrics and pre-ranked actions. A 350M model parsing a 400-token raw board makes random moves; the same model receiving a 70-token compiled state with `best_actions: [...]` plays competently.

Full design: [docs/tetris-architecture.md](docs/tetris-architecture.md).

## Why this works on a 350M model

1. **Grammar enforcement** eliminates malformed output — invalid tokens are never sampled (OS layer). No retries, no rejection — physically prevented.
2. **Action enumeration** is in the Program layer, not the LLM. The Program enumerates every legal placement (Tetris) or runs BFS (Scavenger); the LLM picks from a small sorted list.
3. **State compilation** reduces a 400-token raw board / grid to ~80 tokens of decision-relevant state.
4. **Phase control** restricts which opcodes are available based on game state — the model can't `<|halt|>` mid-game or `pickup` while moving.

These compensate for what the model lacks. Without this stack, even a 3B model produces garbage on raw state.

## ISA

The full 14-opcode ISA spec is in [grammar/isa-spec.md](grammar/isa-spec.md). The current cartridges exercise a subset — `<|call|>cart.method` and `<|halt|>status=...` — sufficient to demonstrate the OS/Program separation.

## Repo layout

```
kernel/                # reusable JS kernel (token-trie, Sampler, dispatch)
                       # — embed in any wllama-driven frontend
demo/tetris-browser/   # arcade demo — single self-contained HTML
demo/scavenger-browser/# quest demo — same kernel, different cartridge
cart/game/tetris/      # tetris cartridge manifest + schemas
cart/game/scavenger/   # scavenger cartridge manifest + schemas
grammar/isa-spec.md    # 14-opcode ISA reference
docs/tetris-architecture.md   # OS/Program layer thesis
```

## License

Apache 2.0.
