# Project_llm_os — Plan

## Status
Scaffolded 2026-04-24. **Week 1 grammar drafted and mechanically validated 2026-04-25** — all 6 legal fixtures parse, all 6 illegal fixtures reject. See `scripts/validate_grammar.sh`.

> See [`CONTINUATION_PLAN.md`](CONTINUATION_PLAN.md) for the operational playbook with file-level deliverables, acceptance criteria, dependency graph, and risk register. This file is the high-level sketch.

## Week-by-week

### Week 1 — ISA + GBNF  [VALIDATED]
- [x] Draft `isa.gbnf` with 13 opcodes, Qwen 3 2B string-pattern form.
- [x] Write `isa-spec.md` with opcode table, invariants, kernel-native migration notes.
- [x] Write 6 legal + 6 illegal test fixtures.
- [x] Run `test-gbnf-validator` on fixtures — `legal: 6/6 pass, illegal: 6/6 fail` (2026-04-25, llama.cpp built locally at `C:/evolvingagents/llama.cpp`).
- [x] Tighten `result-block` to `json` (was loose `safe-text`); fixtures updated. See refinement §1.4.
- [x] Fix GBNF alternation syntax bug (multi-line alternations require single-line or paren grouping); collapse `top-stmt` and `loop-stmt`.
- [x] Sanity-check tokenization of opcode strings via `llama-tokenize`. See `docs/tokenization-report.md`. Headline finding: **`<|think|>` is 1 token on Gemma 4** (vs 5 on Qwen 2/3) — Gemma has it as a native special token, which is a kernel-selection signal in favor of Gemma 4 E2B for the bootstrap kernel.

### Week 2 — Bootloader + I/O daemon
- [x] `bootloader.c` (~250 lines, POSIX C, no extra deps): fork llama-server, wait-for-bind, POST boot prompt with `stop:["<|ready|>"]`, scan SSE stream, print server PID, exit. **Resolved decision:** bootloader does NOT apply the ISA grammar — daemon applies grammar per-request via the `grammar` field in `/v1/completions`. Avoids needing a `<|ready|>` production in `isa.gbnf`. Linux/Pi 5 only; not testable on the Windows dev box.
- [x] `runtime/tool_parser.rs` (~430 lines + 16 tests): fallback `<|call|>` parser ported from skillos_mini, defense in depth below the grammar (refinement §2.4 + R9).
- [ ] `iod.rs` (~500 lines): stream llama.cpp output, dispatch opcodes, inject `<|result|>`/`<|ack|>` blocks, maintain loop-depth counter.
- [x] Decide: one-program-per-task vs long-running session — **resolved: many halting programs** (CONTINUATION_PLAN §3).

### Week 3 — Cartridge adapter
- [ ] Port `cooking`, `residential-electrical`, `demo` cartridges from `skillos_mini/cartridges/` to `/cart/<name>/` layout with manifest + allowed-opcode list.
- [ ] Runtime JSON-schema validator wired into I/O daemon's `<|call|>` dispatch.

### Week 4 — Closed-loop validation
- [ ] End-to-end fixture: 10-minute navigate-and-report task, measure tokens/time/success.

### Week 5 — RoClaw integration
- [ ] `/dev/roclaw` cartridge → 6-byte UDP frame emission. Reuse `skillos/roclaw_bridge.py` or port to Rust.
- [ ] Dual-brain: Pi cerebellum + cloud cortex end-to-end.

### Week 6 — Cloud fallback + compactor + v0.01 release
- [ ] `<|fault|>` → cloud API dispatch.
- [ ] Promote skillos_mini compactor to swap daemon.
- [ ] Tag v0.01. README. Grammar on HuggingFace? Code on GitHub.

## Open questions
- Qwen 3 2B Instruct vs base — tokenizer parity for opcode strings? (1-line `llama-tokenize` check Week 2.)
- Program shape: single long `<|loop|>` session vs many halting programs? Affects bootloader + scheduler design.
- JSON-schema→GBNF per cartridge: v0.1 or v0.5?

## Deferred (not v0.01)
- Fine-tuning recipe (Month 2).
- Preemptive scheduler + grammar preemption hooks (Month 6).
- Self-hosting trace pipeline (Month 6).
- Single-token opcode tokenizer rebuild (Month 2, paired with fine-tune).
