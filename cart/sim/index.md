# cart/sim — Deterministic fixtures

Cartridges used only by the e2e harness. Out of scope for production builds.

| Cartridge | Description |
|---|---|
| [`sim_world`](sim_world/full.md) | 4×1 grid world. Methods: `step({dir})`, `observe`, `reset`. Goal: reach `(3,0)` from `(0,0)`. Used by `tests/e2e/navigate_and_report.sh`. |
