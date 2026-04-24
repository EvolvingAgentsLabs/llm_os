# LLM-OS

> An operating system where `llama.cpp` is the CPU, the KV cache is RAM, a 13-opcode ISA is defined as GBNF over tokenizer special tokens, and the sampler loop is the fetch-decode-execute cycle.

This repository is `v0.01`: **Linux-0.01 posture.** Requires `llama.cpp` built from source. Runs on a Raspberry Pi 5 at ~8 Hz and on cloud APIs at 60–200 Hz. Same ISA, same grammar, same cartridges.

Read [`design/llm-os-design.md`](design/llm-os-design.md) first — it's the thesis.

## What's in this repo

```
llm_os/
├── design/          Thesis doc — why, how, the 13 opcodes, the 6-week plan
├── grammar/         The ISA as GBNF
│   ├── isa.gbnf     Grammar file
│   ├── isa-spec.md  Opcode table, invariants, legal/illegal examples
│   └── tests/       12 fixtures (6 legal, 6 illegal) for llama-gbnf-validator
├── runtime/         v0.01 bootloader + I/O daemon (Week 2 drafts)
├── agents/          Markdown agent definitions (design-time artifacts)
├── scripts/         Build/validate helpers
└── docs/            Plan, context, week-by-week status
```

## The 13 opcodes

| Opcode | Purpose |
|---|---|
| `<\|read\|>` / `<\|write\|>` | File descriptor I/O |
| `<\|call\|>` | Cartridge syscall (the real work) |
| `<\|yield\|>` / `<\|wait\|>` | Scheduler hand-off |
| `<\|fork\|>` | Spawn a subtask with a new KV branch |
| `<\|loop\|>` / `<\|break\|>` | **Model-controlled closed loops** — the LLM decides when to reinject output |
| `<\|halt\|>` | Terminate (legal only at loop depth 0) |
| `<\|think\|>` / `<\|commit\|>` | Inner monologue; materialize state to persistent memory |
| `<\|fault\|>` | Typed exception → handler cartridge (cloud fallback lives here) |
| `<\|policy\|>` | Query capability set → daemon re-masks logit bias |

Full spec with semantics, invariants, and legal/illegal examples: [`grammar/isa-spec.md`](grammar/isa-spec.md).

## Quickstart — validate the grammar

```bash
# One-time: build llama.cpp's validator
git clone --depth 1 https://github.com/ggerganov/llama.cpp.git
cd llama.cpp && cmake -B build -DLLAMA_CURL=OFF && cmake --build build --target llama-gbnf-validator -j
export PATH="$PWD/build/bin:$PATH"

# Run the fixtures
cd /path/to/llm_os
./scripts/validate_grammar.sh
```

Legal fixtures MUST pass, illegal ones MUST fail — both cases are equally important: the grammar is a contract between sampler and I/O daemon, and wrong-rejection is as bad as wrong-acceptance.

## Status (2026-04-24)

- **Week 1 — ISA + GBNF**: grammar drafted + 12 test fixtures. Mechanical validation in progress.
- **Week 2 — Bootloader + I/O daemon**: skeleton drafts in `runtime/`.
- Weeks 3–6: cartridge adapter → closed-loop validation → RoClaw `/dev/roclaw` bridge → cloud fallback + compactor + v0.01 tag.

See [`docs/plan.md`](docs/plan.md) for the running status.

## Why this exists

Three sibling projects in [EvolvingAgentsLabs](https://github.com/EvolvingAgentsLabs) already contain every primitive needed for an actual LLM operating system:

- [`skillos`](https://github.com/EvolvingAgentsLabs/skillos) — markdown agents (user-space programs).
- [`skillos_mini`](https://github.com/EvolvingAgentsLabs/skillos_mini) — blackboard (IPC), cartridges (loadable drivers), schema-validated retry (protected mode), compactor (swap).
- [`RoClaw`](https://github.com/EvolvingAgentsLabs/RoClaw) — 13-opcode bytecode ISA with 6-byte UDP frames, shown to be emitted reliably by a VLM.

`llm_os` is the assembly: a formal ISA as GBNF over tokenizer tokens + a ~500-line I/O daemon + the LLM itself as the CPU. llama.cpp is the substrate.

## License

Apache 2.0 — see [LICENSE](LICENSE).

Kernel fine-tuning recipes and commercial cartridges may be dual-licensed (AGPL/commercial) in later milestones per [design §7](design/llm-os-design.md#7-ip-strategy-since-you-asked-in-a-past-conversation).

## References

- Karpathy, "LLM-OS" (Nov 2023 talk/post).
- Shivance, "LLM-OS with RLSF" (HuggingFace post, 2023).
- METR, "Measuring AI Ability to Complete Long Tasks" (Kwa et al., 2025) + TH1.1 update (Jan 2026).
- llama.cpp master: `grammars/README.md`, `tools/server/README.md`, `common/json-schema-to-grammar.cpp`.
