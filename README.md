# llmos

An operating system where the LLM is the CPU.
13-opcode ISA enforced by GBNF grammar at decode time — wrong sequences are physically impossible to emit.

Part of the [Evolving Agents](https://github.com/EvolvingAgentsLabs) ecosystem.

## Install

```bash
git clone https://github.com/EvolvingAgentsLabs/llm_os.git
cd llm_os
cargo build --release
```

Prerequisites: Rust >= 1.75, llama.cpp (with `--grammar`), a GGUF model (default: Qwen 2.5 3B Q4_K_M).

## Use

```bash
# One-shot bootstrap (build + validate + start daemon)
llmos start

# Dispatch a goal
llmos dispatch "summarize the file at ./grammar/isa-spec.md"

# Inspect runtime state
llmos trace          # recent dispatches
llmos policy         # current capability mask
llmos cartridges     # mounted syscalls

# Validate grammar fixtures
llmos validate       # 12/12 must pass

# Dev mode (regular Linux/macOS)
llmos dev --model ~/models/qwen-2.5-3b-q4.gguf

# Release mode (Buildroot Pi 5 image)
llmos image --model ~/models/qwen-2.5-3b-q4.gguf
```

## How it works

The LLM generates a token stream constrained by GBNF grammar. Each token maps to one of 13 opcodes. The I/O daemon (`iod`) intercepts syscall opcodes, validates arguments against JSON schemas, executes cartridge handlers, and injects results back into the token stream.

```
POSIX                    LLM-OS
────────────────────     ──────────────────────────────
CPU instructions         13 opcodes (GBNF tokens)
RAM                      KV cache
MMU / type safety        GBNF grammar (sampler-enforced)
Swap                     KV compaction (ISA-aware)
Ring 0 / Ring 3          Logit bias capability mask
Syscalls                 <|call|> + cartridge schemas
```

Six cartridges mounted: `system/summarize`, `system/demo`, `io/roclaw`, `sim/sim_world`, `domestic/cooking`, `domestic/residential-electrical`.

## Architecture

Ring model, ISA spec, kernel internals, and v1.0 roadmap:
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)

ISA spec: [grammar/isa-spec.md](grammar/isa-spec.md)
Operator guide: [docs/USAGE.md](docs/USAGE.md)
Write a cartridge: [docs/TUTORIAL.md](docs/TUTORIAL.md)
Roadmap: [docs/NEXT_STEPS.md](docs/NEXT_STEPS.md)

## License

Apache 2.0
