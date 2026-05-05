# LLM-OS

An operating system where the LLM **is** the CPU.

14-opcode ISA enforced by GBNF grammar at decode time — wrong sequences are physically impossible to emit. Programs provide structured state to guide the LLM's decisions; the OS guarantees every output is a valid instruction.

Part of the [Evolving Agents](https://github.com/EvolvingAgentsLabs) ecosystem.

## Demo: LLM Plays Tetris (Browser, 350M params)

A 350M-parameter model plays Tetris entirely in your browser. No server — the model runs via WebAssembly with JS-side grammar enforcement.

```bash
cd demo/tetris-browser
python3 serve.py    # Needed for COOP/COEP headers (SharedArrayBuffer)
open http://localhost:8888
```

Click **Load Model** (downloads ~230MB once), then **Auto Play** to watch it play.

This demo showcases the two-layer architecture:

| Layer | Role | Code |
|-------|------|------|
| **OS** | Grammar enforcement (token trie), KV cache, dispatch | `generateConstrained()`, `doStep()` |
| **Program** | Board analysis, hint generation, state compilation | `compileResult()`, `analyzeBoard()` |

The OS guarantees every output is a valid `<|call|>tetris.move {...}<|/call|>`. The Program decides *what information* the LLM-CPU sees — not raw board strings, but compiled hints: pile height, holes, roughness, strategic suggestions.

See [docs/tetris-architecture.md](docs/tetris-architecture.md) for the full design.

## Install

```bash
git clone https://github.com/EvolvingAgentsLabs/llm_os.git
cd llm_os
cargo build --release
```

Prerequisites: Rust >= 1.75, a GGUF model (default: Qwen 2.5 3B Q4_K_M) or Ollama.

Optional: llama.cpp vendored at `runtime/vendor/llama.cpp` for in-process inference (feature `ffi-inference`).

## Use

### Ollama (quickest path — no llama.cpp build required)

```bash
# Install Ollama (https://ollama.com), then:
ollama pull qwen3.5:2b

cd llm_os/runtime && cargo build --release && cd ..

# Single-call demo
RUST_LOG=info ./runtime/target/release/iod \
  --ollama --model qwen3.5:2b \
  --grammar grammar/isa.gbnf --cart cart \
  --goal "echo hello world" --budget 120 --max-predict 256

# Multi-step demo (proves stop-and-inject loop)
RUST_LOG=info ./runtime/target/release/iod \
  --ollama --model qwen3.5:2b \
  --grammar grammar/isa.gbnf --cart cart \
  --goal "Echo 'LLM-OS' using demo.echo, then hash it using demo.hash. Write the results and halt." \
  --budget 300 --max-predict 256
```

The Ollama backend does not enforce GBNF grammar at the sampler level — the model follows the ISA format through instruction-following alone. The daemon compensates with segment truncation, hallucination stripping, and repeat-call detection. See [docs/USAGE.md](docs/USAGE.md) for details.

### llama-server (full GBNF grammar enforcement)

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

The LLM generates a token stream following 14 opcodes. The I/O daemon (`iod`) drives inference, intercepts syscall opcodes, validates arguments against JSON schemas, executes cartridge handlers, and injects results back into the prompt. Four inference backends are supported:

| Backend | Grammar | Stop sequences | Best for |
|---|---|---|---|
| **Ollama** (`--ollama`) | Instruction-following only | Daemon-side truncation | Quick start, any Ollama model |
| **llama-server** (legacy) | GBNF at sampler level | SSE streaming stops | Full enforcement, dev |
| **FFI** (v1.0, feature-gated) | GBNF in-process | Zero-latency | Production, Pi 5 |
| **wllama** (browser) | JS token-trie (logit filtering) | Trie completion | Browser demo, zero-install |

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

Seven cartridges mounted: `game/tetris`, `system/summarize`, `system/demo`, `io/roclaw`, `sim/sim_world`, `domestic/cooking`, `domestic/residential-electrical`.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  iod (single binary)                                                │
│    ├── iod.rs           (dispatch loop + inference backends)          │
│    ├── dispatch.rs      (opcode → handler routing)                   │
│    ├── parser.rs        (14-opcode ISA state machine)                │
│    ├── cartridge.rs     (manifest + schema validation + arg hints)   │
│    ├── sampler.rs       (in-process grammar stack — v1.0 FFI)        │
│    ├── kv_pager.rs      (deterministic eviction with anchors)        │
│    ├── wasm_host.rs     (wasmtime cartridge sandbox)                 │
│    └── capability.rs    (logit bias + daemon-side reject)            │
│                                                                       │
│  ┌────────────────────────────────────────────────────────────┐       │
│  │  Backends (one active per session):                        │       │
│  │    Ollama      → /api/generate  (no grammar, easiest)      │       │
│  │    llama-server → /v1/completions (GBNF grammar, SSE)      │       │
│  │    libllama.a   → FFI in-process (feature-gated, v1.0)     │       │
│  │    wllama      → WebAssembly + JS token-trie (browser)     │       │
│  └────────────────────────────────────────────────────────────┘       │
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
| **Qwen 3.5 2B** (Ollama) | ~2.7 GB | ~5 tok/s | **Tested end-to-end with Ollama backend** |
| Qwen 2.5 3B Q4_K_M | ~1.7 GB | ~8 tok/s | Release sweet spot (llama-server) |
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
