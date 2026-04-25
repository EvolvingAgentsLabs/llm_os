# Kernel fine-tune recipe (v0.1 → v0.1 final)

*Target: a 2B-class Gemma 4 E2B (or Qwen 3 2B) post-trained on LLM-OS execution traces, with the 18 ISA special tokens added to the tokenizer. Expected gain over the v0.01 bootstrap kernel: **5–10× syscall throughput on Pi 5** (refinement §6 + design §6.2). This document is the recipe; the actual training runs on the user's GPU host.*

---

## 0. Prerequisites

- A GPU with ≥ 16 GB VRAM (24 GB strongly preferred for 2B-class LoRA training; 80 GB for full-parameter).
- Python 3.11+ with: `transformers`, `gguf`, `peft`, `trl`, `datasets`, `accelerate`, `bitsandbytes`.
- `llama.cpp` built with `convert_hf_to_gguf.py` and `llama-quantize` available.
- A trace dataset of ≥ 5 000 valid v0.01 execution traces. Use the daemon's `--trace` flag (Step 1 below) to collect them.

---

## 1. Collect traces

Run the v0.01 daemon with the `--trace <path>` flag. Each task emits one JSONL line containing:

```json
{
  "goal":   "<user goal>",
  "prompt": "<full final prompt incl. injected results>",
  "stream": "<raw model emission stream>",
  "status": "success|partial|failure",
  "steps":  N,
  "wall_seconds": F,
  "cartridges": ["roclaw", "sim_world", ...]
}
```

Aim for 5 000+ traces with `status == "success"` across ≥ 3 cartridges. Quick path to bootstrap data:

```bash
# Smoke fixture for 1000 successful sim_world runs:
for i in $(seq 1 1000); do
  ./runtime/target/release/iod \
    --server http://127.0.0.1:8080 \
    --grammar grammar/isa.gbnf \
    --cart cart \
    --goal "Reach (3,0). Use sim_world.observe + sim_world.step." \
    --trace traces/sim_world.jsonl \
    --budget 60
done
```

For broader coverage, generate synthetic traces by running the v0.01 daemon against task fixtures in `tests/fixtures/` (v0.5 will ship a fixture set; for v0.1 prep, hand-author 50 task templates).

---

## 2. Patch the tokenizer

Add the 18 ISA special tokens to the HF tokenizer **before** GGUF conversion, so the embedding matrix can be resized in the same pass.

```python
# scripts/patch_tokenizer.py — run on a checked-out HF model dir.
from transformers import AutoModelForCausalLM, AutoTokenizer

MODEL = "google/gemma-4-e2b"  # or "Qwen/Qwen3-2B-Instruct"
ISA_TOKENS = [
    "<|read|>", "<|write|>", "<|call|>", "<|/call|>",
    "<|yield|>", "<|fork|>", "<|wait|>", "<|loop|>",
    "<|break|>", "<|halt|>", "<|think|>", "<|/think|>",
    "<|commit|>", "<|fault|>", "<|policy|>",
    "<|result|>", "<|/result|>", "<|ack|>",
]

tok = AutoTokenizer.from_pretrained(MODEL)
n_added = tok.add_special_tokens({"additional_special_tokens": ISA_TOKENS})
print(f"added {n_added} tokens (vocab is now {len(tok)})")

model = AutoModelForCausalLM.from_pretrained(MODEL, torch_dtype="bfloat16")
model.resize_token_embeddings(len(tok))

# Initialize new embeddings as the mean of the existing ones — better
# starting point than random for fine-tune convergence.
with model.no_grad() if hasattr(model, "no_grad") else __import__("torch").no_grad():
    embed = model.get_input_embeddings()
    new_n = n_added
    if new_n > 0:
        old_mean = embed.weight[:-new_n].mean(dim=0, keepdim=True)
        embed.weight[-new_n:] = old_mean

tok.save_pretrained("./out/gemma-4-e2b-isa")
model.save_pretrained("./out/gemma-4-e2b-isa")
```

Then convert + quantize:

```bash
python3 ~/llama.cpp/convert_hf_to_gguf.py ./out/gemma-4-e2b-isa --outfile ./out/gemma-4-e2b-isa.gguf
~/llama.cpp/build/bin/llama-quantize ./out/gemma-4-e2b-isa.gguf ./out/gemma-4-e2b-isa-q4.gguf Q4_K_M
```

Smoke-test the new tokenizer:

```bash
~/llama.cpp/build/bin/llama-tokenize -m ./out/gemma-4-e2b-isa-q4.gguf -p "<|read|>" --no-bos --log-disable --show-count --ids
# Expect: Total number of tokens: 1   (was 5 on the base — see tokenization-report.md)
```

---

## 3. Format training triples (DPO/GRPO)

The reward signal is `successful_syscall + task_completion + grammar_validity`. For DPO, use traces from the same goal where one ended in success and another in failure as the (chosen, rejected) pair. For GRPO, sample N rollouts per goal at training time and weight by reward.

```python
# scripts/build_dpo_pairs.py — sketch.
import json, itertools
traces = [json.loads(l) for l in open("traces/all.jsonl")]
by_goal = {}
for t in traces:
    by_goal.setdefault(t["goal"], []).append(t)
pairs = []
for goal, ts in by_goal.items():
    ok = [t for t in ts if t["status"] == "success"]
    bad = [t for t in ts if t["status"] != "success"]
    for c, r in itertools.product(ok, bad):
        pairs.append({
            "prompt":   f"Goal: {goal}\n",
            "chosen":   c["stream"],
            "rejected": r["stream"],
        })
with open("traces/dpo.jsonl", "w") as f:
    for p in pairs:
        f.write(json.dumps(p) + "\n")
print(f"emitted {len(pairs)} DPO pairs")
```

---

## 4. LoRA fine-tune (low-VRAM path)

Use `trl`'s `DPOTrainer` with a LoRA adapter on the embedding + last 4 transformer blocks. Hyperparams that have worked in similar 2B-class ISA fine-tunes:

| Param | Value | Rationale |
|---|---|---|
| `lora_r` | 64 | Embedding-heavy task (new tokens); rank 64 is the inflection point |
| `lora_alpha` | 128 | 2× rank, standard |
| `learning_rate` | 1e-5 | DPO is sensitive; higher diverges fast |
| `beta` (DPO) | 0.1 | Standard |
| `epochs` | 3 | More overfits to the trace style |
| `batch_size` | 4 with gradient_accumulation_steps=8 | Effective batch 32 |
| `max_length` | 2048 | Enough for one full task |
| `warmup_ratio` | 0.05 | |
| `target_modules` | embed_tokens, q_proj, v_proj, lm_head | Embed for new tokens; q/v for grammar adherence; lm_head for output bias |

```bash
accelerate launch scripts/dpo_train.py \
    --model_name_or_path ./out/gemma-4-e2b-isa \
    --dataset_path traces/dpo.jsonl \
    --output_dir ./out/gemma-4-e2b-isa-lora-dpo \
    --beta 0.1 --learning_rate 1e-5 --num_train_epochs 3
```

Merge LoRA back, export GGUF, quantize:

```bash
python3 scripts/merge_lora.py \
    --base ./out/gemma-4-e2b-isa \
    --lora ./out/gemma-4-e2b-isa-lora-dpo \
    --out  ./out/gemma-4-e2b-isa-merged

python3 ~/llama.cpp/convert_hf_to_gguf.py ./out/gemma-4-e2b-isa-merged --outfile ./out/kernel-v0.1.gguf
~/llama.cpp/build/bin/llama-quantize ./out/kernel-v0.1.gguf ./out/kernel-v0.1-q4.gguf Q4_K_M
```

---

## 5. Acceptance gate

Before tagging v0.1 final, the new kernel must:

1. Pass `bash tests/e2e/navigate_and_report.sh` with `LLM_OS_MODEL=./out/kernel-v0.1-q4.gguf` — same 10/10 success threshold as v0.01.
2. Show ≥ 3× wall-clock improvement on syscall-dense workloads vs the v0.01 bootstrap kernel.
3. Pass grammar validation (`bash scripts/validate_grammar.sh`) — the new tokens must not break the GBNF parsing path. (They shouldn't, since the grammar references opcode strings, but check.)
4. The tokenization report (regenerated via `bash scripts/check_tokenizer.sh` after pointing at the new GGUF) must show every opcode as a single token.

---

## 6. Re-baseline projections

After v0.1 acceptance passes, regenerate `docs/projection-rebaseline.md` per refinement §4 R7. The v0.5 self-hosting milestone depends on the small-on-device kernel tracking frontier-minus-one; if v0.1 measurements show frontier-minus-two, the v0.5 timeline slips and downstream consumers need the heads-up before they plan around it.

---

## 7. What this recipe doesn't yet do (deferred)

- **GRPO instead of DPO.** GRPO is tighter for reward-shaped tasks but more expensive. v0.1 ships DPO; v0.5 may switch.
- **RLHF on real user trajectories.** Requires the daemon to be deployed and collecting real-user traces; v0.01 has no users yet.
- **Multi-task curricula.** v0.1 trains on a flat trace mix; v0.5 schedules cartridges by difficulty.
