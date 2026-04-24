# ISA Grammar Test Fixtures

Run against `../isa.gbnf` with `llama-gbnf-validator` once llama.cpp is built:

```bash
# Build llama.cpp (one-time):
git clone https://github.com/ggerganov/llama.cpp && cd llama.cpp && cmake -B build && cmake --build build --target llama-gbnf-validator -j

# Run (from this tests/ dir):
for f in legal/*.txt;   do llama-gbnf-validator ../isa.gbnf "$f"; done  # expect all PASS
for f in illegal/*.txt; do llama-gbnf-validator ../isa.gbnf "$f"; done  # expect all FAIL
```

## legal/ — grammar MUST accept

| File | Exercises |
|---|---|
| 01_read_and_halt.txt | `<\|read\|>` + result-block + clean halt |
| 02_write_and_halt.txt | `<\|write\|>` + ack |
| 03_roclaw_call.txt | `<\|call\|>` cartridge syscall |
| 04_closed_loop_nav.txt | `<\|loop\|>`/`<\|break\|>` with nested `<\|call\|>` + `<\|think\|>` |
| 05_fault_and_policy.txt | `<\|yield\|>`, `<\|commit\|>`, `<\|fault\|>`, `<\|policy\|>`, partial status |
| 06_fork.txt | `<\|fork\|>` |

## illegal/ — grammar MUST reject

| File | Invariant violated |
|---|---|
| 01_halt_inside_loop.txt | I1 (halt at depth 0) |
| 02_unclosed_loop.txt | I2 (every loop matched) |
| 03_break_at_toplevel.txt | I3 (break inside loops) |
| 04_call_without_result.txt | I4 (result-block after syscall) |
| 05_bad_status.txt | enum `status` violation |
| 06_no_halt.txt | program must end with halt-stmt |

## Status (2026-04-24)

Validator not installed on this host (`which llama-gbnf-validator` → not found).
Build or port to a Pi 5 / dev box with llama.cpp before merging v0.01.
