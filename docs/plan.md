# Project_llm_os — Plan

## Status
Scaffolded 2026-04-24. **Week 1 grammar drafted (not yet validated).**

> See [`CONTINUATION_PLAN.md`](CONTINUATION_PLAN.md) for the operational playbook with file-level deliverables, acceptance criteria, dependency graph, and risk register. This file is the high-level sketch.

## Week-by-week

### Week 1 — ISA + GBNF  [DRAFT COMPLETE]
- [x] Draft `isa.gbnf` with 13 opcodes, Qwen 3 2B string-pattern form.
- [x] Write `isa-spec.md` with opcode table, invariants, kernel-native migration notes.
- [x] Write 6 legal + 6 illegal test fixtures.
- [ ] Run `llama-gbnf-validator` on fixtures — **BLOCKED** (validator not installed on this host; build llama.cpp on dev box or Pi).
- [ ] Sanity-check Qwen 3 2B tokenization of opcode strings via `llama-tokenize`.

### Week 2 — Bootloader + I/O daemon
- [ ] `bootloader.c` (~200 lines): fork llama.cpp server with `--grammar isa.gbnf --cache-prompt true --reasoning-format none`, open FIFOs, inject boot prompt, wait for `<|ready|>`.
- [ ] `iod.rs` (~500 lines): stream llama.cpp output, dispatch opcodes, inject `<|result|>`/`<|ack|>` blocks, maintain loop-depth counter.
- [ ] Decide: one-program-per-task vs long-running session with top-level `<|loop|>`.

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
