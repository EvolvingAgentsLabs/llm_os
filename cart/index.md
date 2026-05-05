# Cartridge index

Each cartridge declares its methods, JSON arg schemas, and the opcodes the LLM-CPU may emit. The browser demo currently mounts one game cartridge; cross-cutting smoke tests live under `system/`.

| Domain | Cartridge | Description |
|---|---|---|
| [`game`](game/index.md) | [`tetris`](game/tetris/full.md) | Turn-based Tetris. Methods: `move({action})`, `observe`, `reset`. |
| [`system`](system/index.md) | [`echo`](system/echo/manifest.json) | Smoke-test cartridge. Methods: `say({text})`, `ping({})`. Validates kernel generalization. |

To add a cartridge: write a manifest under `<domain>/<name>/manifest.json` declaring its methods + opcode strings (see [kernel/schemas/cartridge.manifest.schema.json](../kernel/schemas/cartridge.manifest.schema.json)). The kernel's [Cartridge](../kernel/cartridge.js) class loads the manifest, builds the trie, and exposes per-method opcode index sets for phase control. Cartridge implementations (the Program-layer logic that handles result payloads) live alongside the calling code — see [docs/tetris-architecture.md](../docs/tetris-architecture.md) for the OS/Program contract.
