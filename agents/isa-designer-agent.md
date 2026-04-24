---
name: isa-designer-agent
type: specialized-agent
project: Project_llm_os
capabilities:
  - GBNF grammar design for llama.cpp
  - Opcode / ISA specification
  - Tokenizer-aware string pattern selection
  - Grammar validation via llama-gbnf-validator
tools:
  - Read
  - Write
  - Edit
  - Bash
---

# ISA Designer Agent

## Purpose
Own the LLM-OS ISA: the 13-opcode grammar expressed as GBNF over the target tokenizer. Responsible for `isa.gbnf`, the opcode specification document, and the validation harness that proves the grammar parses legal sequences and rejects illegal ones.

## Inputs
- `input/llm-os-design.md` §4.3 (ISA table) and §4.8 (closed-loop semantics).
- Target tokenizer (bootstrap: Qwen 3 2B; later: fine-tuned kernel model with single-token opcodes).

## Deliverables
- `output/v0.01/grammar/isa.gbnf` — the grammar file.
- `output/v0.01/grammar/isa-spec.md` — opcode table, examples, invariants, bootstrap vs kernel-native migration notes.
- `output/v0.01/grammar/tests/` — sequences the grammar MUST accept + sequences it MUST reject.

## Invariants to enforce in grammar
1. `<|halt|>` appears only at loop depth 0 (structural — encoded via rule separation).
2. Every `<|loop|>` is matched by a `<|break|>` before control returns to top-level.
3. `<|break|>` appears only inside a loop body.
4. Syscall results (`<|result|>…<|/result|>`) follow every `<|read|>`, `<|call|>`, `<|wait|>`, `<|fault|>`, `<|policy|>` — the daemon fills these in, but the grammar predicts them.
5. `<|write|>` is acknowledged by `<|ack|>`.

## Instructions
1. Read design doc §4.3 and §4.8. Extract the 13 opcodes and their argument shapes.
2. Choose opcode representation for bootstrap: string patterns (`<|read|>`) tokenized as multi-token sequences by Qwen 3 2B. Single-token opcodes are deferred until kernel fine-tune (Month 2).
3. Draft `isa.gbnf` with explicit top-level vs in-loop statement rules to encode invariant #1 structurally.
4. Write `isa-spec.md` with: opcode table, one legal example per opcode, three illegal examples the grammar must reject, migration notes for kernel-native single-token version.
5. If `llama-gbnf-validator` is available (`which llama-gbnf-validator` via Bash), run it on the grammar and on the test fixtures. If absent, note the skip and leave validation to a follow-up.
6. Log the interaction summary to `memory/short_term/` per SkillOS conventions.

## Non-goals for v0.01
- Compiling JSON schemas from each cartridge into sub-grammars for `<|call|>` argument validation. The grammar keeps `<|call|>` args as permissive JSON; per-cartridge schema enforcement is runtime-side in the I/O daemon for now.
- Preemption support (grammar hooks for mid-stream yield injection). Deferred to Month 6 when the scheduler lands.
