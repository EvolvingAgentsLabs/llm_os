# LLM-OS Ollama Demo — Status & Next Steps

## What Works (as of 2026-05-04)

### Single-call tasks — FULLY WORKING
```bash
RUST_LOG=info ./runtime/target/release/iod \
  --ollama --model qwen3.5:2b \
  --grammar grammar/isa.gbnf --cart cart \
  --goal "echo hello world" --budget 120 --max-predict 256
```
Result: Think → Call demo.echo → Write → Halt (success, 4 steps, ~60s)

### Multi-step with real daemon feedback — FULLY WORKING
```bash
RUST_LOG=info ./runtime/target/release/iod \
  --ollama --model qwen3.5:2b \
  --grammar grammar/isa.gbnf --cart cart \
  --goal "Echo 'LLM-OS' using demo.echo, then hash it using demo.hash. Write the results and halt." \
  --budget 300 --max-predict 256
```
Result: Call echo → (real result injected) → Call hash → Write → Halt (success, 4 steps, ~2min)

### Cooking cartridge — ARG NAMES CORRECT, but model loops
- Model correctly uses `household_size`, `dietary_notes` from schema hints
- Repeat-call detection kicks in after 3 identical calls and nudges the model
- Model sometimes self-corrects (e.g. fixed `locale="en"` → `"en_US"`)
- Still tends to loop instead of halting after single-call tasks

## Architecture Implemented

### Ollama Workarounds (Ollama lacks GBNF grammar + reliable stop sequences)

1. **`truncate_at_first_call()`** — Cuts each segment at the first `<|/call|>`, forcing per-call daemon dispatch. Without this, the model generates ALL steps in one shot and never sees real results.

2. **`strip_daemon_blocks()`** — Removes model-hallucinated `<|result|>...<|/result|>` and `<|ack|>` blocks. Truncates after `<|halt|>status=X`.

3. **Unclosed `<|think|>` repair** — Model often merges think+call without closing `<|/think|>`. Detected by counting opens vs closes, inserted automatically.

4. **Repeat-call detection** — Tracks call signatures across statements (including across Think blocks). After 3 identical calls, injects an error nudge.

5. **`think: false`** — Qwen 3.5 has CoT thinking enabled by default. Without this, model spends all tokens on internal reasoning and produces empty `response` field.

6. **`repeat_penalty: 1.3`** — Helps prevent token-level repetition within a single generation.

7. **`--max-predict` CLI flag** — Default 128 for Ollama (vs 512 for llama-server). Lower values give the daemon more frequent checkpoints.

8. **Boot prompt with schema arg hints** — `Cartridge.args_hint()` extracts required/optional field names from JSON schemas so the model knows exact arg names.

## Known Issues

### P0 — Multi-step tasks with iteration (sim_world navigation)
- **Problem**: Each segment takes 60-90s on CPU. A 3-step navigation needs 6+ segments (step + observe each time) = 6-9 minutes minimum. With growing prompt, later segments take even longer.
- **Root cause**: Ollama on CPU is slow, and each new request re-evaluates the full prompt.
- **Fix options**:
  - **llama-server with GBNF** — Extract GGUF from `~/.ollama/models/blobs/sha256-b709d...` and run with `llama-server`. Has proper GBNF grammar enforcement AND reliable SSE stop sequences. The existing llama-server code path (`complete_via_llama_server()`) already supports this.
  - **Ollama context caching** — Use `keep_alive: -1` to prevent model unloading. Prompt prefix caching should make subsequent segments faster.
  - **GPU acceleration** — Even a modest GPU would 10x the token generation speed.

### P1 — Model loops instead of halting after single-call tasks
- **Problem**: After getting a result from cooking.plan_menu, model calls it again instead of write+halt.
- **Mitigation**: Repeat-call detection + nudge works but adds 2 wasted segments.
- **Fix options**:
  - Fine-tune on ISA traces (the trace JSONL output is already generated)
  - Try larger models (4B, 7B) which follow instructions better
  - Improve boot prompt with more explicit termination patterns

### P2 — Pre-existing test failures (9 of 109)
- `tool_parser::tests::*` (7 failures) — Regex uses look-ahead not supported by `regex` crate. Pre-existing.
- `dispatch::tests::sim_world_*` (2 failures) — Static state isolation issue between tests. Pre-existing.
- **None of these are from the Ollama changes.**

## Immediate Next Steps (priority order)

### 1. Extract GGUF and run via llama-server
The GGUF is already cached by Ollama:
```bash
# The model blob (2.74GB):
~/.ollama/models/blobs/sha256-b709d81508a078a686961de6ca07a953b895d9b286c46e17f00fb267f4f2d297

# Symlink or copy to a known location:
ln -s ~/.ollama/models/blobs/sha256-b709d81508a078a686961de6ca07a953b895d9b286c46e17f00fb267f4f2d297 \
  models/qwen3.5-2b.gguf

# Run with llama-server (if installed):
llama-server -m models/qwen3.5-2b.gguf --port 8080

# Or use dev.sh:
bash scripts/dev.sh --model models/qwen3.5-2b.gguf
```
This would give GBNF grammar enforcement + reliable stop sequences, which solves both the hallucination and looping problems.

### 2. Generate fine-tune dataset from successful traces
The `--trace` flag already outputs JSONL. Run successful demos to accumulate traces:
```bash
./runtime/target/release/iod --ollama --model qwen3.5:2b \
  --grammar grammar/isa.gbnf --cart cart \
  --goal "echo hello" --budget 120 --max-predict 256 \
  --trace traces/finetune.jsonl
```
Repeat with different goals to build a dataset for LoRA fine-tuning.

### 3. Implement Ollama `/api/chat` endpoint
The `/api/chat` endpoint may handle stop sequences differently. Worth testing as an alternative to `/api/generate`.

### 4. Wire up sim_world for a compelling navigation demo
Once llama-server or a faster model is available, the sim_world cartridge should work for multi-step navigation with proper stop-and-inject.

### 5. Fix pre-existing test failures
- Replace regex look-ahead in tool_parser with compatible patterns
- Fix sim_world test state isolation (use per-test grid instance)

## Files Modified

| File | Changes |
|------|---------|
| `runtime/iod.rs` | Boot prompt, strip_daemon_blocks, truncate_at_first_call, think closure repair, repeat detection, OllamaOptions, HTTP timeout |
| `runtime/iod_main.rs` | `--max-predict` CLI flag, Ollama-aware defaults |
| `runtime/parser.rs` | Tolerant `parse_call()` — defaults to `{}` for missing args |
| `runtime/cartridge.rs` | `raw_schemas` storage, `args_hint()` method |

## Demo Commands (copy-paste ready)

```bash
# Build
cd llm_os/runtime && cargo build --release

# Single call (always works)
cd llm_os && RUST_LOG=info ./runtime/target/release/iod \
  --ollama --model qwen3.5:2b \
  --grammar grammar/isa.gbnf --cart cart \
  --goal "echo hello world" --budget 120 --max-predict 256

# Multi-step (echo + hash, proves stop-and-inject)
cd llm_os && RUST_LOG=info ./runtime/target/release/iod \
  --ollama --model qwen3.5:2b \
  --grammar grammar/isa.gbnf --cart cart \
  --goal "Echo 'LLM-OS' using demo.echo, then hash it using demo.hash. Write the results and halt." \
  --budget 300 --max-predict 256

# Debug mode (see all segment processing)
RUST_LOG=debug ./runtime/target/release/iod ...
```
