#!/usr/bin/env bash
# Quickstart — cold-checkout to first task.
#
# Per CONTINUATION_PLAN V1: "a stranger can git clone && ./scripts/quickstart.sh
# and have the LLM-OS run a 'hello, list this directory' task in under 10 minutes
# from a cold checkout."
#
# Linux/Pi 5 only.

set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# ─── 1. Build llama.cpp (validator + tokenize + server) ─────────────────
LLAMA_DIR="${LLAMA_DIR:-$ROOT/../llama.cpp}"
if [ ! -d "$LLAMA_DIR" ]; then
  echo "[1/5] cloning llama.cpp into $LLAMA_DIR"
  git clone --depth 1 https://github.com/ggerganov/llama.cpp.git "$LLAMA_DIR"
else
  echo "[1/5] llama.cpp already at $LLAMA_DIR — skipping clone"
fi
echo "[2/5] building llama.cpp targets (server, tokenize, gbnf-validator)"
(cd "$LLAMA_DIR" \
  && cmake -B build -DLLAMA_CURL=OFF -DLLAMA_BUILD_TESTS=ON -DBUILD_SHARED_LIBS=OFF \
  && cmake --build build --config Release --target llama-server llama-tokenize test-gbnf-validator -j)

# ─── 2. Build the bootloader (POSIX C) ──────────────────────────────────
echo "[3/5] building runtime/bootloader"
cc -O2 -Wall -Wextra -o "$ROOT/runtime/bootloader" "$ROOT/runtime/bootloader.c"

# ─── 3. Build the daemon (Rust) ─────────────────────────────────────────
echo "[4/5] building runtime/iod (cargo)"
if ! command -v cargo >/dev/null 2>&1; then
  echo "  error: cargo not found. Install via:" >&2
  echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y" >&2
  exit 2
fi
(cd "$ROOT/runtime" && cargo build --release)

# ─── 4. Validate the grammar ────────────────────────────────────────────
echo "[5/5] validating grammar against fixtures"
bash "$ROOT/scripts/validate_grammar.sh"

cat <<EOF

----------------------------------------------------------------
Quickstart complete. To run a task:

  export LLM_OS_MODEL=~/models/gemma-4-e2b-q4.gguf  # or qwen3-2b-q4.gguf
  export PATH="$LLAMA_DIR/build/bin:\$PATH"

  # Boot llama-server (one-shot; prints PID to stdout):
  ./runtime/bootloader --model "\$LLM_OS_MODEL" --port 8080

  # Run a task:
  ./runtime/target/release/iod \\
      --server http://127.0.0.1:8080 \\
      --grammar grammar/isa.gbnf \\
      --cart    cart \\
      --goal    "echo via the demo cartridge"

End-to-end navigate-and-report fixture (10-iteration acceptance test):

  export LLM_OS_MODEL=~/models/gemma-4-e2b-q4.gguf
  bash tests/e2e/navigate_and_report.sh
----------------------------------------------------------------
EOF
