# Cartridge index — LLM-OS v0.01

Top-level cart directory. Each domain has its own index that lists the cartridges it contains. The daemon walks `cart/<domain>/<name>/manifest.json` at startup.

## Domains

- [`domestic`](domestic/index.md) — household-scoped cartridges (cooking, electrical).
- [`io`](io/index.md) — physical-world bridges (`roclaw`).
- [`sim`](sim/index.md) — deterministic fixtures for the e2e harness (`sim_world`).
- [`system`](system/index.md) — meta cartridges (`demo`).
