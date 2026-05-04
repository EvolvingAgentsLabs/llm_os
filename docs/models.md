# LLM-OS Model Matrix

Which model to use depends on your backend (Ollama vs llama-server), mode (dev vs release), and hardware.

## Model Recommendations

| Model | Params | Size | RAM needed | tok/s (laptop) | tok/s (Pi 5) | Use case |
|---|---|---|---|---|---|---|
| **Qwen2.5-0.5B-Instruct** | 0.5B | ~350 MB | 1 GB | ~50 | ~30 | Dev: fastest iteration (llama-server) |
| **Qwen2.5-1.5B-Instruct** | 1.5B | ~900 MB | 2 GB | ~25 | ~15 | Dev: good quality + speed (llama-server) |
| **Qwen 3.5 2B** | 2B | ~2.7 GB | 4 GB | ~5-10 | ~4 | **Ollama: tested end-to-end** |
| Qwen2.5-3B-Instruct | 3B | ~1.7 GB | 3 GB | ~15 | **~8** | **Release: Pi 5 sweet spot** |
| Gemma-4-E2B | 3B | ~1.8 GB | 3 GB | ~14 | ~8 | Release: alternative to Qwen 3B |
| Qwen3-4B | 4B | ~2.5 GB | 4 GB | ~12 | ~5 | Release: best quality that fits |
| Qwen2.5-7B-Instruct | 7B | ~4.1 GB | 6 GB | ~8 | ~2-3 | Release: Pi 5 ceiling (tight) |

## Ollama Models (no GBNF, instruction-following only)

**Recommendation: Qwen 3.5 2B** — tested and working end-to-end with the Ollama backend.

```bash
ollama pull qwen3.5:2b
```

Why Qwen 3.5 works for ISA generation without grammar:

1. **Strong instruction following** — the 3.5 series follows structured output formats well, even at 2B parameters.
2. **256K context** — large enough for boot prompt + multi-step traces.
3. **`think: false` support** — disables internal CoT reasoning that otherwise produces empty responses.

**Caveats at 2B:**
- CPU inference is slow (~5-10 tok/s on laptop, ~30-60s per generation segment)
- Model sometimes loops on repeated calls (mitigated by daemon-side detection)
- Schema arg names need explicit hints in the boot prompt (provided automatically)
- Multi-step iterative tasks (3+ calls) may hit wall-clock budget on CPU

**Other Ollama models that should work** (untested):
- `qwen3.5:4b` — better instruction following, slower
- `qwen3.5:8b` — best quality, needs GPU for reasonable speed
- `llama3.2:3b` — Meta's small model, good at structured output

## Dev Mode: Smallest Viable Model

**Recommendation: Qwen2.5-0.5B-Instruct Q4_K_M** (~350 MB)

Why it works for dev despite being tiny:

1. **Grammar enforcement is at the sampler level.** The GBNF grammar prevents
   invalid ISA sequences regardless of model quality. A 0.5B model constrained
   by `isa.gbnf` will always emit syntactically valid opcodes.

2. **Dev tasks are simple.** You're testing the pipeline (bootloader → server →
   iod → parser → dispatch → cartridge → inject result → halt), not asking the
   model to reason about complex goals.

3. **Speed matters more than quality in dev.** At ~50 tok/s on a modern laptop,
   the edit-run-debug cycle is near-instant. A 7B model at 8 tok/s slows you
   down for no benefit during development.

4. **Cloud fallback covers quality gaps.** Use `--cloud-url` with Gemini or
   Claude for goals that require actual reasoning. The 0.5B handles the ISA
   mechanics; the cloud handles the hard thinking.

**Quality caveats at 0.5B:**
- Tool-call argument generation may be low quality (wrong JSON values)
- Loop/break decisions may be suboptimal (unnecessary iterations)
- Goal decomposition is poor (can't plan multi-step tasks well)
- These don't matter for dev — you're testing the runtime, not the model

**Download:**
```bash
# Via huggingface-cli
pip install huggingface-hub
huggingface-cli download Qwen/Qwen2.5-0.5B-Instruct-GGUF \
    qwen2.5-0.5b-instruct-q4_k_m.gguf \
    --local-dir ~/models

# Or direct URL
curl -L -o ~/models/qwen2.5-0.5b-instruct-q4_k_m.gguf \
    "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf"
```

## Release Mode: Largest Viable Model for Pi 5

**Recommendation: Qwen2.5-3B-Instruct Q4_K_M** (~1.7 GB)

This is the sweet spot for Pi 5 8GB:

| Resource | Budget | Used by 3B Q4 | Remaining |
|---|---|---|---|
| RAM (8 GB) | 8192 MB | ~1700 MB (weights) | 6492 MB |
| KV cache (ctx=32768) | — | ~2048 MB | 4444 MB |
| OS + llama-server binary | — | ~50 MB | 4394 MB |
| Headroom (safety) | — | ~500 MB | 3894 MB |

Performance: **~8 tokens/second** steady-state on Pi 5 Cortex-A76 (NEON).
This matches the existing `8.2 Hz` benchmark in the README.

**Maximum viable: Qwen2.5-7B Q4_K_M** (~4.1 GB)

Technically loads on 8GB Pi 5, but:
- KV cache must be reduced (`--ctx-size 8192` instead of 32768)
- ~2-3 tokens/second — noticeably slow for interactive use
- RAM headroom is tight (~1 GB free after model + KV)
- Swap thrashing risk on sustained multi-turn tasks

Use 7B only if task quality matters more than latency (e.g., complex
planning that runs unattended overnight).

**Future: Pi 5 + AI HAT+ 2**

The Raspberry Pi AI HAT+ 2 adds 8GB LPDDR4X + Hailo 10H (40 TOPS INT8).
However, as of 2026-04, the Hailo NPU is **slower than the Pi 5 CPU for
autoregressive LLM inference** (benchmarked by Jeff Geerling). The extra
RAM helps, but the NPU doesn't accelerate the token-by-token sampling
loop that llama.cpp uses. Not recommended until Hailo supports KV-cache
attention offload.

## Quantization Guide

| Quantization | Quality | Size (3B) | Speed | When to use |
|---|---|---|---|---|
| Q8_0 | Best | ~3.2 GB | Slowest | Debugging quality issues |
| Q5_K_M | Very good | ~2.2 GB | Medium | Quality-sensitive release |
| **Q4_K_M** | **Good** | **~1.7 GB** | **Fast** | **Default for dev + release** |
| Q3_K_M | Acceptable | ~1.4 GB | Faster | Tight RAM (Pi 5 + 7B) |
| Q2_K | Poor | ~1.1 GB | Fastest | Not recommended (artifacts) |

**Always use Q4_K_M unless you have a specific reason not to.** It's the
best tradeoff between quality and size, and it's what llm_os is tested against.

## Download Commands

### Ollama (easiest)

```bash
# Tested and working
ollama pull qwen3.5:2b

# Larger alternatives (untested with ISA)
ollama pull qwen3.5:4b
ollama pull qwen3.5:8b
```

To extract the GGUF from Ollama for use with llama-server:

```bash
# Find the model blob
cat ~/.ollama/models/manifests/registry.ollama.ai/library/qwen3.5/2b \
  | python3 -c "import sys,json; layers=json.load(sys.stdin)['layers']; print([l['digest'] for l in layers if 'model' in l['mediaType']][0])"

# Symlink to a known path (replace sha256-... with the actual digest)
ln -s ~/.ollama/models/blobs/sha256-b709d815... ~/models/qwen3.5-2b.gguf
```

### HuggingFace (GGUF for llama-server)

```bash
mkdir -p ~/models

# Dev (0.5B — fastest iteration)
huggingface-cli download Qwen/Qwen2.5-0.5B-Instruct-GGUF \
    qwen2.5-0.5b-instruct-q4_k_m.gguf --local-dir ~/models

# Dev (1.5B — better quality)
huggingface-cli download Qwen/Qwen2.5-1.5B-Instruct-GGUF \
    qwen2.5-1.5b-instruct-q4_k_m.gguf --local-dir ~/models

# Release: Pi 5 sweet spot (3B)
huggingface-cli download Qwen/Qwen2.5-3B-Instruct-GGUF \
    qwen2.5-3b-instruct-q4_k_m.gguf --local-dir ~/models

# Release: Pi 5 ceiling (7B — tight fit)
huggingface-cli download Qwen/Qwen2.5-7B-Instruct-GGUF \
    qwen2.5-7b-instruct-q4_k_m.gguf --local-dir ~/models
```
