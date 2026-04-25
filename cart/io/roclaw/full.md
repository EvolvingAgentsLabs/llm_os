# roclaw — full spec

Built-in handler. Each method compiles its args + the matching opcode into a 6-byte frame `[0xAA] [op] [L] [R] [XOR] [0xFF]` and sends to UDP `127.0.0.1:4210` (override via `LLM_OS_ROCLAW_ADDR`).

## Opcodes (13, per RoClaw v1.1)

| Method | Opcode | Args | Notes |
|---|---|---|---|
| `forward`      | `0x01` | `{left, right}` | Differential motion |
| `backward`     | `0x02` | `{left, right}` | |
| `turn_left`    | `0x03` | `{left, right}` | |
| `turn_right`   | `0x04` | `{left, right}` | |
| `rotate_cw`    | `0x05` | `{degrees, speed}` | |
| `rotate_ccw`   | `0x06` | `{degrees, speed}` | |
| `stop`         | `0x07` | `{}` | |
| `get_status`   | `0x08` | `{}` | |
| `set_speed`    | `0x09` | `{left, right}` | |
| `move_steps`   | `0x0A` | `{left, right}` | |
| `move_steps_r` | `0x0B` | `{left, right}` | |
| `led_set`      | `0x10` | `{led, value}` | |
| `reset`        | `0xFE` | `{}` | |

## Result shape

```json
{ "ok": true, "bytes": 6, "hex": "AA 01 96 96 01 FF" }
```

On UDP send failure: `{"ok": false, "bytes": 6, "hex": "...", "error": "..."}` — the model decides whether to retry.

## Reference

Source: `RoClaw/src/2_qwen_cerebellum/bytecode_compiler.ts` — port lives in `runtime/roclaw.rs`. Tests in the same file cover the design §1.3 example (`forward 150 150` → `AA 01 96 96 01 FF`).
