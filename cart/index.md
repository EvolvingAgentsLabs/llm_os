# Cartridge index

LLM-OS exists to demonstrate that an LLM-as-CPU can drive non-trivial interactive programs through a grammar-enforced ISA. The cartridges here are the games — each is a complete validation of the kernel pattern against a different game shape.

| Cartridge | Description |
|---|---|
| [`game/tetris`](game/index.md) | Single-loop arcade. Methods: `move({action})`, `observe`, `reset`. The Program layer pre-computes every legal placement, ranks them, and gives the LLM a top-3 list to ratify. |
| [`game/scavenger`](game/index.md) | Multi-step quest. Methods: `move({dir})`, `pickup`, `drop`, `look`. The Program layer runs BFS around walls and pits and tells the LLM the next move. |

To add a cartridge: write a manifest under `<domain>/<name>/manifest.json` matching [kernel/schemas/cartridge.manifest.schema.json](../kernel/schemas/cartridge.manifest.schema.json). The kernel's [Cartridge](../kernel/cartridge.js) class loads the manifest, builds the trie, and exposes per-method opcode index sets for phase control. The Program layer (per-game `compileResult` + planner functions) lives alongside the demo — see [docs/tetris-architecture.md](../docs/tetris-architecture.md) for the OS/Program contract.
