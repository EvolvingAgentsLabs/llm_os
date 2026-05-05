# cart/system — Cross-cutting cartridges

Cartridges that aren't tied to a specific domain — smoke tests, kernel diagnostics, and generic utilities.

| Cartridge | Description |
|---|---|
| [`echo`](echo/manifest.json) | Trivial cartridge: `say({text})` emits a greeting; `ping({})` returns pong. Used to validate kernel generalization beyond Tetris. |
