#!/usr/bin/env bash
# L1 — End-to-end navigate-and-report fixture.
#
# Runs the I/O daemon against the `sim_world` cartridge with the goal of
# reaching (3,0) from (0,0). Per CONTINUATION_PLAN L1, acceptance is:
#   - 10 consecutive runs all `<|halt|>status=success`
#   - Average wall time ≤ 10 minutes on Pi 5 8GB with Qwen 3 2B Q4
#   - Token count per run logged
#   - Zero grammar violations (validated by tee'ing daemon output through
#     test-gbnf-validator post hoc)
#
# This script is a Pi 5 / Linux deployment harness. It is NOT runnable on
# the Windows dev box directly because:
#   1. The daemon needs llama-server running (POSIX path setup).
#   2. The bootloader is POSIX C (uses fork/execvp).
#   3. The 10-minute budget is sized for Pi 5 throughput.
#
# Usage (on Pi 5 or Linux dev box):
#     export LLM_OS_MODEL=~/models/qwen3-2b-q4.gguf
#     export LLM_OS_LLAMA_SERVER=$(which llama-server)
#     export LLM_OS_PORT=8080
#     bash tests/e2e/navigate_and_report.sh
#
# Optional env:
#     LLM_OS_RUNS=10              # how many iterations
#     LLM_OS_TASK_BUDGET=600      # per-task wall-clock seconds
#     LLM_OS_LOG_DIR=./logs/e2e

set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

: "${LLM_OS_MODEL:?error: set LLM_OS_MODEL to a GGUF path}"
: "${LLM_OS_LLAMA_SERVER:=$(command -v llama-server || true)}"
: "${LLM_OS_PORT:=8080}"
: "${LLM_OS_RUNS:=10}"
: "${LLM_OS_TASK_BUDGET:=600}"
: "${LLM_OS_LOG_DIR:=$ROOT/logs/e2e}"

if [ -z "$LLM_OS_LLAMA_SERVER" ]; then
  echo "error: llama-server not on \$PATH and LLM_OS_LLAMA_SERVER not set" >&2
  exit 2
fi

mkdir -p "$LLM_OS_LOG_DIR"

# ─── Boot llama-server via the bootloader ───────────────────────────────
BOOTLOADER="$ROOT/runtime/bootloader"
if [ ! -x "$BOOTLOADER" ]; then
  echo "error: bootloader binary missing. Build with:" >&2
  echo "    cc -O2 -Wall -Wextra -o runtime/bootloader runtime/bootloader.c" >&2
  exit 2
fi

IOD="$ROOT/runtime/target/release/iod"
if [ ! -x "$IOD" ]; then
  echo "error: iod binary missing. Build with:" >&2
  echo "    (cd runtime && cargo build --release)" >&2
  exit 2
fi

VALIDATOR=""
for cand in \
  "$ROOT/../llama.cpp/build/bin/Release/test-gbnf-validator.exe" \
  "$ROOT/../llama.cpp/build/bin/test-gbnf-validator" \
  "$(command -v test-gbnf-validator || true)" \
  "$(command -v llama-gbnf-validator || true)"; do
  if [ -n "$cand" ] && [ -x "$cand" ]; then
    VALIDATOR="$cand"
    break
  fi
done

echo "[e2e] booting llama-server with model=$LLM_OS_MODEL port=$LLM_OS_PORT"
SERVER_PID=$("$BOOTLOADER" \
    --model "$LLM_OS_MODEL" \
    --port "$LLM_OS_PORT" \
    --server-bin "$LLM_OS_LLAMA_SERVER" \
    --boot-timeout 60)
if [ -z "$SERVER_PID" ]; then
  echo "[e2e] bootloader failed; aborting" >&2
  exit 1
fi
trap 'kill "$SERVER_PID" 2>/dev/null || true' EXIT
echo "[e2e] server up (pid=$SERVER_PID); running $LLM_OS_RUNS iterations"

# ─── Run iterations ─────────────────────────────────────────────────────
GOAL="Reach the cell at (3, 0). Use sim_world.observe to see your current position, sim_world.step with dir in {north, east, south, west} to move, and halt with status=success when at_goal is true."
SUCCESS=0
FAILURE=0
PARTIAL=0
WALL_TOTAL=0
for i in $(seq 1 "$LLM_OS_RUNS"); do
  LOG="$LLM_OS_LOG_DIR/run_$(printf '%02d' "$i").log"
  T_START=$(date +%s)
  set +e
  "$IOD" \
    --server "http://127.0.0.1:$LLM_OS_PORT" \
    --grammar "$ROOT/grammar/isa.gbnf" \
    --cart "$ROOT/cart" \
    --goal "$GOAL" \
    --budget "$LLM_OS_TASK_BUDGET" \
    > "$LOG" 2>&1
  CODE=$?
  set -e
  T_END=$(date +%s)
  WALL=$((T_END - T_START))
  WALL_TOTAL=$((WALL_TOTAL + WALL))
  case "$CODE" in
    0) SUCCESS=$((SUCCESS + 1)); STATUS="success" ;;
    2) PARTIAL=$((PARTIAL + 1)); STATUS="partial" ;;
    *) FAILURE=$((FAILURE + 1)); STATUS="failure" ;;
  esac

  # Post-hoc grammar validation: extract emitted ISA segments from the log
  # and run the validator. v0.01 bonus check.
  GVIOL="?"
  if [ -n "$VALIDATOR" ] && [ -f "$LOG" ]; then
    # The daemon logs each segment at debug level; we don't have a clean
    # extracted-stream fixture here, so this is informational. For v0.1,
    # iod should emit a `--trace` flag that writes the raw model stream
    # to a sibling file for direct validation.
    GVIOL="not-checked"
  fi

  echo "  run $i: $STATUS (${WALL}s, grammar=$GVIOL, log=$LOG)"
done

# ─── Summary ────────────────────────────────────────────────────────────
AVG_WALL=$((WALL_TOTAL / LLM_OS_RUNS))
echo
echo "[e2e] summary"
echo "  success: $SUCCESS / $LLM_OS_RUNS"
echo "  partial: $PARTIAL"
echo "  failure: $FAILURE"
echo "  avg wall-clock per run: ${AVG_WALL}s"
echo "  budget per run: ${LLM_OS_TASK_BUDGET}s"

if [ "$SUCCESS" -ne "$LLM_OS_RUNS" ]; then
  echo "[e2e] FAIL — not all runs succeeded" >&2
  exit 1
fi
echo "[e2e] PASS — $LLM_OS_RUNS/$LLM_OS_RUNS success"
