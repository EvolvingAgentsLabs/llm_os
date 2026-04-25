#!/usr/bin/env bash
# Validate isa.gbnf against the legal/illegal fixture corpus.
#
# Legal fixtures (grammar/tests/legal/*.txt) MUST parse.
# Illegal fixtures (grammar/tests/illegal/*.txt) MUST be rejected.
#
# Exit codes:
#   0  — every fixture behaved as classified.
#   1  — at least one fixture misbehaved (grammar bug or fixture mis-classified).
#   2  — llama-gbnf-validator not found on $PATH; see README §Quickstart.
#
# Pin: tested against llama.cpp commit referenced in
# C:/evolvingagents/llama.cpp (cloned 2026-04-24). If the validator's
# exit-code semantics drift in a future llama.cpp release, update the
# parsing in this script.

set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GRAMMAR="$ROOT/grammar/isa.gbnf"
LEGAL_DIR="$ROOT/grammar/tests/legal"
ILLEGAL_DIR="$ROOT/grammar/tests/illegal"

# Locate the validator. Accept either a global install or a sibling
# llama.cpp checkout under C:/evolvingagents/llama.cpp/.
#
# Note: in current llama.cpp master, the validator binary is built as
# `test-gbnf-validator` under tests/ (it was renamed from
# `llama-gbnf-validator` in 2025); we accept both for back-compat.
VALIDATOR=""
for name in llama-gbnf-validator test-gbnf-validator; do
  if command -v "$name" >/dev/null 2>&1; then
    VALIDATOR="$name"
    break
  fi
done

if [ -z "$VALIDATOR" ]; then
  for cand in \
    "$ROOT/../llama.cpp/build/bin/test-gbnf-validator" \
    "$ROOT/../llama.cpp/build/bin/Release/test-gbnf-validator.exe" \
    "$ROOT/../llama.cpp/build/bin/test-gbnf-validator.exe" \
    "$ROOT/../llama.cpp/build/bin/llama-gbnf-validator" \
    "$ROOT/../llama.cpp/build/bin/Release/llama-gbnf-validator.exe" \
    "$ROOT/../llama.cpp/build/bin/llama-gbnf-validator.exe"; do
    if [ -x "$cand" ] || [ -f "$cand" ]; then
      VALIDATOR="$cand"
      break
    fi
  done
fi

if [ -z "$VALIDATOR" ]; then
  echo "error: gbnf-validator not found on \$PATH or in ../llama.cpp/build/bin/." >&2
  echo "       Build it via: cd ../llama.cpp \\" >&2
  echo "                  && cmake -B build -DLLAMA_CURL=OFF -DLLAMA_BUILD_TESTS=ON \\" >&2
  echo "                  && cmake --build build --config Release --target test-gbnf-validator -j" >&2
  echo "       Or see README \xc2\xa7Quickstart." >&2
  exit 2
fi

# Run a fixture. Returns 0 if validator accepts (legal sense), 1 if rejects.
#
# Note: test-gbnf-validator always exits 0 — it reports success/failure via
# stdout ("Input string is valid" vs "Input string is invalid"). We match
# the stdout text rather than the exit code.
run_one() {
  local fixture="$1"
  local out
  out=$("$VALIDATOR" "$GRAMMAR" "$fixture" 2>&1)
  case "$out" in
    *"Input string is valid"*)   return 0 ;;
    *"Input string is invalid"*) return 1 ;;
    *)
      echo "validator returned unexpected output for $fixture:" >&2
      echo "$out" >&2
      return 2
      ;;
  esac
}

legal_pass=0
legal_total=0
legal_fail_files=()
for f in "$LEGAL_DIR"/*.txt; do
  legal_total=$((legal_total + 1))
  if run_one "$f"; then
    legal_pass=$((legal_pass + 1))
  else
    legal_fail_files+=("$(basename "$f")")
  fi
done

illegal_fail=0
illegal_total=0
illegal_pass_files=()
for f in "$ILLEGAL_DIR"/*.txt; do
  illegal_total=$((illegal_total + 1))
  if run_one "$f"; then
    illegal_pass_files+=("$(basename "$f")")
  else
    illegal_fail=$((illegal_fail + 1))
  fi
done

echo "legal:   $legal_pass/$legal_total pass"
echo "illegal: $illegal_fail/$illegal_total fail"

problems=0
if [ "$legal_pass" -ne "$legal_total" ]; then
  echo "  legal fixtures that should have passed but did not:" >&2
  for f in "${legal_fail_files[@]}"; do echo "    - $f" >&2; done
  problems=1
fi
if [ "$illegal_fail" -ne "$illegal_total" ]; then
  echo "  illegal fixtures that should have been rejected but parsed:" >&2
  for f in "${illegal_pass_files[@]}"; do echo "    - $f" >&2; done
  problems=1
fi

exit $problems
