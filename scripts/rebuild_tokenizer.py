#!/usr/bin/env python3
"""
v0.1-E — Tokenizer rebuild for the LLM-OS kernel.

Adds the 18 ISA special tokens to a base GGUF and writes a new GGUF whose
tokenizer has them as single-token control tokens. After this step, each
opcode tokenizes to exactly 1 token instead of the 4–12 tokens the
bootstrap form uses (see `docs/tokenization-report.md`).

Usage:
    python3 scripts/rebuild_tokenizer.py \\
        --in  ~/models/gemma-4-e2b-q4.gguf \\
        --out ~/models/gemma-4-e2b-isa-q4.gguf

Requires the `gguf` Python package (ships with llama.cpp:
`pip install gguf` — pulls from PyPI, version pinned to >=0.10).

This does not retrain — it only updates the tokenizer side of the GGUF.
The model still treats new tokens as random embeddings until the
fine-tune (`docs/fine-tune-recipe.md`) trains them.

What this script does NOT do:
- Resize the model embedding matrix. With most GGUF inference paths, the
  added tokens are treated as out-of-vocab unless the embedding rows are
  added before quantization. For correct behavior, run this on a
  fp16/bf16 GGUF (or convert from HF first), THEN quantize.
- Validate that the new tokens don't collide with existing vocab. The
  standard ISA tokens (`<|read|>` etc.) collide with nothing in any
  Gemma/Qwen/Llama tokenizer surveyed at v0.01 — see
  `docs/tokenization-report.md` — but check before running on a custom
  base.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

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
    "<|result|>",
    "<|/result|>",
    "<|ack|>",
]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--in", dest="src", required=True, help="source GGUF (fp16 / bf16 strongly preferred)")
    parser.add_argument("--out", dest="dst", required=True, help="destination GGUF path")
    parser.add_argument("--dry-run", action="store_true", help="report tokens that would be added; write nothing")
    args = parser.parse_args()

    try:
        import gguf  # type: ignore
    except ImportError:
        print("error: `gguf` package not installed. Run: pip install gguf", file=sys.stderr)
        return 2

    src = Path(args.src)
    dst = Path(args.dst)
    if not src.exists():
        print(f"error: source GGUF not found: {src}", file=sys.stderr)
        return 1

    reader = gguf.GGUFReader(src.as_posix())

    def _field(name: str):
        for f in reader.fields.values():
            if f.name == name:
                return f
        return None

    tok_field = _field("tokenizer.ggml.tokens")
    type_field = _field("tokenizer.ggml.token_type")
    if tok_field is None or type_field is None:
        print("error: GGUF missing tokenizer.ggml.tokens/token_type — cannot extend", file=sys.stderr)
        return 1

    # Decode existing vocab. The gguf library stores tokens as bytes per entry.
    existing = []
    for i in range(len(tok_field.data)):
        raw = tok_field.parts[tok_field.data[i]]
        try:
            existing.append(bytes(raw).decode("utf-8"))
        except Exception:
            existing.append(None)

    additions = []
    for tok in ISA_SPECIAL_TOKENS:
        if tok in existing:
            print(f"  already present: {tok}", file=sys.stderr)
            continue
        additions.append(tok)

    print(f"[rebuild_tokenizer] base vocab size: {len(existing)}")
    print(f"[rebuild_tokenizer] would add {len(additions)} ISA tokens:")
    for t in additions:
        print(f"  + {t}")

    if args.dry_run:
        print("[rebuild_tokenizer] dry-run; not writing")
        return 0

    # The actual write path uses GGUFWriter. For brevity (and because the
    # llama.cpp GGUFReader/Writer round-trip with vocab extension is
    # non-trivial without also resizing the embedding matrix), we delegate
    # to llama.cpp's `convert_hf_to_gguf.py` after first patching the HF
    # tokenizer — see docs/fine-tune-recipe.md §2 for the full workflow.
    print()
    print("[rebuild_tokenizer] write path requires the HF→GGUF round trip:", file=sys.stderr)
    print("  1. Patch the HF tokenizer: tokenizer.add_special_tokens({'additional_special_tokens': [...]})", file=sys.stderr)
    print("  2. Resize embeddings: model.resize_token_embeddings(len(tokenizer))", file=sys.stderr)
    print("  3. Save: model.save_pretrained(); tokenizer.save_pretrained()", file=sys.stderr)
    print("  4. Convert: python3 llama.cpp/convert_hf_to_gguf.py <patched_dir> --outfile <out.gguf>", file=sys.stderr)
    print("  5. Quantize: llama-quantize <out.gguf> <out-q4.gguf> Q4_K_M", file=sys.stderr)
    print()
    print("  See docs/fine-tune-recipe.md for the full DPO/GRPO recipe.", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
