# LLM-OS Refinement — Corrigendum + Sharpened Sprint

*Companion to [`llm-os-design.md`](llm-os-design.md) and [`../docs/CONTINUATION_PLAN.md`](../docs/CONTINUATION_PLAN.md). Refinement date: 2026-04-24. The original design doc remains the thesis; this document records what changes after re-auditing the three sibling repos and re-reading the trajectory data, plus the resulting changes to the 6-week sprint, risks, and decisions.*

---

## 0. Why this document exists

The original design doc was drafted before the three sibling repos (`skillos`, `skillos_mini`, `RoClaw`) were re-read end-to-end. A fresh audit found four classes of divergence:

1. **Factual misstatements** — file names, opcode counts, language of validators, and one piece of architecture (RoClaw's bytecode emission path) that the design doc gets backwards in a load-bearing way.
2. **Missed primitives** — capabilities that already exist in the sibling repos and would meaningfully change the LLM-OS implementation if treated as first-class. The design doc treats `skillos` as "user-space" and skips most of its internals; several of those internals are exactly what the LLM-OS needs.
3. **Plan/disk drift** — the README claims artifacts (`runtime/`, `scripts/validate_grammar.sh`) that don't exist on disk. CONTINUATION_PLAN already flags this; refinement keeps the same posture but tightens what to do first.
4. **Trajectory framing** — the benchmark data the prompt cites is correctly summarized in design §6. The refinement is in *which* extrapolation is load-bearing and where to insert a re-baseline checkpoint.

This document does not supersede the design — it amends it.

---

## 1. Corrigendum (factual corrections)

### 1.1 SkillOS

| Design claim | Reality | Fix |
|---|---|---|
| `qwen_runtime.py` as the lightweight path | File does not exist by that name. The runtime is `agent_runtime.py` (multi-provider: Qwen via OpenRouter, Gemini, Gemma 4 via Ollama, OpenAI-compatible endpoints). `QwenRuntime = AgentRuntime` is an alias in `agent_runtime.py:1722` for backward compatibility. | Rename references in design §1.1. The "lightweight Qwen path" framing is correct; the filename is not. |
| "No formalized syscall interface" | Partially wrong. `system/skills/orchestration/core/claude-code-tool-map.md` (173 lines) is a per-tool spec with cost envelope, latency SLA, error modes, retry strategy, and a decision matrix. It is not a wire protocol but it is a syscall *taxonomy*. | Update design §1.1: there *is* a formalized tool surface, just not a token-level ABI. The LLM-OS ISA can borrow the cost/latency/error-mode columns directly. |

### 1.2 SkillOS-mini

| Design claim | Reality | Fix |
|---|---|---|
| "Deterministic Python validators" inside cartridges | Validators are **TypeScript** ports living in `mobile/src/lib/cartridge/validators_builtin.ts:29-123`. The `.py` filenames in each `cartridge.yaml` are source-of-truth labels for parity with the Python `skillos`; the mobile runner cannot exec Python. Lookup is by name in a `BUILTIN_VALIDATORS` registry (`validators_builtin.ts:127-132`). | Update design §1.2. v0.01 cartridge port (Week 3) should target the registry shape, not Python. |
| Cooking + residential-electrical + learn cartridges with IEC 60364 compliance | Confirmed. `mobile/src/cartridges/{cooking,residential-electrical,learn}/cartridge.yaml`. IEC reference is in residential-electrical's description; schemas enforce breaker amperage enums (`circuits.schema.json:13`) and wire gauges. | No change — design §1.2 is correct on this. |
| `<produces>{...}</produces>` contract with retry | Confirmed. `runner.ts:123` defines `PRODUCES_RE`; retry-with-feedback is in `runner.ts:208-227, 790-796`. **Single retry per step**, not unlimited; failure escalates via tier router (`routing.ts:11-20`). | Note the single-retry constraint in design §1.2 — it shapes the `<\|fault\|>` design (cloud fallback fires after one local retry, not after every failure). |

### 1.3 RoClaw

| Design claim | Reality | Fix |
|---|---|---|
| **13-opcode bytecode ISA** | **14 opcodes** in v1.1: 0x01–0x0B (motion + status + speed + step variants), 0x10 (LED_SET), **0xFD (ACK)**, 0xFE (RESET). Defined in `src/2_qwen_cerebellum/bytecode_compiler.ts:20-34`. | Update everywhere in design + plan + spec that says "13 opcodes" for RoClaw. The LLM-OS ISA's "13 opcodes" coincidence is now exactly that — a coincidence, not a parallel. |
| LLM emits raw hex directly | **Backwards.** Primary path: Gemini Robotics-ER 1.6 emits **structured tool calls**; the `BytecodeCompiler` translates client-side to 6-byte frames. Raw-hex emission is a *fallback* path with a GBNF grammar (`bytecode_compiler.ts:647-649`) used when running against Ollama with grammar-constrained decoding. There are **four compilation modes**: tool-call, grammar, few-shot, host-text. | Material for design §1.3 and §3.1. The "VLM emits the ISA" claim is true only in mode 2. The LLM-OS thesis still holds — but the proof point is "we have a working GBNF that reliably produces the bytecode form when the model supports it" rather than "we have a model that natively emits hex." |
| Dreaming Engine = tracing-JIT analogue that hoists hot loops to microcode | Half right. Two implementations: `npm run dream:v1` (sliding-window opcode RLE pattern detection, ≥3 occurrences ⇒ promotion) and `npm run dream:loop` (3-phase: baseline → LLM consolidation → post-dream eval). Promoted artifacts are **markdown strategy files** in `src/3_llmunix_memory/strategies/level_{1..4}_*/`, not microcode blobs. | The JIT analogy is fine for §1.3's narrative but should be qualified: it's pattern abstraction → strategy text, not code-gen. The LLM-OS equivalent is closer to "promote a frequent `<\|loop\|>` body to a cartridge method" than to "compile it to a hot opcode." |
| `<\|loop\|>` / `<\|break\|>` is a formalization of RoClaw's Navigation Chain of Thought pattern | Overstated. RoClaw uses telemetry injection + entropy-based stuck detection (`vision_loop.ts:101-104`: 12-opcode window with <0.5 entropy ⇒ halt) + 45s step timeout. There is no `<\|loop\|>`/`<\|break\|>` in the repo. | The closed-loop primitive is **net new** to LLM-OS, not a formalization of existing code. Design §4.8's claim still stands as a design choice — but it should be framed as "RoClaw demonstrates that VLM-controlled loops work; LLM-OS proposes to make the entry/exit explicit in the ISA" rather than "we are formalizing what RoClaw already does." |

### 1.4 LLM-OS itself (this repo)

The CONTINUATION_PLAN's Section 0 already calls these out, but for completeness:

- README claims `runtime/` exists; directory does not exist.
- README references `scripts/validate_grammar.sh`; `scripts/` does not exist.
- The grammar's `safe-text ::= [^<]*` rule (in `isa.gbnf:85`) is the exact "result-block too permissive" risk flagged in CONTINUATION_PLAN R8 — confirmed by inspection. Result blocks accept any text without `<`, which means malformed JSON, partial opcodes with mistyped prefixes, or stray data all parse. This is the most important grammar bug to fix in Week 1.

---

## 2. Missed primitives (high-leverage additions)

These are capabilities that already exist in the sibling repos and that the design doc either fails to mention or treats as out of scope. Each is high enough leverage that v0.01 should plan around it explicitly.

### 2.1 SkillOS — `claude-code-tool-map.md` as the syscall taxonomy

`system/skills/orchestration/core/claude-code-tool-map.md` defines, per tool: cost envelope (e.g. Task: $0.01–$0.50), latency SLA, error modes (`AGENT_NOT_FOUND`, `CONTEXT_INSUFFICIENT`), retry strategy, and a decision matrix for tool selection. **The LLM-OS ISA spec is missing this column.** The opcode table in `grammar/isa-spec.md` lists arity, form, and daemon response, but not cost, latency, or error mode. Add it: every opcode entry should specify the side-effect cost on Pi 5 (tokens emitted, ms wall-time), the failure mode (which is reflected in `<|fault|>` payloads), and the retry policy the daemon uses on transient failure.

### 2.2 SkillOS — hierarchical skill tree + lazy loading (61% token reduction)

`system/skills/SkillIndex.md` (50 lines) → domain index (30–60 lines) → skill manifest (15 lines) → full spec (250–330 lines). The audit reports a 61% token reduction during the routing phase versus a flat registry. **The LLM-OS cartridge model should adopt this verbatim.** Today's `/cart/<name>/manifest.json` is a flat registry. Replace with `/cart/index.md` → `/cart/<domain>/index.md` → `/cart/<domain>/<name>/manifest.json` → `/cart/<domain>/<name>/full.md`. On Pi 5 with an 8K-effective context window, this is the difference between "kernel + 2 cartridges fit" and "kernel + 8 cartridges fit."

### 2.3 SkillOS — dialect framework (-51% to -97% token compression)

`system/skills/dialects/` contains 14 domain-specific notations: `strict-patch` (code diffs, -97.5%), `formal-proof` (-51%), `system-dynamics` (-61%). On Pi 5 at 8 Hz, every token saved is a millisecond of latency back. **The design doc never mentions dialects.** v0.1 should compile per-cartridge dialects: a `roclaw` cartridge whose `<|call|>` arg is `F 150 150` instead of `{"left":150,"right":150}` is a 4× compression at the wire level, and the daemon expands it before tokenizing for the cartridge runtime.

### 2.4 SkillOS-mini — `tool_parser.ts` with five tool-call shapes + JSON repair

`mobile/src/lib/cartridge/tool_parser.ts` parses five distinct tool-call shapes (A/A2/B/C/D) with JSON repair fallback for unescaped quotes. This is what real models actually emit when asked to call tools without grammar constraints. **For v0.01, the I/O daemon's `<|call|>` parser should embed this directly** — bootstrap-mode Qwen 3 2B Instruct will not always emit the design's `<|call|>cart.method <json> <|/call|>` form even with the grammar (multi-token opcodes have prefix-attention failure modes), and a tolerant parser is a defense-in-depth layer below the grammar. Port this file to Rust as a sidecar in `iod.rs`.

### 2.5 SkillOS-mini — tier system (`cheap`/`capable` + `preferred_tier`)

`mobile/src/lib/cartridge/types.ts:6` and `routing.ts` give cartridges a routing hint (`preferred_tier: "local" | "cloud" | "auto"`) and agents a difficulty hint (`tier: "cheap" | "capable"`). The router escalates after one local retry failure. **This is a second cloud-fallback mechanism the design doc misses.** Design §4.5 frames cloud fallback as `<|fault|>`-only — i.e. the LLM has to explicitly raise an exception. The skillos_mini pattern is *proactive*: some calls are routed to the cloud from the start because the cartridge declared `preferred_tier: cloud`. Add a `tier` field to the cartridge manifest spec; the I/O daemon picks the model per-`<|call|>` based on tier × current capability.

### 2.6 SkillOS-mini — JS-skills cartridge type + `__skillos.llm` proxy

`mobile/src/lib/cartridge/runner.ts:184` injects an LLM proxy into iframe-sandboxed JS skills, so a "skill" can recursively call the LLM via `__skillos.llm.chat()`. This is exactly the **hypervisor-running-untrusted-guest-code** pattern the design doc gestures at. For LLM-OS v0.5, this is the model for **userspace cartridges that themselves run small inference loops** — a "spell-check" cartridge that loads its own tiny model, gets a `<|call|>` from the kernel, runs a sub-inference, and returns. v0.01 doesn't need it, but the manifest schema should reserve a `subagent: true` field so v0.5 doesn't need a breaking change.

### 2.7 SkillOS-mini — post-flow validators (cross-blackboard invariants)

`runner.ts:249-254` runs validators *after* the entire flow completes, cross-referencing multiple blackboard entries (e.g. `menuComplete` checks both `weekly_menu` and `recipes` exist and reference each other consistently). **The design's `<|halt|>status=success` is too thin.** A model can `<|halt|>` with `success` while the actual task invariants are violated. Add a `post_validators` field to the cartridge manifest; the I/O daemon runs them on `<|halt|>` before declaring the task complete. If they fail, the daemon re-prompts the model with the failure as a `<|result|>` and the model decides whether to continue (re-enter a loop) or `<|halt|>partial`.

### 2.8 RoClaw — GBNF grammar for hex emission as the sub-grammar template

`bytecode_compiler.ts:647-649` defines a working GBNF (`root ::= hex-byte " " hex-byte " " hex-byte " " hex-byte " " hex-byte " " hex-byte`). **This is the smallest working ISA-as-GBNF artifact in the entire org** — a 4-line grammar that constrains a real model on real hardware to emit a real ISA. v0.01 should reuse it as the sub-grammar inside `<|call|>roclaw.*`. Concretely: when the cartridge router dispatches a `<|call|>roclaw.forward`, the daemon swaps in a per-call grammar that requires the result to be six hex bytes, and the cartridge's job becomes "emit those six bytes over UDP." This proves the ISA-as-GBNF thesis end-to-end on a path that already works in production code.

### 2.9 SkillOS-mini — `compactor.ts` already implements two modes

`mobile/src/lib/llm/compactor.ts` is TypeScript, has both FIFO and LLM modes, embeds a 16-model context-window catalog, and triggers at 70% utilization. Week 6's "promote compactor to swap daemon" should port from `mobile/src/lib/llm/compactor.ts` (TS), not from any Python file. The 70% threshold is more conservative than the design's 75%; keep 70% — it gives the swap LLM call time to complete on Pi 5 before context overflows.

---

## 3. Sharpened 6-week sprint

The CONTINUATION_PLAN's week breakdown is sound. These are the deltas.

### Week 1 — Mechanically verify the grammar (unchanged) + tighten `safe-text`

In addition to G1/G2/T1: **fix `safe-text ::= [^<]*` before validating fixtures.** The current rule means illegal fixture #4 ("call without result-block") may pass because the next opcode token's text is consumed as result content. Replacement: define `safe-text` as either a JSON value or a quoted string, so a stray opcode prefix breaks parsing. This is CONTINUATION_PLAN R8 made concrete.

### Week 2 — Bootloader + I/O daemon (B1, D1) + `tool_parser.rs` sidecar

Add deliverable **D2 — `runtime/tool_parser.rs`**: port `mobile/src/lib/cartridge/tool_parser.ts` to Rust (~200 lines). Used as a fallback `<|call|>` arg parser when grammar-constrained output diverges in shape. Acceptance: 5 fixture inputs (one per shape A/A2/B/C/D) all extract the same `(name, args)` tuple. No dependency on grammar validation; runs in parallel with B1.

### Week 3 — Cartridge adapter (port from skillos_mini, not Python)

Two corrections to C2:
1. **Source path is `mobile/src/cartridges/`**, not `cartridges/` at repo root. The mobile prefix is load-bearing.
2. **Validators ship as Rust functions registered in a `BUILTIN_VALIDATORS` map**, mirroring the TS shape. Don't re-implement in Python; that's a regression from skillos_mini's mobile design.

Add C4 (new): **adopt the hierarchical layout from §2.2** — `/cart/index.md` + per-domain index files. Even with three cartridges, this proves the lazy-loading shape so v0.1 can scale to 8+ without rework.

### Week 4 — Closed-loop validation + post-flow validators

L1's acceptance criterion currently says "10 consecutive runs all `<|halt|>status=success`." Tighten: success means `<|halt|>status=success` **and** the post-flow validator (per §2.7) confirms the goal predicate. This catches the failure mode where the model declares success without achieving it.

### Week 5 — RoClaw integration (reuse, don't reinvent)

R1 should use the **GBNF + tool-call dual path from §2.8**, not invent a new wire format:
1. `<|call|>roclaw.forward {"left":150,"right":150}<|/call|>` enters the daemon.
2. Daemon dispatches to `/cart/roclaw/`, which **runs the existing `BytecodeCompiler`** (ported from `RoClaw/src/2_qwen_cerebellum/bytecode_compiler.ts`).
3. Result: 6-byte frame on UDP port 4210 (RoClaw's actual port, confirmed in `udp_transmitter.ts:48`).

The skillos `roclaw_bridge.py` referenced in the design and CONTINUATION_PLAN R4 should be deprecated in favor of porting `BytecodeCompiler` directly — the Python bridge is a third copy of code that already exists in canonical form in RoClaw.

### Week 6 — Cloud fallback + compactor + tag (refined)

S1's compactor port: source is `mobile/src/lib/llm/compactor.ts`, threshold 70% (not 75%), implement both FIFO and LLM modes, ship FIFO as default for v0.01 (deterministic + fast), make LLM mode opt-in for tasks that lose state badly under FIFO.

F1's cloud fallback: the OpenAI-compatible-endpoint decision is correct. Add: the decision of *whether* to fall back is no longer just `<|fault|>`-driven — it also fires proactively per §2.5 tier routing. v0.01 wires the proactive path as a stub; full implementation lands v0.1 alongside per-cartridge tier policy.

---

## 4. Updated risk register

Carrying forward CONTINUATION_PLAN R1–R8 with these changes:

| # | Change |
|---|---|
| **R8** (safe-text permissiveness) | **Severity raised from medium→high** and **mitigation pulled into Week 1** — the audit confirmed the rule is too loose in `isa.gbnf:85`. Fix before any validation runs. |
| **R4** (roclaw_bridge.py drift) | **Mitigation changed**: don't audit drift, *deprecate the bridge*. Port `BytecodeCompiler` from `RoClaw/src/2_qwen_cerebellum/bytecode_compiler.ts` as the canonical implementation. Eliminates the drift class entirely. |
| **R7** (METR extrapolation wrong by v1.0) | **Re-baseline checkpoint moved earlier**: from v0.5 to v0.1 (Month 2). The v0.5 self-hosting milestone depends on small-on-device tracking frontier-minus-one; if v0.1 traces show the kernel-fine-tuned 2B is closer to frontier-minus-two, slip v0.5 to Month 9 and tell people now. |

Three new risks the audit surfaced:

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R9 | `<|call|>` grammar shape diverges from the five tool-call shapes (A/A2/B/C/D) real models actually emit. Bootstrap-mode Qwen 3 2B Instruct emits shape-D unreliably under multi-token-opcode prefix attention. | Medium | High | §2.4 — port `tool_parser.ts` to Rust as a fallback parser below the grammar. Don't trust the grammar alone for v0.01. Tighten in v0.1 once opcodes are single tokens. |
| R10 | Token cost per syscall is 2× higher than necessary because `<|call|>` args use raw JSON instead of dialect-compressed forms. Effective Pi 5 throughput halved. | High | Medium | §2.3 — defer to v0.1; document the headroom in the v0.01 release notes so "8 Hz" doesn't get cited as a hard ceiling. |
| R11 | Post-flow goal validation gap: model `<|halt|>status=success` while task invariants are violated. v0.01 acceptance test passes; real users see the OS lying about success. | Medium | High | §2.7 — bake post_validators into the cartridge manifest spec from v0.01; require at least one for each new cartridge in code review. |

---

## 5. Decision log additions

To CONTINUATION_PLAN §3:

| Decision | Resolution | Justification |
|---|---|---|
| Validator implementation language for v0.01 | **Rust functions in a `BUILTIN_VALIDATORS` registry**, not Python | §1.2 + §3 Week 3 — mirrors the TS pattern in `validators_builtin.ts`; eliminates the Python runtime from the Pi 5 footprint; matches design §4.1's "no Python at runtime" target. |
| RoClaw bridge implementation source | **Port `BytecodeCompiler` from RoClaw, deprecate `skillos/roclaw_bridge.py`** | §2.8 + §3 Week 5 — single canonical source; reuses the working GBNF; proves the ISA-as-GBNF thesis on a path that already works. |
| `safe-text` grammar tightness | **Tighten to JSON-or-quoted-string in v0.01, not v0.1** | §1.4 — the audit confirmed the bug is real; the fix is small; deferring it means every Week 2+ component is built on a permissive contract. |
| Cartridge layout shape | **Hierarchical (index → domain → cartridge → spec) from v0.01** | §2.2 — even at 3 cartridges the layout is the test; rework cost is low now, prohibitive after v0.1 ships. |
| Re-baseline of capability projections | **Month 2 (v0.1), not Month 6 (v0.5)** | §4 R7 — earlier signal is cheaper; v0.5 plans depend on the projection holding. |

---

## 6. Trajectory note (what the data actually argues)

The benchmark numbers cited in the prompt and design §6 are correctly summarized. The refinement is in which numbers carry weight for which milestone:

- **v0.01 (Month 1)** does not depend on capability projections. It depends on llama.cpp's grammar engine working as documented today, and on Qwen 3 2B Instruct being able to emit the bootstrap ISA reliably enough to run a 10-minute fixture. Both are present-day questions.
- **v0.1 (Month 2)** depends on fine-tuning a 2B model adding 18 special tokens and improving syscall throughput 5–10×. This is well within demonstrated 2024–2026 fine-tuning practice (DPO/GRPO with reward = grammar validity + task success; cf. RLSF lineage).
- **v0.5 (Month 6)** is where the trajectory matters. The claim is that a 2B-class Pi 5 model will handle 1–2 hour task horizons, derived from METR's 131-day doubling on cloud frontier minus a 12–18 month small-model lag. **If RULER-style effective-context measurements continue to show 50–65% of advertised context is usable, the per-task token budget is half what naive math says.** Plan accordingly: v0.5 acceptance tests should run with `ctx_size=8192` and assume effective ≈ 5K, not 16K assumed-effective.
- **v1.0 (Month 12)** is the Linux-1.0 parallel and a sales pitch. METR's confidence intervals are wide at 12 months out. Treat as directional.

The METR 4-month doubling is the single most load-bearing extrapolation in the design. If it slows to the 7-month historical rate, v0.5 slips to Month 9 and the Linux-1.0-in-30-months parallel breaks. Re-baseline at v0.1 (per R7 above) so this is not discovered at Month 5.

---

## 7. The one-paragraph version (revised)

Three sibling repos already contain every primitive an LLM-OS needs: `skillos` provides the syscall taxonomy (`claude-code-tool-map.md`), the hierarchical lazy-loaded skill tree (61% routing-token reduction), and the dialect compression framework (51–97% per-arg savings); `skillos_mini` provides a typed blackboard, a tolerant tool-call parser handling five emission shapes with JSON repair, schema-validated retry with one-shot tier escalation, two-mode LLM-aware compaction, and a cartridge-runner pattern with post-flow cross-blackboard validators; `RoClaw` provides a working GBNF-constrained bytecode emission path (14 opcodes, 6-byte frames, UDP port 4210) where the LLM emits structured tool calls that a client-side compiler turns into hex. The LLM-OS v0.01 sprint is six weeks of *assembly*, not invention: validate the GBNF-as-ISA grammar, fork llama.cpp's server with it, port the existing parsers and validators to Rust, and reuse RoClaw's `BytecodeCompiler` as the `/cart/roclaw/` body. The original design's `<|loop|>`/`<|break|>` closed-loop primitive is genuinely new; everything else is a port. The 4-month METR doubling is the trajectory bet; re-baseline it at v0.1 (Month 2), not v0.5 (Month 6), because the 6-month and 12-month milestones depend on the small-on-device lag staying inside 12–18 months of frontier.

---

*Audited against `C:/evolvingagents/{skillos,skillos_mini,RoClaw,llm_os}` on 2026-04-24. File and line references are accurate as of that date. All "missed primitive" items are in production code in the cited repos; none are speculative.*
