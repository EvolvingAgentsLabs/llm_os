# LLM-OS v0.01 — ISA Specification

Companion to `isa.gbnf`. Canonical reference for the 13-opcode LLM-OS ISA in its bootstrap form (string-pattern opcodes on the Qwen 3 2B tokenizer).

## 1. Opcode table

| Opcode | Arity | Form | Daemon response | Purpose |
|---|---|---|---|---|
| `<\|read\|>` | fd, len | `<\|read\|>fd=N len=N\n` | `<\|result\|>…<\|/result\|>` | Read `len` bytes from fd; bytes injected as result. |
| `<\|write\|>` | fd, payload | `<\|write\|>fd=N <json>\n` | `<\|ack\|>` | Write JSON payload to fd. |
| `<\|call\|>` | cart.method, args | `<\|call\|>cart.method <json> <\|/call\|>\n` | `<\|result\|>…<\|/result\|>` | Cartridge syscall. Per-cartridge schema validation runtime-side. |
| `<\|yield\|>` | — | `<\|yield\|>\n` | (scheduler swap) | Voluntarily release the CPU (preemptive hook point). |
| `<\|fork\|>` | goal | `<\|fork\|>goal=<label>\n` | — | Spawn subtask with new KV branch. |
| `<\|wait\|>` | fd | `<\|wait\|>fd=N\n` | `<\|result\|>…<\|/result\|>` | Block on fd until data available. |
| `<\|loop\|>` | goal | `<\|loop\|>goal=<label>\n … <\|break\|>\n` | — | Enter closed-loop region. Terminated by `<\|break\|>`. |
| `<\|break\|>` | — | `<\|break\|>\n` | — | Exit innermost loop. Only legal inside `<\|loop\|>`. |
| `<\|halt\|>` | status | `<\|halt\|>status=success\|failure\|partial\n` | (program ends) | Program terminus. Only legal at loop depth 0. |
| `<\|think\|>` | prose | `<\|think\|>…<\|/think\|>\n` | — | Inner monologue — stays in KV, not copied to fd 1. |
| `<\|commit\|>` | json | `<\|commit\|><json>\n` | — | Materialize working state into persistent cartridge memory. |
| `<\|fault\|>` | json | `<\|fault\|><json>\n` | `<\|result\|>…<\|/result\|>` | Raise typed exception; handler cartridge replies. |
| `<\|policy\|>` | — | `<\|policy\|>\n` | `<\|result\|>…<\|/result\|>` | Query current permission set; daemon updates logit bias. |

## 2. Grammar invariants (structural)

1. **`<|halt|>` at depth 0.** `halt-stmt` appears only in `program`, never in `loop-stmt`. A model that tries to halt inside a loop is decode-rejected.
2. **Every `<|loop|>` matched.** `loop-block ::= "<|loop|>…" loop-stmt* break-stmt` — the terminal `break-stmt` is mandatory. No unclosed loops.
3. **`<|break|>` inside loops only.** `break-stmt` appears only in `loop-stmt`, never in `top-stmt`.
4. **Result-blocks mandatory.** `read-stmt`, `call-stmt`, `wait-stmt`, `fault-stmt`, `policy-stmt` all end with `result-block`. The daemon pauses sampling, injects `<|result|>…<|/result|>`, and resumes — the grammar predicts the shape of what will be injected.
5. **`<|write|>` ack'd.** `write-stmt` ends with `<|ack|>`, same daemon-injection pattern.

Invariants 4 and 5 mean: the grammar is a *contract between sampler and daemon.* Neither side is trusted alone; both must agree.

## 3. Legal examples (one per opcode; these MUST parse)

```
<|read|>fd=3 len=128
<|result|>hello world<|/result|>
<|halt|>status=success
```

```
<|write|>fd=1 {"msg":"ready"}
<|ack|>
<|halt|>status=success
```

```
<|call|>roclaw.forward {"left":150,"right":150} <|/call|>
<|result|>{"ok":true,"bytes":6}<|/result|>
<|halt|>status=success
```

```
<|loop|>goal=navigate_to_kitchen
<|think|>I see a hallway. Kitchen is left.<|/think|>
<|call|>roclaw.turn_left {"degrees":90} <|/call|>
<|result|>ok<|/result|>
<|break|>
<|halt|>status=success
```

```
<|fork|>goal=compact_context
<|halt|>status=success
```

```
<|think|>planning next move<|/think|>
<|yield|>
<|commit|>{"plan":"retry with lower speed"}
<|fault|>{"kind":"schema_violation","needs_cloud":true}
<|result|>{"cloud_response":"use speed=100"}<|/result|>
<|policy|>
<|result|>{"allowed":["read","write","call.roclaw.*"]}<|/result|>
<|halt|>status=partial
```

## 4. Illegal examples (MUST be decode-rejected)

```
# I1 violation — halt inside loop
<|loop|>goal=x
<|halt|>status=success
<|break|>
```

```
# I2 violation — unclosed loop
<|loop|>goal=x
<|think|>never breaks<|/think|>
<|halt|>status=failure
```

```
# I3 violation — break at top level
<|break|>
<|halt|>status=success
```

```
# I4 violation — call without result-block
<|call|>roclaw.forward {"left":150,"right":150} <|/call|>
<|halt|>status=success
```

## 5. Migration to kernel-native (Month 2 fine-tune)

Bootstrap opcodes like `<|read|>` tokenize to ~5–8 Qwen 3 2B tokens each. After the Month 2 kernel fine-tune (add 13 special tokens to tokenizer + post-train on trace data), each opcode becomes **one token**. Concretely:

| Change | Effect |
|---|---|
| Add `<\|read\|>`, `<\|write\|>`, `<\|call\|>`, `<\|/call\|>`, `<\|yield\|>`, `<\|fork\|>`, `<\|wait\|>`, `<\|loop\|>`, `<\|break\|>`, `<\|halt\|>`, `<\|think\|>`, `<\|/think\|>`, `<\|commit\|>`, `<\|fault\|>`, `<\|policy\|>`, `<\|result\|>`, `<\|/result\|>`, `<\|ack\|>` to tokenizer | 18 new token IDs. Each opcode becomes 1 token. |
| Switch GBNF to token-ID form: `root ::= <[N_halt]> status …` | Grammar operates at token level, not character level. Decode mask is tighter. |
| Ban all 18 IDs via `--logit-bias` in cartridges that lack the capability | Capability enforcement at decode. |
| Per-cartridge JSON-schema→GBNF compilation for `<\|call\|>` args | Eliminates schema-validated-retry runtime cost for well-typed calls. |

Effective throughput on Pi 5 jumps from ~8 Hz × multi-token opcodes (≈1–2 "syscalls/sec") to ~8 Hz × 1-token opcodes (≈8 "syscalls/sec") for syscall-dense workloads — the 5–10× gain called out in design §6.2.

## 6. What this grammar does NOT yet do (deferred)

- **Per-cartridge call arg validation.** `<|call|>` args are any JSON. Cartridge schemas are validated runtime-side in the I/O daemon. Compile-time GBNF per cartridge lands in v0.1.
- **Preemption points.** Preemptive scheduler (Month 6) requires the daemon to be able to inject `<|yield|>` at grammar-legal statement boundaries mid-stream; the current grammar permits yield as a `top-stmt`/`loop-stmt` but there is no mid-argument preemption.
- **Nested loop depth limit.** Grammar permits unbounded nesting. Runtime daemon SHOULD cap this (proposed: depth ≤ 4).
- **Result-block contents.** Grammar treats results as opaque text. Per-cartridge result schemas land with the Month 2 compilation path.
- **Policy enforcement.** `<|policy|>` returns the current capability set as a daemon message; actual bias changes happen in the daemon, not the grammar.

## 7. Validation status

See `tests/` for fixture files. Run:

```
llama-gbnf-validator isa.gbnf tests/legal/*.txt   # all must PASS
llama-gbnf-validator isa.gbnf tests/illegal/*.txt # all must FAIL
```

Validator availability on this machine: not yet checked. See `memory/short_term/` for the build-from-source path if `llama.cpp` is not installed.
