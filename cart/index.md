# Cartridge index

Each cartridge declares its methods, JSON arg schemas, and the opcodes it consumes. The browser demo currently mounts one:

| Domain | Cartridge | Description |
|---|---|---|
| [`game`](game/index.md) | [`tetris`](game/tetris/full.md) | Turn-based Tetris. Methods: `move({action})`, `observe`, `reset`. |

To add a cartridge, write a new `compileResult()` (Program layer) and append its opcode strings to the token trie in [`demo/tetris-browser/index.html`](../demo/tetris-browser/index.html). See [docs/tetris-architecture.md](../docs/tetris-architecture.md) for the OS/Program contract.
