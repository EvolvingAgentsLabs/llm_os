# LLM-OS v3

Dataset generation for fine-tuning. Uses a fast, cheap model (Gemini 3.1 Flash Lite via OpenRouter) to generate ISA execution traces, then exports them as JSONL training data to fine-tune a smaller model (e.g. Gemma 4 E4B) for v2.

## Why v3?

v2 uses Gemma 4 31B to play Tetris — it works but the model only drops pieces at center (no rotation/positioning). To get a smaller model to play well, we need training data: thousands of correct opcode traces showing observe → think → rotate → move → drop sequences.

v3 generates that data:
1. Run a fast model (Gemini 3.1 Flash Lite) against cartridge engines
2. Export every execution as JSONL training examples
3. Fine-tune Gemma 4 E4B on the traces
4. Deploy the fine-tuned model in v2 (or in-browser via wllama)

## Quick start

```bash
export OPENROUTER_API_KEY=sk-or-v1-...
cd v3

# Single run (see it play)
node bin/llm-os.js --cart cart/game/tetris \
  --task "Play tetris. Observe, think, position pieces, drop." \
  --verbose

# Generate dataset: 10 runs -> JSONL
node bin/llm-os.js --cart cart/game/tetris \
  --task "Play tetris. Observe, think about placement, rotate and position, drop." \
  --runs 10 --export-dataset dataset/tetris_traces.jsonl

# Step-level dataset (one example per opcode)
node bin/llm-os.js --cart cart/game/tetris \
  --task "Play tetris." \
  --runs 5 --export-dataset dataset/tetris_steps.jsonl --export-steps
```

## Dataset format

Each line in the JSONL file is a training example in OpenAI chat fine-tune format:

```json
{
  "messages": [
    {"role": "system", "content": "You are the LLM-OS kernel..."},
    {"role": "user", "content": "Play tetris..."},
    {"role": "assistant", "content": "<|call|>tetris.observe {}<|/call|>"},
    {"role": "user", "content": "<|result|>{...board state...}<|/result|>"},
    {"role": "assistant", "content": "<|call|>tetris.move {\"action\":\"rotate\"}<|/call|>"}
  ],
  "metadata": {
    "status": "failure",
    "steps": 26,
    "cartridges": ["tetris"]
  }
}
```

### Export modes

- **Full trace** (default): One example per run. Good for teaching the full execution pattern.
- **Step-level** (`--export-steps`): One example per opcode. Good for per-opcode prediction.

## Fine-tuning pipeline

```bash
# 1. Generate traces
node bin/llm-os.js --cart cart/game/tetris \
  --task "Play tetris. Observe, rotate, position, drop." \
  --runs 50 --export-dataset dataset/tetris_50runs.jsonl

# 2. Fine-tune with Unsloth
# python finetune.py --base google/gemma-4-E4B-it \
#   --data dataset/tetris_50runs.jsonl

# 3. Export to GGUF and deploy in v2 or v1
```
