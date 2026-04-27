#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────────
# LLM-OS dev mode — run on regular Linux/macOS for development.
#
# This is the development counterpart to image/build.sh (release mode).
# It runs everything locally without Buildroot, cross-compilation, or
# SD card images. Same architecture, same binaries, different host.
#
# Prerequisites:
#   - llama.cpp built (scripts/quickstart.sh handles this)
#   - cargo (Rust toolchain)
#   - A GGUF model file
#
# Usage:
#   # Minimal (uses smallest viable model for fast iteration)
#   bash scripts/dev.sh --model ~/models/qwen2.5-0.5b-q4.gguf
#
#   # With a goal
#   bash scripts/dev.sh --model ~/models/qwen2.5-3b-q4.gguf \
#       --goal "echo via the demo cartridge"
#
#   # With cloud fallback
#   bash scripts/dev.sh --model ~/models/qwen2.5-0.5b-q4.gguf \
#       --cloud-url "https://generativelanguage.googleapis.com/v1beta" \
#       --cloud-key "$GEMINI_API_KEY" \
#       --cloud-model "gemini-2.5-flash"
#
#   # Just build (don't run)
#   bash scripts/dev.sh --build-only
#
# Recommended dev models (see docs/models.md for full matrix):
#   Fastest iteration:  Qwen2.5-0.5B-Instruct Q4_K_M  (~350MB, ~50 tok/s on laptop)
#   Good balance:       Qwen2.5-1.5B-Instruct Q4_K_M  (~900MB, ~25 tok/s)
#   Pi 5 reference:     Qwen2.5-3B-Instruct Q4_K_M    (~1.7GB, ~8 tok/s)
# ──────────────────────────────────────────────────────────────────────

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# ── Parse args ───────────────────────────────────────────────────────
MODEL_FILE=""
GOAL=""
PORT="${LLM_OS_PORT:-8080}"
CTX="${LLM_OS_CTX:-4096}"   # Smaller ctx for dev speed
BUILD_ONLY=false
CLOUD_URL="${LLM_OS_CLOUD_URL:-}"
CLOUD_KEY="${LLM_OS_CLOUD_KEY:-}"
CLOUD_MODEL="${LLM_OS_CLOUD_MODEL:-}"

while [ $# -gt 0 ]; do
    case "$1" in
        --model)       MODEL_FILE="$2"; shift 2 ;;
        --goal)        GOAL="$2"; shift 2 ;;
        --port)        PORT="$2"; shift 2 ;;
        --ctx)         CTX="$2"; shift 2 ;;
        --build-only)  BUILD_ONLY=true; shift ;;
        --cloud-url)   CLOUD_URL="$2"; shift 2 ;;
        --cloud-key)   CLOUD_KEY="$2"; shift 2 ;;
        --cloud-model) CLOUD_MODEL="$2"; shift 2 ;;
        --help|-h)
            sed -n '2,/^set -/{ /^#/s/^# \?//p }' "$0"
            exit 0 ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

# ── 1. Build llama.cpp (host architecture) ──────────────────────────
LLAMA_DIR="${LLAMA_DIR:-$ROOT/../llama.cpp}"
if [ ! -d "$LLAMA_DIR" ]; then
    echo "[dev] cloning llama.cpp"
    git clone --depth 1 https://github.com/ggerganov/llama.cpp.git "$LLAMA_DIR"
fi

if [ ! -f "$LLAMA_DIR/build/bin/llama-server" ]; then
    echo "[dev] building llama.cpp (host)"
    (cd "$LLAMA_DIR" \
        && cmake -B build -DLLAMA_CURL=OFF -DBUILD_SHARED_LIBS=OFF \
        && cmake --build build --config Release --target llama-server -j"$(nproc 2>/dev/null || sysctl -n hw.ncpu)")
fi

# ── 2. Build bootloader (host) ──────────────────────────────────────
if [ ! -f "$ROOT/runtime/bootloader" ]; then
    echo "[dev] building bootloader"
    cc -O2 -Wall -Wextra -o "$ROOT/runtime/bootloader" "$ROOT/runtime/bootloader.c"
fi

# ── 3. Build iod (host) ─────────────────────────────────────────────
echo "[dev] building iod (cargo)"
(cd "$ROOT/runtime" && cargo build --release 2>&1 | tail -1)

if $BUILD_ONLY; then
    echo "[dev] build complete. Binaries:"
    echo "  llama-server: $LLAMA_DIR/build/bin/llama-server"
    echo "  bootloader:   $ROOT/runtime/bootloader"
    echo "  iod:          $ROOT/runtime/target/release/iod"
    exit 0
fi

# ── 4. Validate inputs ──────────────────────────────────────────────
if [ -z "$MODEL_FILE" ]; then
    echo "error: --model is required"
    echo "Recommended for dev: Qwen2.5-0.5B-Instruct Q4_K_M (~350MB)"
    echo "  Download: huggingface-cli download Qwen/Qwen2.5-0.5B-Instruct-GGUF qwen2.5-0.5b-instruct-q4_k_m.gguf --local-dir ~/models"
    exit 1
fi

if [ ! -f "$MODEL_FILE" ]; then
    echo "error: model not found: $MODEL_FILE"
    exit 1
fi

# ── 5. Export cloud config if set ────────────────────────────────────
if [ -n "$CLOUD_URL" ]; then
    export LLM_OS_CLOUD_URL="$CLOUD_URL"
    export LLM_OS_CLOUD_KEY="$CLOUD_KEY"
    export LLM_OS_CLOUD_MODEL="$CLOUD_MODEL"
    echo "[dev] cloud fallback: $CLOUD_MODEL via $CLOUD_URL"
fi

# ── 6. Boot ──────────────────────────────────────────────────────────
export PATH="$LLAMA_DIR/build/bin:$PATH"

echo "[dev] booting llama-server (model=$(basename "$MODEL_FILE"), port=$PORT, ctx=$CTX)"
"$ROOT/runtime/bootloader" \
    --model "$MODEL_FILE" \
    --port "$PORT" \
    --ctx-size "$CTX" \
    --server-bin llama-server

echo "[dev] llama-server ready"

# ── 7. Run iod ──────────────────────────────────────────────────────
IOD_ARGS=(
    --server "http://127.0.0.1:${PORT}"
    --grammar "$ROOT/grammar/isa.gbnf"
    --cart "$ROOT/cart"
)

if [ -n "$GOAL" ]; then
    IOD_ARGS+=(--goal "$GOAL")
    echo "[dev] running goal: $GOAL"
else
    echo "[dev] no --goal specified; iod will await dispatch via /dispatch endpoint"
fi

exec "$ROOT/runtime/target/release/iod" "${IOD_ARGS[@]}"
