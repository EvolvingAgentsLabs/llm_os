# Project_llm_os — Context

## Goal
Build LLM-OS: an OS where llama.cpp is the CPU, KV cache is RAM, a 13-opcode ISA is defined as GBNF special tokens, and the sampler loop is fetch-decode-execute. Target Pi 5 (8 Hz) and cloud (60–200 Hz) with the same ISA.

## Thesis
SkillOS + SkillOS-mini + RoClaw already contain every OS primitive — blackboard (IPC), cartridges (loadable drivers), schema-validated retry (protected mode), compactor (swap), and the RoClaw 6-byte UDP bytecode (proof an LLM can emit an ISA). Assembly is the remaining work.

## Repos this project leans on
- `skillos/` — Desktop prefrontal cortex (markdown agents, SystemAgent orchestrator).
- `skillos_mini/` — Edge runtime (cartridges, blackboard, compactor, wllama/LiteRT).
- `RoClaw/` — 13-opcode bytecode ISA, 6-byte UDP frames, VLM→ISA emission proven.

## v0.01 scope (6-week sprint per design §8)
- Week 1: `isa.gbnf` targeting Qwen 3 2B tokenizer, two-token opcode sequences.
- Week 2: `bootloader.c` (~200 lines) + `iod.rs` (~500 lines); FIFOs for transport.
- Week 3: Cartridge adapter — port `cooking`, `residential-electrical`, `demo` from skillos_mini.
- Week 4: Closed-loop `<|loop|>`/`<|break|>` grammar + Pi 5 fixture test.
- Week 5: `/dev/roclaw` cartridge bridging to the 6-byte UDP ISA.
- Week 6: `<|fault|>`→cloud fallback, compactor as swap daemon, tag v0.01.

## ISA (13 opcodes)
read, write, call, yield, fork, wait, loop, break, halt, think, commit, fault, policy.
Grammar enforces legal sequences (e.g., `<|halt|>` only at loop depth 0).

## Hardware targets
- Pi 5 8GB + Gemma 4 E2B Q4 → ~8 tok/s (~8 Hz, ~6s per 50-token syscall).
- Cloud (GPT-5.4 / Opus 4.6 / Kimi K2.5) → 60–200 Hz (250–800ms per syscall).
- Dual-brain: Pi cerebellum at 8 Hz + cloud cortex every 10–30s.

## Milestones
- v0.01 (1mo) — bootstrap, 3–5 opcodes, prompt-based, Linux-0.01 posture.
- v0.1 (2mo) — fine-tuned kernel GGUF, full 13-opcode ISA as single tokens.
- v0.5 (6mo) — preemptive scheduler, KV swap, self-hosting, `/dev/roclaw` live.
- v1.0 (12mo) — production, ~30mo total Linux-parallel timeline.

## Status
Project scaffolded 2026-04-24. Design doc in `input/llm-os-design.md`. No code yet.
