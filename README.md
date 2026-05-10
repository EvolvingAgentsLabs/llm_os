# LLM-OS

An operating system where the LLM **is** the CPU. Part of the [Evolving Agents](https://github.com/EvolvingAgentsLabs) ecosystem.

## Versions

### [v1/](v1/) — Browser-side selector model

The LLM is a **decoder + selector**. A 350M-parameter model picks from pre-computed options via grammar-constrained ISA opcodes. Runs entirely in the browser via WebAssembly.

- Model: LiquidAI LFM 2.5 350M
- Runtime: wllama (browser WebAssembly)
- Grammar: client-side token trie
- Program layer: required (compileResult, bestActions)
- ISA: 2 of 14 opcodes implemented (call, halt)
- Demo: Tetris + Scavenger in-browser

### [v2/](v2/) — Server-side programmer model

The LLM is the **programmer**. A 31B model reasons from raw state and writes bytecode programs. No Program layer — the model does the planning.

- Model: any model via OpenRouter (default: Gemma 4 31B)
- Runtime: Node.js + OpenRouter HTTP API
- Grammar: server-side constraints
- ISA: all 14 opcodes implemented
- Cartridges: parameterized with JSON schema args
- I/O: typed ports (keyboard, screen) for microcomputer-style closed-loop execution

```bash
cd v2
export OPENROUTER_API_KEY=sk-or-v1-...
node bin/llm-os.js --cart cart/game/tetris --task "Play tetris" --verbose
```

### [v3/](v3/) — Dataset generation for fine-tuning

Uses a fast, cheap model (Gemini 3.1 Flash Lite) to generate ISA execution traces, then exports them as JSONL training data to fine-tune a smaller model for v2.

- Model: Gemini 3.1 Flash Lite via OpenRouter (fast + cheap)
- Runtime: Node.js + OpenRouter HTTP API
- Export: JSONL training data (full-trace or step-level)
- Purpose: generate training data to fine-tune Gemma 4 E4B for v2

```bash
cd v3
export OPENROUTER_API_KEY=sk-or-v1-...
node bin/llm-os.js --cart cart/game/tetris \
  --task "Play tetris. Observe, think, position, drop." \
  --runs 10 --export-dataset dataset/tetris.jsonl
```

## Fine-tuning pipeline

The end-to-end pipeline: v3 generates traces with a fast model, then a Colab notebook fine-tunes a small model to run locally in v2.

```
v3 (Gemini Flash Lite)          Colab (Unsloth)              v2 (local)
─────────────────────    ──────────────────────────    ──────────────────
 Generate traces    ──>  Fine-tune Gemma 4 E4B    ──>  Run fine-tuned
 --runs 100              QLoRA on free T4 GPU          model via GGUF
 --export-steps          Export to GGUF                 (llama.cpp/Ollama)
 ~$1.50, ~25 min         ~1500-2000 examples
```

- **Model**: Gemma 4 E4B (4B params, fits Colab free T4 at ~10GB VRAM)
- **Method**: QLoRA via [Unsloth](https://unsloth.ai) (1.5x faster, 60% less VRAM)
- **Dataset**: step-level JSONL from v3 (one opcode prediction per example)
- **Export**: GGUF Q4_K_M (~3-4GB) for local inference
- **Notebook**: [v3/notebook/finetune_gemma4_e4b.ipynb](v3/notebook/finetune_gemma4_e4b.ipynb)

## License

Apache 2.0. See [v1/LICENSE](v1/LICENSE).
