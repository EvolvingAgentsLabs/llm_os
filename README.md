# llmos

An operating system where the LLM is the CPU.
14-opcode ISA enforced by GBNF grammar at decode time — wrong sequences are physically impossible to emit.

Part of the [Evolving Agents](https://github.com/EvolvingAgentsLabs) ecosystem.

## Install

```bash
git clone https://github.com/EvolvingAgentsLabs/llm_os.git
cd llm_os
cargo build --release
```

Prerequisites: Rust >= 1.75, a GGUF model (default: Qwen 2.5 3B Q4_K_M).

Optional: llama.cpp vendored at `runtime/vendor/llama.cpp` for in-process inference (feature `ffi-inference`).

## Use

```bash
# Dev mode — in-process inference (default, no external server)
bash scripts/dev.sh --model ~/models/qwen-2.5-3b-q4.gguf

# Legacy mode — external llama-server (HTTP/SSE)
iod --legacy-server --server http://localhost:8080 --grammar grammar/isa.gbnf

# Dispatch a goal
iod dispatch "summarize the file at ./grammar/isa-spec.md"

# Inspect runtime state
iod trace          # recent dispatches
iod policy         # current capability mask
iod cartridges     # mounted syscalls

# Validate grammar fixtures
iod validate       # 12/12 must pass

# Release mode (Buildroot Pi 5 image)
bash image/build.sh --model ~/models/qwen-2.5-3b-q4.gguf
```

## How it works

The LLM generates a token stream constrained by GBNF grammar. Each token maps to one of 14 opcodes. The I/O daemon (`iod`) drives inference in-process via FFI to llama.cpp, intercepts syscall opcodes, validates arguments against JSON schemas, executes cartridge handlers (WASM-sandboxed or legacy in-process), and injects results directly into the KV cache.

```
POSIX                    LLM-OS
────────────────────     ──────────────────────────────
CPU instructions         14 opcodes (GBNF tokens)
RAM                      KV cache
MMU / type safety        GBNF grammar (sampler-enforced)
Swap / paging            Deterministic KV pager (anchors + eviction)
Ring 0 / Ring 3          WASM cartridge sandbox + logit bias
Syscalls                 <|call|> + cartridge schemas
Virtual memory           Grammar stack (push/pop sub-grammars)
```

Six cartridges mounted: `system/summarize`, `system/demo`, `io/roclaw`, `sim/sim_world`, `domestic/cooking`, `domestic/residential-electrical`.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  iod (single binary)                                              │
│    ├── sampler.rs       (in-process token generation + grammar)    │
│    ├── llama_ffi.rs     (thin C-ABI wrapper over llama.cpp)       │
│    ├── kv_pager.rs      (deterministic eviction with anchors)     │
│    ├── wasm_host.rs     (wasmtime cartridge sandbox)              │
│    ├── dispatch.rs      (opcode → handler routing)                │
│    ├── parser.rs        (14-opcode ISA state machine)             │
│    └── capability.rs    (logit bias + daemon-side reject)         │
│                                                                    │
│  ┌────────────────────────────────────────────────────┐            │
│  │  libllama.a (statically linked, feature-gated)     │            │
│  │    - model load, decode, sample, KV cache ops      │            │
│  └────────────────────────────────────────────────────┘            │
└──────────────────────────────────────────────────────────────────┘
```

Ring model, ISA spec, kernel internals, and v1.0 details:
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)

ISA spec: [grammar/isa-spec.md](grammar/isa-spec.md)
Operator guide: [docs/USAGE.md](docs/USAGE.md)
Write a cartridge: [docs/TUTORIAL.md](docs/TUTORIAL.md)
Improvement plan: [docs/IMPROVEMENT_PLAN.md](docs/IMPROVEMENT_PLAN.md)

## Feature flags

```toml
[features]
default = ["legacy-server", "legacy-handlers"]

# In-process inference via FFI to llama.cpp (requires vendored libllama)
ffi-inference = []

# Legacy mode: HTTP/SSE to external llama-server
legacy-server = []

# WASM cartridge sandbox via wasmtime
wasm-sandbox = ["dep:wasmtime"]

# In-process Rust handlers (default until WASM is fully wired)
legacy-handlers = []
```

## Model requirements

The runtime works with any GGUF model. For optimal performance with the ISA:

| Model | Size | Speed (Pi 5) | Notes |
|---|---|---|---|
| Qwen 2.5 0.5B Q4_K_M | ~350 MB | ~15 tok/s | Dev/testing only |
| Qwen 2.5 3B Q4_K_M | ~1.7 GB | ~8 tok/s | Release sweet spot |
| Qwen 2.5 7B Q4_K_M | ~4.1 GB | ~2-3 tok/s | Max quality ceiling |

For best results, fine-tune with `scripts/rebuild_tokenizer.py` to ensure all 20 ISA tokens are single-token (uniform latency, exact capability enforcement).

## Scripts

| Script | Purpose |
|---|---|
| `scripts/dev.sh` | Development build + run |
| `scripts/quickstart.sh` | Cold checkout to first task |
| `scripts/validate_grammar.sh` | 12/12 grammar fixture validator |
| `scripts/check_tokenizer.sh` | Tokenization parity + `--model` validation mode |
| `scripts/rebuild_tokenizer.py` | Full ISA fine-tune pipeline (patch/validate/finetune/export) |
| `scripts/promote_traces.py` | JSONL traces to DPO triples (self-hosting) |

## License

Apache 2.0
