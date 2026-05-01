#!/usr/bin/env python3
"""
v1.0 — Full tokenizer rebuild + fine-tune pipeline for LLM-OS.

Adds 19 ISA special tokens to a HuggingFace model's tokenizer, resizes
the embedding matrix, and saves the result ready for LoRA fine-tuning.
After fine-tuning, the output can be converted to GGUF via llama.cpp's
convert_hf_to_gguf.py.

Usage:
    # Step 1: Patch tokenizer + resize embeddings
    python3 scripts/rebuild_tokenizer.py patch \
        --base Qwen/Qwen2.5-3B-Instruct \
        --out ./patched_model

    # Step 2: Validate single-token property
    python3 scripts/rebuild_tokenizer.py validate \
        --model ./patched_model

    # Step 3: Fine-tune with LoRA on ISA traces
    python3 scripts/rebuild_tokenizer.py finetune \
        --model ./patched_model \
        --dataset out/dpo.jsonl \
        --out ./finetuned_model

    # Step 4: Export to GGUF
    python3 scripts/rebuild_tokenizer.py export \
        --model ./finetuned_model \
        --out ~/models/qwen-2.5-3b-llmos-v1.gguf \
        --quant Q4_K_M

Requires:
    pip install transformers torch peft trl datasets accelerate

The GGUF export step requires llama.cpp's convert_hf_to_gguf.py and
llama-quantize binary.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# The 19 ISA tokens (18 original + <|state|> added in v0.5).
# All must be single tokens that cannot be further BPE-merged.
ISA_SPECIAL_TOKENS = [
    "<|read|>",
    "<|write|>",
    "<|call|>",
    "<|/call|>",
    "<|yield|>",
    "<|fork|>",
    "<|wait|>",
    "<|loop|>",
    "<|break|>",
    "<|halt|>",
    "<|think|>",
    "<|/think|>",
    "<|commit|>",
    "<|fault|>",
    "<|policy|>",
    "<|state|>",
    "<|/state|>",
    "<|result|>",
    "<|/result|>",
    "<|ack|>",
]


def cmd_patch(args) -> int:
    """Patch tokenizer: add ISA tokens as special tokens + resize embeddings."""
    try:
        from transformers import AutoTokenizer, AutoModelForCausalLM
        from tokenizers import AddedToken
    except ImportError:
        print("error: install transformers + tokenizers: pip install transformers", file=sys.stderr)
        return 2

    print(f"[patch] Loading base model tokenizer: {args.base}")
    tokenizer = AutoTokenizer.from_pretrained(args.base, trust_remote_code=True)

    # Check which tokens already exist
    existing_special = set(tokenizer.all_special_tokens)
    to_add = []
    for t in ISA_SPECIAL_TOKENS:
        if t in existing_special:
            print(f"  already present: {t}")
        else:
            to_add.append(t)

    if not to_add:
        print("[patch] All ISA tokens already present. Nothing to do.")
    else:
        print(f"[patch] Adding {len(to_add)} ISA tokens as special tokens:")
        for t in to_add:
            print(f"  + {t}")

        # Add as special tokens with single_word=True to prevent BPE merging
        added_tokens = [
            AddedToken(t, single_word=True, normalized=False, special=True)
            for t in to_add
        ]
        tokenizer.add_special_tokens({"additional_special_tokens": added_tokens})

    # Validate single-token property
    print("[patch] Validating single-token property...")
    failures = []
    for t in ISA_SPECIAL_TOKENS:
        ids = tokenizer.encode(t, add_special_tokens=False)
        if len(ids) != 1:
            failures.append((t, len(ids), ids))
            print(f"  FAIL: {t} → {len(ids)} tokens: {ids}")
        else:
            print(f"  OK: {t} → token_id={ids[0]}")

    if failures:
        print(f"\n[patch] WARNING: {len(failures)} tokens did not achieve single-token encoding.")
        print("  This may indicate the tokenizer backend doesn't support AddedToken(special=True).")
        print("  The fine-tune will still work but latency won't be uniform.")

    # Save tokenizer
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    tokenizer.save_pretrained(str(out_dir))
    print(f"[patch] Tokenizer saved to {out_dir}")

    # Load model and resize embeddings if requested
    if not args.tokenizer_only:
        print(f"[patch] Loading model: {args.base} (this may take a while)...")
        model = AutoModelForCausalLM.from_pretrained(
            args.base,
            trust_remote_code=True,
            torch_dtype="auto",
        )
        if len(to_add) > 0:
            print(f"[patch] Resizing token embeddings: {model.config.vocab_size} → {len(tokenizer)}")
            model.resize_token_embeddings(len(tokenizer))
        model.save_pretrained(str(out_dir))
        print(f"[patch] Model saved to {out_dir}")
    else:
        print("[patch] --tokenizer-only: skipping model load + resize")

    # Write token map for downstream tools
    token_map = {}
    for t in ISA_SPECIAL_TOKENS:
        ids = tokenizer.encode(t, add_special_tokens=False)
        token_map[t] = ids
    map_path = out_dir / "isa_token_map.json"
    map_path.write_text(json.dumps(token_map, indent=2))
    print(f"[patch] Token map written to {map_path}")

    return 0


def cmd_validate(args) -> int:
    """Validate that all ISA tokens are single-token in a model/tokenizer."""
    try:
        from transformers import AutoTokenizer
    except ImportError:
        print("error: install transformers: pip install transformers", file=sys.stderr)
        return 2

    print(f"[validate] Loading tokenizer from: {args.model}")
    tokenizer = AutoTokenizer.from_pretrained(args.model, trust_remote_code=True)

    failures = []
    print(f"\n{'Token':<18} {'IDs':<20} {'Count':<8} {'Status'}")
    print("-" * 60)
    for t in ISA_SPECIAL_TOKENS:
        ids = tokenizer.encode(t, add_special_tokens=False)
        count = len(ids)
        status = "OK" if count == 1 else "FAIL"
        print(f"{t:<18} {str(ids):<20} {count:<8} {status}")
        if count != 1:
            failures.append(t)

    print()
    if failures:
        print(f"[validate] FAILED: {len(failures)}/{len(ISA_SPECIAL_TOKENS)} tokens are multi-token")
        return 1
    else:
        print(f"[validate] PASSED: all {len(ISA_SPECIAL_TOKENS)} ISA tokens are single-token")
        return 0


def cmd_finetune(args) -> int:
    """Fine-tune with LoRA + DPO on ISA traces."""
    try:
        from transformers import AutoTokenizer, AutoModelForCausalLM, TrainingArguments
        from peft import LoraConfig, get_peft_model
        from trl import DPOTrainer, DPOConfig
        from datasets import load_dataset
    except ImportError as e:
        print(f"error: missing dependency: {e}", file=sys.stderr)
        print("  pip install transformers peft trl datasets accelerate", file=sys.stderr)
        return 2

    print(f"[finetune] Loading model: {args.model}")
    tokenizer = AutoTokenizer.from_pretrained(args.model, trust_remote_code=True)
    model = AutoModelForCausalLM.from_pretrained(
        args.model,
        trust_remote_code=True,
        torch_dtype="auto",
        device_map="auto",
    )

    # LoRA config optimized for ISA token learning
    lora_config = LoraConfig(
        r=args.lora_r,
        lora_alpha=args.lora_alpha,
        lora_dropout=0.05,
        target_modules=["q_proj", "k_proj", "v_proj", "o_proj",
                        "gate_proj", "up_proj", "down_proj"],
        bias="none",
        task_type="CAUSAL_LM",
    )

    model = get_peft_model(model, lora_config)
    model.print_trainable_parameters()

    # Load DPO dataset
    print(f"[finetune] Loading dataset: {args.dataset}")
    dataset = load_dataset("json", data_files=args.dataset, split="train")

    # DPO training config
    training_args = DPOConfig(
        output_dir=args.out,
        num_train_epochs=args.epochs,
        per_device_train_batch_size=args.batch_size,
        learning_rate=args.lr,
        max_length=args.max_seq_length,
        max_prompt_length=args.max_seq_length // 2,
        logging_steps=10,
        save_steps=500,
        save_total_limit=3,
        bf16=True,
        gradient_accumulation_steps=args.gradient_accumulation,
        warmup_ratio=0.1,
        lr_scheduler_type="cosine",
        remove_unused_columns=False,
    )

    # Train
    trainer = DPOTrainer(
        model=model,
        args=training_args,
        train_dataset=dataset,
        processing_class=tokenizer,
    )

    print("[finetune] Starting DPO training...")
    trainer.train()

    # Merge LoRA weights and save
    print("[finetune] Merging LoRA weights...")
    merged = model.merge_and_unload()
    merged.save_pretrained(args.out)
    tokenizer.save_pretrained(args.out)
    print(f"[finetune] Merged model saved to {args.out}")

    return 0


def cmd_export(args) -> int:
    """Export fine-tuned model to GGUF format."""
    import subprocess
    import shutil

    # Find llama.cpp convert script
    llama_dir = Path(args.llama_cpp) if args.llama_cpp else Path("../llama.cpp")
    convert_script = llama_dir / "convert_hf_to_gguf.py"
    quantize_bin = None

    for cand in [
        llama_dir / "build" / "bin" / "llama-quantize",
        llama_dir / "build" / "bin" / "Release" / "llama-quantize",
        shutil.which("llama-quantize") or "",
    ]:
        if cand and Path(cand).exists():
            quantize_bin = str(cand)
            break

    if not convert_script.exists():
        print(f"error: convert_hf_to_gguf.py not found at {convert_script}", file=sys.stderr)
        print("  Set --llama-cpp to your llama.cpp directory", file=sys.stderr)
        return 2

    # Step 1: Convert to f16 GGUF
    out_path = Path(args.out)
    f16_path = out_path.with_suffix(".f16.gguf")

    print(f"[export] Converting {args.model} → {f16_path}")
    result = subprocess.run(
        [sys.executable, str(convert_script), args.model,
         "--outfile", str(f16_path), "--outtype", "f16"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        print(f"error: convert failed:\n{result.stderr}", file=sys.stderr)
        return 1
    print(f"[export] f16 GGUF written: {f16_path}")

    # Step 2: Quantize
    if args.quant and args.quant != "f16":
        if not quantize_bin:
            print(f"error: llama-quantize not found. Build llama.cpp first.", file=sys.stderr)
            return 2

        print(f"[export] Quantizing to {args.quant}: {f16_path} → {out_path}")
        result = subprocess.run(
            [quantize_bin, str(f16_path), str(out_path), args.quant],
            capture_output=True, text=True,
        )
        if result.returncode != 0:
            print(f"error: quantize failed:\n{result.stderr}", file=sys.stderr)
            return 1

        # Remove intermediate f16
        f16_path.unlink(missing_ok=True)
        print(f"[export] Quantized GGUF written: {out_path}")
    else:
        # No quantization — rename f16 to final
        f16_path.rename(out_path)
        print(f"[export] GGUF written (f16): {out_path}")

    # Validate tokens in the GGUF
    print(f"[export] Run `bash scripts/check_tokenizer.sh --model {out_path}` to validate.")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="LLM-OS tokenizer rebuild + fine-tune pipeline",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    subparsers = parser.add_subparsers(dest="command")

    # patch
    p_patch = subparsers.add_parser("patch", help="Add ISA tokens + resize embeddings")
    p_patch.add_argument("--base", required=True, help="Base model (HF name or local path)")
    p_patch.add_argument("--out", required=True, help="Output directory for patched model")
    p_patch.add_argument("--tokenizer-only", action="store_true", help="Only save tokenizer, skip model")

    # validate
    p_val = subparsers.add_parser("validate", help="Validate single-token property")
    p_val.add_argument("--model", required=True, help="Model/tokenizer path to validate")

    # finetune
    p_ft = subparsers.add_parser("finetune", help="LoRA fine-tune on ISA traces")
    p_ft.add_argument("--model", required=True, help="Patched model directory")
    p_ft.add_argument("--dataset", required=True, help="DPO JSONL file from promote_traces.py")
    p_ft.add_argument("--out", required=True, help="Output directory for fine-tuned model")
    p_ft.add_argument("--epochs", type=int, default=3, help="Training epochs (default: 3)")
    p_ft.add_argument("--batch-size", type=int, default=4, help="Batch size (default: 4)")
    p_ft.add_argument("--lr", type=float, default=2e-4, help="Learning rate (default: 2e-4)")
    p_ft.add_argument("--lora-r", type=int, default=16, help="LoRA rank (default: 16)")
    p_ft.add_argument("--lora-alpha", type=int, default=32, help="LoRA alpha (default: 32)")
    p_ft.add_argument("--max-seq-length", type=int, default=4096, help="Max sequence length (default: 4096)")
    p_ft.add_argument("--gradient-accumulation", type=int, default=4, help="Gradient accumulation steps (default: 4)")

    # export
    p_exp = subparsers.add_parser("export", help="Export to GGUF")
    p_exp.add_argument("--model", required=True, help="Fine-tuned model directory")
    p_exp.add_argument("--out", required=True, help="Output GGUF path")
    p_exp.add_argument("--quant", default="Q4_K_M", help="Quantization type (default: Q4_K_M)")
    p_exp.add_argument("--llama-cpp", help="Path to llama.cpp directory")

    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        return 0

    commands = {
        "patch": cmd_patch,
        "validate": cmd_validate,
        "finetune": cmd_finetune,
        "export": cmd_export,
    }
    return commands[args.command](args)


if __name__ == "__main__":
    sys.exit(main())
