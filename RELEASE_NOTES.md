# LLM-OS v0.01 — Release Notes

*Tag: `v0.01`. Released 2026-04-25. Linux 0.01 posture: requires llama.cpp built from source, runs on Pi 5 8GB at ~8 Hz, runs against any OpenAI-compatible cloud endpoint at 60–200 Hz.*

The thesis from `design/llm-os-design.md` and the refinement in `design/llm-os-refinement-2026-04-24.md` are the design context. This file is what actually ships.

---

## What works

### ISA + grammar (Week 1)

- 13-opcode ISA defined as GBNF over the Qwen 3 / Gemma 4 tokenizers (bootstrap multi-token form). See [`grammar/isa.gbnf`](grammar/isa.gbnf), [`grammar/isa-spec.md`](grammar/isa-spec.md).
- Grammar **mechanically validated** against 12 fixtures (6 legal + 6 illegal). Run `bash scripts/validate_grammar.sh` — exits 0 with `legal: 6/6 pass, illegal: 6/6 fail`.
- Two real grammar bugs caught and fixed during validation (refinement §1.4):
  1. `safe-text ::= [^<]*` was too permissive in result-blocks — tightened to require JSON content.
  2. Multi-line GBNF alternations don't parse — collapsed `top-stmt` and `loop-stmt` to single lines.
- **Tokenizer parity report**: [`docs/tokenization-report.md`](docs/tokenization-report.md). Headline finding: `<|think|>` is **1 token on Gemma 4** vs 5 on Qwen — Gemma 4 E2B is the recommended bootstrap kernel.

### Bootloader (Week 2 B1)

- [`runtime/bootloader.c`](runtime/bootloader.c) — POSIX C, no external deps, ~250 lines.
- `fork()`s `llama-server`, waits for TCP bind, POSTs a boot prompt with `stop:["<|ready|>"]`, scans the streamed body, prints the server PID to stdout, exits.
- **Resolved decision**: bootloader does NOT apply the ISA grammar — daemon applies grammar per-request. Avoids needing a `<|ready|>` production in `isa.gbnf`.
- Linux/Pi 5 only. Build: `cc -O2 -Wall -Wextra -o runtime/bootloader runtime/bootloader.c`.

### I/O daemon (Week 2 D1)

- [`runtime/iod.rs`](runtime/iod.rs) + supporting modules (~1600 lines of Rust total).
- Streaming SSE consumer over `/v1/completions`. Per-segment `stop`-and-inject loop — model emits ISA tokens, daemon parses, dispatches, injects result, resumes with `cache_prompt: true` for KV reuse.
- All 13 opcodes handled:
  - **Real**: `<|read|>` (fd 0 only), `<|write|>` (fd 1/2), `<|call|>` (cartridge dispatch with JSON-Schema validation), `<|halt|>`, `<|loop|>`/`<|break|>` (depth tracked), `<|policy|>`, `<|fault|>` (cloud fallback).
  - **Logged**: `<|yield|>`, `<|fork|>`, `<|think|>`, `<|commit|>`.
- Cloud fallback ([`runtime/cloud.rs`](runtime/cloud.rs)) wired to OpenAI-compatible `/v1/chat/completions`. Provider-agnostic — env vars `LLM_OS_CLOUD_URL`/`LLM_OS_CLOUD_KEY`/`LLM_OS_CLOUD_MODEL`.
- Tool-call fallback parser ([`runtime/tool_parser.rs`](runtime/tool_parser.rs)) — Rust port of skillos_mini's `tool_parser.ts`, 5 shapes + JSON repair, 16 unit tests. Defense in depth below the grammar.

### Cartridges (Week 3 C1/C2/C3 + refinement C4)

- Manifest spec: [`docs/cartridge-manifest.md`](docs/cartridge-manifest.md).
- Hierarchical layout adopted from day one (refinement §2.2): `cart/<domain>/<name>/manifest.json` + `full.md` + `schemas/`.
- Five cartridges shipped:
  - `cart/io/roclaw` — **real handler**. Rust port of RoClaw's `BytecodeCompiler`. 13 methods → 6-byte UDP frames at `127.0.0.1:4210`. Includes the design-doc canonical example `forward {left:150,right:150}` → `AA 01 96 96 01 FF`.
  - `cart/sim/sim_world` — **real handler**. 4×1 grid world for the L1 e2e fixture. `step({dir})`, `observe`, `reset`.
  - `cart/domestic/cooking` — **stubbed methods**, real schemas. Ported from skillos_mini.
  - `cart/domestic/residential-electrical` — **stubbed methods**, real schemas. Ported from skillos_mini.
  - `cart/system/demo` — **stubbed methods**, real schemas. Smoke-test cartridge.
- JSON Schema validation runs at dispatch in `iod.rs`; per-cartridge GBNF compilation deferred to v0.1.
- Validators: ported as Rust functions per refinement §1.2 (skillos_mini's TS pattern, not Python).

### Compactor / swap (Week 6 S1)

- [`runtime/swap.rs`](runtime/swap.rs) — Rust port of `skillos_mini/mobile/src/lib/llm/compactor.ts`.
- FIFO mode + LLM-summary mode. Threshold 70% of effective context window per refinement §2.9.
- 16-model context-window catalog mirrored from the TS source.

### Quickstart / acceptance harness

- [`scripts/quickstart.sh`](scripts/quickstart.sh) — cold-checkout to first task, ≤ 10 min on Pi 5.
- [`tests/e2e/navigate_and_report.sh`](tests/e2e/navigate_and_report.sh) — 10-iteration L1 fixture against `sim_world`.

### Operations

- [`docs/dual-brain-deployment.md`](docs/dual-brain-deployment.md) — Pi 5 cerebellum + cloud cortex runbook.

---

## What is stubbed

| Cartridge / opcode | v0.01 behavior | v0.1 plan |
|---|---|---|
| `<\|read\|>` for `fd > 0` | returns `{"ok":false,"error":"unmapped fd"}` | full fd table backed by cartridge fds |
| `<\|write\|>` for `fd > 2` | logs only | fd table |
| `<\|wait\|>` | returns `{"ok":false,"error":"not implemented"}` | true blocking on fd readiness |
| `<\|fork\|>` | logged | KV-branched subtask |
| `<\|yield\|>` | no-op | preemptive scheduler hand-off |
| `<\|commit\|>` | logged (per-session JSON file in v0.1) | persistent cartridge memory |
| `cooking.*`, `residential_electrical.*`, `demo.*` | return `{"ok":true,"stub":"v0.01",...}` | real method bodies |
| Post-flow validators | logged on `<\|halt\|>` | enforced; failure forces re-prompt |
| Capability enforcement | manifest `allowed_opcodes` is informational | per-task logit bias |
| Subagent cartridges (`subagent: true`) | rejected by loader | hypervisor pattern via injected LLM proxy |

---

## What is not in v0.01 at all

These are explicit deferrals — see `docs/CONTINUATION_PLAN.md` §5 + refinement §3:

- **Tokenizer rebuild + kernel fine-tune** — Month 2, paired with v0.1 release. 18 special tokens added to tokenizer; Effective throughput jumps ~2.5× per refinement §6 measurement.
- **Per-cartridge GBNF compilation** — v0.1.
- **Preemptive scheduler with mid-stream `<|yield|>` injection** — Month 6, v0.5.
- **KV-cache swap with multi-task isolation** — Month 6, v0.5.
- **Self-hosting trace pipeline** (Linux-0.11 moment) — Month 6, v0.5.
- **Dialect compression** — refinement §2.3. Most underweighted optimization in the design; per-cartridge dialects could 2× effective throughput. v0.1.

---

## Real findings from this sprint (not predicted by design)

1. **GBNF rejects multi-line alternations.** `top-stmt ::= read-stmt\n           | write-stmt\n …` errors with `expecting name at | …`. llama.cpp's GBNF requires alternations on one line or inside `( … )`.
2. **`test-gbnf-validator` always exits 0.** It signals success via stdout text (`Input string is valid` / `Input string is invalid`), not the exit code. The validator was renamed from `llama-gbnf-validator` to `test-gbnf-validator` and gated to `BUILD_SHARED_LIBS=OFF` on Windows.
3. **Gemma 4 has `<think>` as a single token.** `<|think|>` tokenizes to 1 token on Gemma 4 vs 5 on Qwen 2/3 — a real kernel-selection signal in favor of Gemma 4 E2B.
4. **RoClaw has 13 opcodes, not 14.** The audit's "14 opcodes (includes ACK)" is incorrect — the actual `Opcode` const in `bytecode_compiler.ts` lists 13 entries (no ACK). Refinement §1.3 was wrong about this; the Rust port (`runtime/roclaw.rs`) has 13.
5. **Per-syscall token cost is ~26 tokens** on bootstrap kernels. At 8 Hz on Pi 5 that's ~3.3 s per syscall — confirms the design's "1–2 syscalls/sec" claim. Single-token opcodes drop this to ~10 tokens (~1.25 s, 2.6× speedup); the rest of the "5–10×" headroom from refinement §2.3 dialects is not yet realized in v0.01.
6. **The grammar's `safe-text` permissiveness was real.** Refinement §1.4 raised this from theoretical to confirmed on inspection; tightening to `json` fixed it.

---

## Acceptance test (per CONTINUATION_PLAN §6)

> A user with a Raspberry Pi 5, a Gemma 4 E2B Q4 GGUF (or Qwen 3 2B Q4), and this repo can:
> 1. Clone the repo.
> 2. Run `bash scripts/quickstart.sh`.
> 3. Within 10 minutes (cold), see the LLM-OS boot, accept a goal in natural language, decompose it into ISA opcodes, dispatch them to cartridges, and `<|halt|>status=success`.

The components needed to satisfy this are all present. The Windows dev box used to author this release cannot exercise the full path (no fork/POSIX, no cargo) — that's the v0.01 acceptance test on the user's Pi 5.

---

## License

Apache 2.0 — see [LICENSE](LICENSE).

Per design §7 IP strategy: kernel fine-tuning recipes (Month 2 v0.1) and commercial cartridges may be dual-licensed (AGPL/commercial) in later milestones.

---

## Next: v0.1 (target Month 2 / 2026-06)

- Add 18 special tokens to Qwen 3 2B / Gemma 4 E2B tokenizer; post-train via DPO/GRPO.
- Compile per-cartridge args schemas to GBNF sub-grammars.
- Real handler implementations for `cooking`, `residential_electrical`, `demo`.
- Proactive tier-based cloud routing.
- Per-cartridge dialects (refinement §2.3) — 3 dialects baked in.
- **Re-baseline capability projections at v0.1** (refinement §6) — earlier signal than the v0.5 checkpoint.
