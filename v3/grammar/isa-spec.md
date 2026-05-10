# LLM-OS v2 — ISA Specification

The v2 ISA is the same 14-opcode set from v1, but v2 implements all of them
in the kernel executor. The model emits opcodes as structured text; the kernel
parses and dispatches them. Grammar enforcement is handled server-side via
GBNF or structured output constraints, not by a client-side token trie.

## 1. Opcode table

| Opcode | Arity | Form | Kernel response | Purpose |
|---|---|---|---|---|
| `<\|call\|>` | cart.method, args | `<\|call\|>cart.method {json}<\|/call\|>` | `<\|result\|>...<\|/result\|>` | Cartridge syscall. |
| `<\|read\|>` | fd, len | `<\|read\|>fd=N len=N` | `<\|result\|>...<\|/result\|>` | Read from I/O port. |
| `<\|write\|>` | fd, payload | `<\|write\|>fd=N {json}` | `<\|ack\|>` | Write to I/O port. |
| `<\|halt\|>` | status | `<\|halt\|>status=success\|failure\|partial` | (program ends) | Terminus. Only at depth 0. |
| `<\|loop\|>` | goal | `<\|loop\|>goal=label` | — | Enter loop. |
| `<\|break\|>` | — | `<\|break\|>` | — | Exit innermost loop. |
| `<\|think\|>` | prose | `<\|think\|>...<\|/think\|>` | — | Inner monologue. |
| `<\|commit\|>` | json | `<\|commit\|>{json}` | — | Persist state. |
| `<\|fork\|>` | goal | `<\|fork\|>goal=label` | — | Spawn subtask. |
| `<\|yield\|>` | — | `<\|yield\|>` | — | Yield CPU. |
| `<\|wait\|>` | fd | `<\|wait\|>fd=N` | `<\|result\|>...<\|/result\|>` | Block on fd. |
| `<\|fault\|>` | json | `<\|fault\|>{json}` | `<\|result\|>...<\|/result\|>` | Raise exception. |
| `<\|policy\|>` | — | `<\|policy\|>` | `<\|result\|>...<\|/result\|>` | Query capabilities. |

## 2. Key differences from v1

| Aspect | v1 | v2 |
|---|---|---|
| Implemented opcodes | `call`, `halt` only | All 14 |
| Grammar enforcement | Client-side token trie (JS) | Server-side (model constraints / GBNF) |
| Arguments | Enumerated (each combo is a trie entry) | Schema-constrained (JSON schema per method) |
| Model | LFM 2.5 350M (browser, wllama) | Any model via OpenRouter (Gemma 4 31B default) |
| Program layer | Required (compileResult, bestActions) | Eliminated — LLM reasons from raw state |
| Context window | ~3800 tokens (effective) | 128K-256K (model-dependent) |
| Execution | Single-step (1 opcode per doStep call) | Full program (loop until halt) |
| State persistence | None | `<\|commit\|>` → key-value store |
| I/O model | Hardcoded game state injection | Typed I/O ports (fd=0 stdin, fd=1 stdout) |

## 3. Cartridge manifests (v2 format)

v2 manifests declare methods with argument schemas instead of enumerating
every possible opcode string:

```json
{
  "name": "tetris",
  "methods": {
    "move": {
      "summary": "Move the active piece.",
      "args_schema": "schemas/move.args.schema.json"
    }
  },
  "ports": {
    "stdin": {"fd": 0, "type": "keyboard"}
  },
  "halt": ["success", "failure", "partial"]
}
```

The kernel generates the ISA description for the system prompt from the
manifest + resolved schemas. The LLM sees method signatures with typed
args, not a fixed list of opcode strings.

## 4. Grammar invariants (unchanged from v1)

1. `<|halt|>` at depth 0 only.
2. Every `<|loop|>` matched by `<|break|>`.
3. `<|break|>` inside loops only.
4. Result-blocks after syscalls (`call`, `read`, `wait`, `fault`, `policy`).
5. `<|ack|>` after `<|write|>`.
