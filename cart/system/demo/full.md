# demo — full spec

Smoke-test cartridge. Two methods, both stubbed in v0.01 (return the v0.01 stub envelope).

- `echo({text})` — return `text`.
- `hash({text, algo?})` — return `{algo, digest_hex}`.
