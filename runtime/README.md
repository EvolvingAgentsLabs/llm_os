# llm_os runtime crate (v0.01 scaffold)

## Modules

- **`bootloader.c`** — One-shot POSIX C program (~250 lines, no extra deps).
  `fork()`s `llama-server`, waits for it to bind, sends a boot prompt, scans
  the SSE stream for `<|ready|>`, prints the server PID to stdout, and exits.
  The I/O daemon owns the long-running session. Note: the bootloader does
  NOT apply the ISA grammar — that's per-request from the daemon.
  Linux/Pi 5 only (uses `fork`, `execvp`, POSIX sockets).

- **`tool_parser.rs`** — Rust port of `skillos_mini/mobile/src/lib/llm/tool_parser.ts`.
  Five tool-call shapes (A/A2/B/C/D) + JSON-repair for unescaped quotes.
  Used by the I/O daemon as a defense-in-depth fallback below the GBNF
  grammar — see refinement §2.4 + §4 R9.

To land in subsequent weeks per [`../docs/CONTINUATION_PLAN.md`](../docs/CONTINUATION_PLAN.md):

- `iod.rs` (~500 lines) — Week 2 D1.
- `swap.rs` — Week 6 (port of `skillos_mini/mobile/src/lib/llm/compactor.ts`).

## Building the bootloader

POSIX target — Linux or Raspberry Pi OS, not Windows. On a Pi 5:

```bash
cc -O2 -Wall -Wextra -o runtime/bootloader runtime/bootloader.c
```

Then run against a llama-server-capable build of llama.cpp (the `llama-server` binary must be on `$PATH`):

```bash
runtime/bootloader \
    --model ~/models/qwen3-2b-q4.gguf \
    --port  8080 \
    --ctx-size 32768
# Prints: <server-pid>   on success.
# Stderr: bootloader status messages.
```

## Building

Cargo is not installed by default on the target Pi 5 — install via `rustup`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
source "$HOME/.cargo/env"
```

On Windows: install the official `rustup-init.exe` from <https://rustup.rs/>.

## Running tests

```bash
cd runtime
cargo test
```

The `tool_parser` test suite is a 1:1 port of `skillos_mini/mobile/tests/tool_parser.spec.ts`.
Sixteen cases covering shapes A/A2/B/C/D, JSON repair, and tag extraction.
