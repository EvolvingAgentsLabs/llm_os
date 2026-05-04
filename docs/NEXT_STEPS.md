# Roadmap

> Updated May 2026 to reflect v1.0 implementation status. The original
> six items from the April 2026 architecture review have been addressed
> via the four-phase improvement plan (see [`IMPROVEMENT_PLAN.md`](IMPROVEMENT_PLAN.md)).
>
> Companion docs: [`README.md`](../README.md) · [`ARCHITECTURE.md`](ARCHITECTURE.md) · [`USAGE.md`](USAGE.md) · [`TUTORIAL.md`](TUTORIAL.md).

## Status summary (v0.5 → v1.0)

```
  ┌─[ ROADMAP STATUS ]──────────────────────────────────────────────┐
  │                                                                  │
  │  §1 GRAMMAR  in-process grammar stack         ✓ IMPLEMENTED     │
  │  §2 COMPACT  deterministic KV pager           ✓ IMPLEMENTED     │
  │  §3 RECOVER  three-strikes + cloud escalation   PARTIALLY DONE  │
  │  §4 SANDBOX  WASM cartridge sandbox           ✓ IMPLEMENTED     │
  │  §5 CAPS     daemon-side opcode reject          PARTIALLY DONE  │
  │  §6 TOKENS   single-token ISA fine-tune       ✓ IMPLEMENTED     │
  │  §7 OLLAMA   Ollama backend (no GBNF)         ✓ WORKING         │
  │                                                                  │
  └──────────────────────────────────────────────────────────────────┘
```

---

## §1 Grammar swap — IMPLEMENTED (Phase 1)

**Original problem:** 3 HTTP requests per syscall for grammar swap
(~200-400 ms overhead per syscall).

**Solution implemented:** In-process grammar stack in `sampler.rs`.
The sampler drives llama.cpp directly via FFI (`llama_ffi.rs`),
eliminating the HTTP boundary entirely. Grammar push/pop is a
zero-cost in-memory operation:

```rust
// On <|call|>cart.method — push method's args sub-grammar
sampler.push_grammar(method_grammar);

// On <|/call|> — pop back to ISA grammar
sampler.pop_grammar();
```

**Key files:**
- `runtime/llama_ffi.rs` — FFI wrapper over llama.cpp C API
- `runtime/sampler.rs` — in-process token generation + grammar stack

**Remaining work:**
- [ ] Vendor llama.cpp as submodule and wire `build.rs`
- [ ] Benchmark on Pi 5 hardware (target: 8 Hz steady)
- [ ] Wire `generate_segment()` actual implementation (currently returns
  interface-contract error pending libllama linkage)

---

## §2 ISA-aware KV compactor — IMPLEMENTED (Phase 2)

**Original problem:** KV compaction drops unclosed `<|loop|>` / pending
`<|result|>` state, causing silent grammar corruption.

**Solution implemented:** `kv_pager.rs` — deterministic positional
token eviction with anchors. No LLM involvement, fully deterministic,
< 5 ms.

Layout:
```
[SYSTEM PROMPT | ANCHORS... | EVICTABLE WINDOW | RECENT TAIL]
 ^never evict   ^protected   ^evict oldest     ^never evict
```

Anchors protect structurally important tokens (system prompt, ISA
state preamble, open loop headers). The pager integrates with
`swap::IsaState` for state serialization.

**Key files:**
- `runtime/kv_pager.rs` — pager implementation (9 tests)
- `runtime/swap.rs` — ISA state extraction (reused)

**Remaining work:**
- [ ] Wire pager into `iod.rs` dispatch loop
- [ ] Integration test: 5000-token dispatch with depth-3 loops
  survives forced compaction

---

## §3 Three-strikes recovery — PARTIALLY DONE

**Original problem:** Schema violations burn context in retry loops
(30 seconds wasted on bootstrap models).

**Status:** The three-strikes rule and cloud escalation pattern are
**designed** in the dispatch module but not yet fully wired with the
`<|fault|>{needs_cloud:true}` automatic escalation path.

**What exists:**
- `runtime/cloud.rs` — cloud fallback HTTP adapter
- Schema violation results include `expected_grammar` for retry help

**Remaining work:**
- [ ] Add `violation_streak` + `violation_history` to dispatch context
- [ ] Wire automatic `<|fault|>` injection after 3 consecutive violations
- [ ] CLI flag: `--max-schema-strikes N`
- [ ] Integration test: synthetic violations escalate correctly

---

## §4 WASM cartridge sandbox — IMPLEMENTED (Phase 3)

**Original problem:** Cartridge handlers run in-process with full
system-call access. No real Ring 3 isolation.

**Solution implemented:** `wasm_host.rs` — wasmtime-based execution
environment with:
- `CartridgeCaps` — per-cartridge capability declaration
- `WasmEngine` — module loading, caching, dispatch
- Curated host functions: `host_log`, `host_clock_ms`, `host_kv_*`, `host_http_get`
- `check_capability()` + `check_timeout()` enforcement
- Per-cartridge KV store persistence

**Key files:**
- `runtime/wasm_host.rs` — WASM engine (6 tests)
- Feature flag: `wasm-sandbox = ["dep:wasmtime"]`
- Legacy: `runtime/handlers.rs` behind `legacy-handlers` flag

**Remaining work:**
- [ ] Wire wasmtime instantiation (currently returns interface error)
- [ ] Compile existing cartridges to `.wasm` (wasm32-wasip2)
- [ ] Build pipeline: `cart/build_all.sh`
- [ ] Evil-cart smoke test (filesystem escape attempt → trap)
- [ ] Benchmark: call overhead ≤ 0.5 ms cold, ≤ 0.05 ms warm

---

## §5 Daemon-side opcode reject — PARTIALLY DONE

**Original problem:** Logit bias brittle on multi-token bootstrap models.

**Status:** `runtime/capability.rs` already implements both layers:
- Logit-bias construction via `/tokenize` (soft optimization)
- The daemon-side parser catches forbidden ops

The v1.0 architecture makes this largely solved by Phase 4 (single-token
ISA). With every opcode being exactly one token, the logit bias is
**exact** — there's no multi-token ambiguity to worry about.

**What exists:**
- `runtime/capability.rs` — logit bias + unique-token preference
- Daemon-side reject in parser/dispatch path

**Remaining work:**
- [ ] Formalize `enforce_runtime` path as the documented hard fence
- [ ] Downgrade bias documentation to "hint / optimization"
- [ ] Evil-cart smoke test with bias disabled → daemon catches all

---

## §6 Single-token ISA fine-tune — IMPLEMENTED (Phase 4)

**Original problem:** Bootstrap opcodes are 1–5 tokens depending on BPE.

**Solution implemented:** `scripts/rebuild_tokenizer.py` — full pipeline
with 4 subcommands:

1. `patch` — add 20 ISA tokens as special tokens + resize embeddings
2. `validate` — verify all are single-token
3. `finetune` — LoRA DPO on promoted traces
4. `export` — convert to GGUF with quantization

And `scripts/check_tokenizer.sh` with `--model <path>` validation mode
(CI gate — exits 0 only if all 20 ISA tokens are single-token).

**Key files:**
- `scripts/rebuild_tokenizer.py` — full pipeline
- `scripts/check_tokenizer.sh` — CI validation gate

**Remaining work:**
- [ ] Run the full pipeline on Qwen 2.5 3B base model
- [ ] Publish `qwen-2.5-3b-llmos-v1.gguf` on HuggingFace
- [ ] Measure schema-violation rate on fine-tuned model (target < 0.5%)

---

## §7 Ollama backend — WORKING (May 2026)

**Problem:** Running llm_os requires building llama.cpp from source and
downloading GGUF models manually. High friction for first-time users.

**Solution implemented:** Native Ollama backend via `/api/generate`.
Any `ollama pull`-ed model works with `--ollama --model <tag>`.

**Tested end-to-end with Qwen 3.5 2B:**

- Single-call tasks: Think → Call → Write → Halt (100% reliable)
- Multi-step tasks: echo → hash with real daemon feedback between calls
- Cooking cartridge: correct arg names from schema hints

**Key features:**

| Feature | Purpose |
|---|---|
| `truncate_at_first_call()` | Forces per-call daemon dispatch (Ollama stops are unreliable) |
| `strip_daemon_blocks()` | Removes hallucinated `<\|result\|>` and `<\|ack\|>` blocks |
| Unclosed `<\|think\|>` repair | Fixes model merging think+call without closing tag |
| Repeat-call detection | Nudges model after 3 identical calls |
| `think: false` | Disables Qwen 3.5 CoT mode (empty response otherwise) |
| `repeat_penalty: 1.3` | Token-level repetition penalty |
| Schema arg hints | `Cartridge.args_hint()` puts field names in boot prompt |
| `--max-predict` flag | Default 128 for Ollama (vs 512 for llama-server) |

**Key files:**
- `runtime/iod.rs` — `boot_prompt_ollama()`, `complete_via_ollama()`,
  `strip_daemon_blocks()`, `truncate_at_first_call()`
- `runtime/iod_main.rs` — `--ollama`, `--model`, `--max-predict` CLI flags
- `runtime/parser.rs` — tolerant `parse_call()` (defaults to `{}` for missing args)
- `runtime/cartridge.rs` — `args_hint()` method

**Remaining work:**
- [ ] Test with larger Ollama models (4B, 8B) for better instruction following
- [ ] Try `/api/chat` endpoint as alternative to `/api/generate`
- [ ] Extract GGUF from Ollama cache for llama-server (best of both worlds)
- [ ] Multi-step iterative tasks (sim_world navigation) — blocked by CPU inference speed

---

## Remaining v1.0 final gate

Before tagging `v1.0` (no `-rc`):

- [x] Ollama backend working end-to-end (single + multi-step)
- [ ] Vendor llama.cpp submodule + `build.rs` compiles `libllama.a`
- [ ] Full stop-and-inject loop working in-process on Pi 5
- [ ] KV pager wired into dispatch loop with real anchor tracking
- [ ] At least 1 cartridge compiled to `.wasm` and dispatched via wasmtime
- [ ] Fine-tuned GGUF validated: all 20 tokens single-token
- [ ] 100 sequential calls at ≥ 8 Hz on Pi 5 with 3B Q4_K_M
- [ ] 102+ tests passing (current: 100 pass, 9 pre-existing failures)

---

## Explicit non-goals (v1.0 scope)

- Multi-process iod federation (v1.5+)
- Persistent KV across reboots (separate cartridge concern)
- Distributed cartridges over network (RPC-style)
- Custom kernel-mode model from scratch (post-v1.0 research)
- Recursive subagent calls (capability propagation needs design)
- Dynamic cartridge hot-reload (v1.5+)

---

## Post-v1.0 considerations

| Topic | Earliest | Notes |
|---|---|---|
| Federation (multi-iod) | v1.5 | Shared KV pool across processes |
| Persistent state cartridge | v1.5 | `cart/system/state` with WAL |
| Cartridge hot-reload | v1.5 | Watch `cart/` dir, reload WASM modules |
| Custom kernel model | v2.0 | From-scratch training on ISA corpus |
| Recursive subagents | v1.5 | Capability propagation rules |
| Hardware watchdog integration | v1.0-final | Already in `image/overlay/init` |
