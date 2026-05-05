# LLM-OS on llama.cpp: A Design Grounded in SkillOS, SkillOS-mini, and skillos_robot

*For Matias Molinas — EvolvingAgentsLabs — April 2026*

---

## 0. Executive summary

You have three projects that, read together, are a single thesis being assembled in pieces.

- **SkillOS** treats the LLM as a user-space interpreter over markdown agents and tools — it is an *application framework* running on top of Claude Code.
- **SkillOS-mini** pushes the same substrate to the edge: wllama in the browser, LiteRT on Android, cartridges (schemas + validators) as sealed device drivers, and compaction as swap.
- **skillos_robot** already crosses the final abstraction boundary you are reaching for: a VLM emits a **13-opcode bytecode ISA** as raw hex (`AA 01 64 64 CB FF`) and the firmware executes it with one `memcpy`. That is not a framework — that is an instruction set architecture.

The question your prompt is really asking is: **what happens if we stop treating the LLM as a program running inside an OS, and start treating it as the OS itself — with llama.cpp as the CPU, the KV cache as RAM, special tokens as the ISA, and the sampler loop as the fetch-decode-execute cycle?**

This document lays out that design, grounded in (a) what llama.cpp can already do today — GBNF grammars with token-level constraints, logit bias, JSON-schema-to-grammar, tool templates — (b) what the Raspberry Pi 5 can physically run in April 2026, and (c) the measured capability trajectory (METR 4-month doubling, Gemma 4 E2B at 2.3B effective, Kimi K2.5 at 99 HumanEval) that tells us what the same design will do in 1, 2, 6, and 12 months.

---

## 1. What you already built (and why the next move is obvious)

### 1.1 SkillOS (desktop)

- Markdown-defined agents with frontmatter (`name`, `description`, `tools`) — agents are *data*, not code.
- A SystemAgent orchestrator that discovers agents, picks one per task, and delegates.
- `qwen_runtime.py` as a lightweight path (Qwen 3 4B via OpenRouter or local Ollama).
- `skillos.py` REPL that wraps Claude Code and hides its internals.

**What this really is:** a user-space POSIX-like abstraction where `agent_name` is the binary name and `tools: Read, Write, Bash` is the `#!/usr/bin/env` declaration. The LLM is the shell *and* the CPU, but there is no formalized syscall interface — the LLM talks to tools in whatever format the harness accepts.

### 1.2 SkillOS-mini (mobile + small local LLMs)

This one is much closer to an OS. Reading the repo layout:

- **Blackboard** — classic AI blackboard pattern, which is shared-memory IPC with a subscription model.
- **CartridgeRegistry + CartridgeRunner** — cartridges are sealed per-domain bundles: agents + JSON Schemas + deterministic Python validators. This is exactly a **loadable device driver model** (cooking driver, residential-electrical driver with IEC 60364 compliance, learn driver with persistent knowledge).
- **`<produces>{...}</produces>` contract with schema-validated retry** — this is a **syscall return-value protocol with exception handling.** The model emits a structured "system call"; the validator checks it; on failure the runtime retries with the error injected. That is protected mode.
- **LLM-powered compaction** — this is swap/paging for context windows too small to hold full history.
- **Local-first with cloud fallback on validation failure** — this is software-level fault tolerance, equivalent to the x86 `#UD` trap that used to fall back to software emulation on 386s.
- **wllama (WASM) + LiteRT-LM + iframe sandbox for Gallery skills** — this is a hypervisor running untrusted guest code with the LLM as host.

You have built most of an OS without calling it one. The missing pieces are (a) a formal ISA, (b) a boot sequence that is just llama.cpp + a GGUF, and (c) the closed-loop execution model where the LLM decides when to reinject its own output.

### 1.3 skillos_robot (the piece that makes it real)

skillos_robot is your proof that an LLM can emit an ISA directly:

> `JSON (58 bytes): {"cmd":"move_cm","left_cm":10,"right_cm":10,"speed":500}`
> `Bytecode (6 bytes): AA 01 64 64 CB FF`

6 bytes. One `memcpy`. ~0.1ms parse vs ~15ms JSON. Thirteen opcodes. Frame start `0xAA`, frame end `0xFF`. This is a hardware ISA for a robot, and the VLM has already been shown to generate it reliably.

The Dual-Brain architecture (Cortex for strategy, Cerebellum for reflex, LLMunix Memory for the hippocampus) is also the right coarse pattern for an LLM-OS: a **slow, large, high-capability model** setting goals into a **fast, small, ISA-tuned model** that actually emits the opcode stream. That maps cleanly onto the two Raspberry Pi deployment modes below.

The Dreaming Engine (`npm run dream` — extract recurring motor patterns from traces and promote them to skills) is the piece that makes this self-improving: it is the equivalent of a tracing JIT that hoists hot loops into microcode.

---

## 2. Historical anchor: what DOS, Linux, and Windows actually teach us

The prompt asks for this explicitly, and the lesson is specific.

**MS-DOS (1981–1995).** Single-tasking, real mode, no memory protection, 640KB limit, programs talked to hardware directly via interrupts. Shipped because it was *good enough for one program at a time* and because it was distributed with hardware. Most DOS software was ~1 layer of abstraction thick. **Analogy for LLM-OS:** SkillOS today. One agent does one task, the harness is thin, there is no isolation between "processes."

**Linux 0.01 (17 Sep 1991).** ~10,000 lines of C, would not compile without Minix (its predecessor-competitor), no networking, supported one filesystem. Torvalds' own words: *"just a hobby, won't be big and professional like GNU."* The crucial property: **it bootstrapped on top of an existing OS and then escaped it.** By 0.11 (Dec 1991) it was self-hosting. By 1.0 (Mar 1994) — 176K LoC — it was production. That is 30 months from "needs Minix to compile" to "runs the internet."

**Windows 1.0 → 3.1 → 95 → NT.** The important transition is 3.1 → 95: *cooperative multitasking to preemptive multitasking.* In cooperative, every program had to voluntarily yield to the scheduler. If one program hung, the whole machine hung. In preemptive, the kernel interrupts programs on a timer. **Analogy for LLM-OS:** today's agent frameworks are cooperative — an agent runs to completion and hands control back. You need a *preemptive* model where the OS interrupts the LLM when I/O is ready, budget is blown, or a higher-priority task arrives. The sampler loop is the timer interrupt.

The historical pattern is consistent:

1. **Minimal bootstrap on top of an existing platform** (DOS on BIOS, Linux on Minix, your LLM-OS on llama.cpp + the existing OS it is compiled for).
2. **Formalize the syscall interface early.** POSIX existed before Linux; Torvalds was trying to obtain POSIX docs on Usenet before writing 0.01. *You do not invent the ABI as you go — you declare it.*
3. **Self-hosting is the milestone that matters.** When the LLM-OS can be used to fine-tune itself on its own execution traces, it has escaped the parent.
4. **Isolation/protection comes later than you think.** DOS had none. Linux 0.01 had basic paging and nothing else. Today's LLM sandboxes (your iframe/validator layer) are already ahead of early Linux on this axis.

Do not try to build NT on day one. Build Linux 0.01 in a month.

---

## 3. The llama.cpp substrate: what is actually available

You asked for "boots with a llama.cpp." Here is precisely what llama.cpp (current master, April 2026) gives you for free, with direct relevance to an LLM-OS.

### 3.1 Token-level output constraints (the ISA enforcer)

**GBNF grammars** can reference tokens by ID or by string:

```
# Match a thinking block bounded by actual tokenizer tokens
root ::= <[1000]> thinking <[1001]> .*
thinking ::= !<[1001]>*
```

This is the critical primitive. You define the LLM-OS ISA as a GBNF grammar over real tokenizer tokens — not as a post-hoc JSON parser. The sampler physically cannot emit a token outside the grammar. This is hardware-level type safety at decode time.

**Concrete example from the field.** The gpt-oss release uses exactly this structure:

```
<|channel|>analysis<|message|> ...CoT thinking... <|end|>
<|start|>assistant<|channel|>final<|message|> ...answer... <|return|>
```

Those are real special tokens in the tokenizer. A GBNF grammar over those tokens *is* the harmony protocol. For your LLM-OS, this means you can define opcodes as single tokens (`<|syscall|>`, `<|read|>`, `<|write|>`, `<|fork|>`, `<|halt|>`) and the grammar decides legal sequences.

### 3.2 JSON-Schema → GBNF

llama.cpp ships `common/json-schema-to-grammar.cpp`. You hand it a JSON schema; it emits the corresponding GBNF. This is exactly the pattern your cartridges already use (JSON Schema per cartridge) — you can compile those same schemas directly to decode-time constraints on a local model, eliminating the retry-on-validation-failure path for well-typed outputs.

### 3.3 Logit bias (the privilege check)

`--logit-bias 15043-1` reduces token probability; `[[15043,false]]` bans a token entirely. This is how you implement **capability-based security at the decode level**: a cartridge that is not authorized to emit `<|bash|>` gets that token banned in its sampler config. The model literally cannot call it.

### 3.4 Server-side primitives

- `cache_prompt: true` — KV cache reuse across requests, so the "loaded OS state" persists.
- `response_format: {type: "json_schema", ...}` — OpenAI-compatible schema mode.
- `logit_bias` per-request — lets you change the privilege set mid-session.
- Context checkpoints during prefill — crash recovery primitives.
- Samplers pipeline (`dry, top_k, typ_p, top_p, min_p, xtc, temperature`) reorderable — this is your scheduler policy.

### 3.5 Reasoning format parsing

`--reasoning-format none` + parse `<|channel|>analysis|final|...|>` yourself. You get raw token output. This matters because the LLM-OS needs *every* token, not a pre-parsed chat message.

---

## 4. Design: the LLM-OS

The concrete architecture. I will present it in the same order you would read a computer architecture textbook: boot, CPU, ISA, memory, I/O, scheduling, filesystem.

### 4.1 Boot sequence

```
[bootloader.c: ~150 lines]
  1. fork() llama.cpp server with:
       - model=llm-os-kernel.gguf (Gemma 4 E2B or Qwen 3 2B initially)
       - grammar=isa.gbnf
       - n-ctx=32768
       - cache-prompt=true
  2. Open FIFOs: /tmp/llmos.in, /tmp/llmos.out
  3. Inject boot prompt: "You are LLM-OS v0.01. Your ISA is defined by the
     grammar. On startup, emit <|ready|>. Then read from <|stdin|> tokens."
  4. Wait for <|ready|> on /tmp/llmos.out
  5. Spawn I/O daemon.
  6. exit; the running llama.cpp + I/O daemon *is* the OS.
```

Total footprint on a Pi 5 8GB: llama.cpp binary (~15MB), kernel GGUF (~1.5GB for Gemma 4 E2B Q4), ISA grammar (~2KB), I/O daemon (~500 lines of C or Rust). No Python runtime required at runtime.

### 4.2 The CPU (llama.cpp sampling loop)

The fetch-decode-execute cycle is one sampler iteration:

- **Fetch**: compute logits over vocabulary given current KV cache.
- **Decode**: apply grammar mask (disallow ungrammatical tokens) + logit bias (disallow unauthorized tokens) + sampler chain.
- **Execute**: sampled token is appended to KV cache. If the token is a syscall opcode, the I/O daemon is notified and executes the side effect.

Clock rate is tokens/sec. On a Pi 5 with a 3B Q4 model, this is 4–12 Hz. Karpathy estimated 10–20 Hz in his LLM-OS talk in 2023; the Pi 5 is in that ballpark today. For reference a cloud model on GPT-5.4 is ~60–200 Hz for the same role.

### 4.3 The ISA

Thirteen or so opcodes. Every opcode is a **single special token** added to the tokenizer (a tokenizer rebuild is required for the fine-tuned kernel model; for the bootstrap model you can use two-token sequences like `<|op|>READ<|/op|>` that are deterministically parsed).

| Token | Name | Semantics |
|---|---|---|
| `<|read|>` | READ | Followed by a file descriptor token and a length token; returns bytes in the KV cache as the result stream. |
| `<|write|>` | WRITE | Followed by fd + payload (arbitrary grammar-valid JSON). |
| `<|call|>` | SYSCALL | Followed by a cartridge name (from a closed enum) and a schema-validated JSON arg object. |
| `<|yield|>` | YIELD | Returns control to scheduler; suspends this task. |
| `<|fork|>` | FORK | Spawn a subtask with a new KV cache branch and a goal payload. |
| `<|wait|>` | WAIT | Block on an fd. Scheduler swaps this task out. |
| `<|loop|>` | LOOP | Begin a closed-loop region. The model explicitly opts into reinjecting its own output. |
| `<|break|>` | BREAK | Exit the innermost loop. Required before `<|halt|>` terminates cleanly. |
| `<|halt|>` | HALT | Task done; emit final result to fd 1. |
| `<|think|>` | THINK | Inner monologue; contents are not copied to the output fd, but remain in KV. |
| `<|commit|>` | COMMIT | Materialize think-block into persistent memory (cartridge `memory`). |
| `<|fault|>` | FAULT | Raise an exception with a typed error payload. Invokes the handler cartridge. |
| `<|policy|>` | POLICY | Query the current permission set; emitted result updates the logit bias for subsequent tokens. |

The grammar enforces legal sequences: e.g., `<|read|>` must be followed by a digit token (the fd), then an integer, then the return stream must be consumed before the next opcode. `<|loop|>` must be closed by a matching `<|break|>`. `<|halt|>` is only legal at loop depth zero.

**This is the key design choice that makes closed loops controllable.** The LLM decides when it wants to operate in a feedback loop by emitting `<|loop|>`. Inside the loop, the I/O daemon reinjects the emitted tokens plus any external input back into the prompt at the next sampler step. The LLM decides when to exit the loop by emitting `<|break|>`. No external logic has to infer when to keep the loop running — the model's autonomy is *typed into the ISA*.

### 4.4 Memory model

- **Registers** = the last N tokens of KV cache. The model can reference them implicitly.
- **RAM** = the full KV cache (~128K tokens on a decent quantization at 8GB RAM).
- **Swap** = the compactor from SkillOS-mini, which is already LLM-powered context compaction. Promote it to kernel-level: the scheduler triggers it when KV is 75% full.
- **Disk** = cartridge-resident JSON files. Accessed via `<|read|>` / `<|write|>` syscalls with fd > 2.
- **L1 cache** = the prompt prefix (system prompt + ISA grammar + currently-loaded cartridge descriptors), pinned in KV and never evicted. llama.cpp `cache_prompt: true` gives you this for free across requests.

### 4.5 I/O daemon (the MMU and device layer)

A ~500-line C or Rust program. It watches llama.cpp's streaming output for ISA opcode tokens. On each opcode:

1. Validate the opcode + its arguments against the grammar's state machine (defense in depth — the grammar should have already guaranteed this, but do not trust).
2. Execute the side effect against the real OS (read a file, call an API, write to a robot bus over UDP — your skillos_robot bytecode path is already a template).
3. Tokenize the result and inject it into the KV cache as a new input segment, wrapped in `<|result|> ... <|/result|>`.
4. Resume sampling.

This daemon is also where cloud fallback lives: if the local model emits `<|fault|>` with a `needs_cloud: true` payload, the daemon spawns a request to the cloud model with the compacted state and injects the cloud response as a result stream. Your SkillOS-mini already does this at the cartridge level — promoting it to kernel level is trivial.

### 4.6 Scheduler (preemptive, not cooperative)

The scheduler lives in the I/O daemon. It owns a timer (wall clock) and a token budget per task. It can:

- Preempt a task by injecting a `<|yield|>` at the next grammar-legal position (the grammar must allow `<|yield|>` at most statement boundaries).
- Swap tasks by saving/restoring the KV cache (llama.cpp's context checkpoints are the substrate).
- Enforce quotas via logit bias — a task that has used 80% of its budget gets `<|halt|>` biased toward +5.

This is the Windows 3.1 → 95 jump. Without it, one runaway agent eats the whole machine. With it, you have a real OS.

### 4.7 Filesystem

Cartridges are already your filesystem. Formalize:

- `/cart/<name>/manifest.json` — schema definitions, allowed opcodes (capability set), validators.
- `/cart/<name>/bin/` — optional deterministic helpers (your Python validators, JS skills).
- `/cart/<name>/mem/` — persistent state for this cartridge.
- `/dev/<name>` — physical devices exposed as cartridges (`/dev/roclaw` emits the 6-byte UDP ISA).

Loading a cartridge = injecting its manifest into the prompt prefix + updating the logit bias / grammar to enable its opcodes. Unloading = reverse. This is `insmod` / `rmmod`.

### 4.8 The closed loop (the specific thing you asked about)

The LLM controls when to reinject output as input via `<|loop|>`. Inside the loop:

```
<|loop|>goal=navigate_to_kitchen
  <|think|> I see a hallway. Kitchen is left. <|/think|>
  <|call|>roclaw.turn_left{"degrees":90}<|/call|>
  <|result|>ok<|/result|>                           ← injected by daemon
  <|call|>roclaw.get_camera<|/call|>
  <|result|>{"img":"..."}<|/result|>                ← injected by daemon
  <|think|> I now see the kitchen. <|/think|>
  <|break|>
<|halt|>status=success
```

This is a closed control loop where **the model decides the next action based on the previous result, and the model decides when the loop terminates.** No external agent framework is injecting "Observation:" strings. The ISA is what it was in the tokenizer all along.

This is the skillos_robot Navigation Chain of Thought pattern you already validated — formalized as an OS primitive rather than a navigation-specific prompt template.

---

## 5. Target hardware: Pi 5 reality check

From April 2026 benchmarks:

- **Pi 5 8GB + llama.cpp + OpenBLAS.** Llama 3.2 3B Q4: 4–7 tok/s. Qwen 2.5 1.5B: ~8 tok/s. Gemma 4 E2B (2.3B effective, 1.5GB footprint): 7.6 tok/s on Pi 5 per the official model card. BitNet B1.58 2B 4T: >8 tok/s with sub-GB RAM.
- **TinyLlama 1.1B**: 5–7 tok/s, useful for reflex-latency tasks.
- **7B models**: 0.7–3 tok/s on Pi 5 — too slow for OS work, but fine for the "compaction daemon" that runs in the background.

For LLM-OS on a Pi 5, the sweet spot is **Gemma 4 E2B or Qwen 3 2B** at Q4, giving you ~8 tok/s = 8 Hz. That is fast enough for a cognitive OS whose unit of work is a ~50-token syscall. Each syscall takes ~6 seconds, which is lethal for a desktop but fine for what this actually is: an always-on home assistant, a robot brain, a personal data agent.

For cloud mode, you run the same bootloader against a cloud model (GPT-5.4 Codex, Claude Opus 4.6, Kimi K2.5). You get 60–200 Hz — each syscall takes 250–800ms. That is interactive. Same ISA, same grammar, same cartridges. The only change is the server URL.

**Dual-brain Pi deployment** (following skillos_robot's pattern): Pi 5 runs E2B as cerebellum at 8 Hz for reflex syscalls, and a cloud API call to Kimi K2.5 or Claude Opus at ~100 Hz provides cortex strategy every 10–30 seconds. This matches your existing dual-brain separation and gives you hybrid autonomy — full local when offline, cloud-boosted when connected.

---

## 6. The capability trajectory (the part you asked for with "real data")

These are the numbers that drive the projections.

### 6.1 What the last 24 months actually did

- **MMLU**: frontier saturated at 88–94% by early 2025. No longer differentiates.
- **HumanEval**: saturated; Kimi K2.5 at 99%.
- **GPQA Diamond**: went from ~50% to 85%+ in 18 months.
- **SWE-bench Verified**: GPT-5 at 74.9%; this benchmark was considered unsolvable a year earlier.
- **HLE (Humanity's Last Exam)** — designed to be beyond 2024 AIs: Claude Opus 4.6 hit 53.1% with tools in Feb 2026.
- **METR 50%-time-horizon**: doubled every 7 months over 6 years, accelerated to every ~4 months in 2024–2025. TH1.1 (Jan 2026) measured 131-day doubling since 2023. GPT-5 has a ~2h17min horizon as of early 2026.
- **Context windows**: 128K became standard; 1M is frontier (Gemini 3.1 Pro), 10M advertised (Llama 4 Scout), but RULER shows only ~50–65% of advertised context is effectively usable.
- **Small-model revolution**: Llama 3.2 3B exceeds GPT-3.5 on GSM8K. Gemma 4 E2B (2.3B effective) is natively multimodal. Qwen 3-30B-A3B uses MoE to activate only 3B per token, running on consumer hardware. Open weights hit S-tier (Kimi K2.5, MiniMax M2.5, GLM-5).
- **Tool use / agentic**: BFCL v4, Terminal-Bench, Toolathlon all showed rapid gains. Best agentic model (GPT-5.4 Pro) at 89.3 on BenchLM agentic composite; best open-weight (Holo3-35B-A3B) at 77.8.
- **Grammar-constrained generation in llama.cpp**: matured to include LLGuidance integration, token-level grammar references, and speculative decoding compatibility.
- **The harness became the bottleneck**: 2025 was models, 2026 is harnesses. This is the exact gap your SkillOS-mini cartridge runtime occupies.

### 6.2 Temporal projections

I'm applying the METR 4-month doubling as the backbone, with the caveat that projections compound error fast. Numbers are 50%-reliability horizons on code-like tasks; treat them as rough shape, not exact.

#### 1 month out (mid-May 2026)

- **Achievable with today's llama.cpp + existing models**: a bootstrap prompt-based LLM-OS on Pi 5, using Gemma 4 E2B or Qwen 3 2B, GBNF grammar over two-token opcode sequences (no tokenizer rebuild), ~3–5 of the 13 opcodes implemented (`read`, `write`, `call`, `halt`, `yield`). Closed loops work via `<|loop|>/<|break|>` grammar markers. Syscalls go through your existing cartridge runtime.
- **Time horizon on Pi 5**: ~5–10 minute tasks at 50% reliability (because you can compose ~40–80 syscalls at 6 s each, and the bottleneck is the model's planning, not the substrate).
- **What to build**: the bootloader (~200 lines), the I/O daemon (~500 lines), the GBNF grammar, a minimal kernel manifest. Repurpose 100% of SkillOS-mini's cartridges as-is.
- **What to release**: `llm_os/v0.01` — "requires llama.cpp to compile and run." Exactly Linux 0.01's posture.

#### 2 months out (mid-June 2026)

- **Fine-tune the first kernel model.** Take Qwen 3 2B or Gemma 4 E2B, add the 13 ISA tokens to the tokenizer, and fine-tune on (a) execution traces from the v0.01 bootstrap and (b) synthetic traces of legal opcode sequences. Use GRPO or DPO with reward = successful syscall + task completion (this is Shivance's RLSF idea, which was theoretical in 2023 and is commodity training tech in 2026).
- **Expected gain**: 5–10× reduction in wasted tokens (no need for two-token opcode sequences; opcodes are single tokens). Effective throughput on Pi 5 jumps to ~25 "effective Hz" for syscall-dense workloads.
- **Time horizon**: ~15–30 minute tasks on Pi 5.
- **What to release**: `kernel-gemma4-e2b-isa-v1.gguf` — a 2B-class model specifically post-trained for the LLM-OS ISA. Apache 2.0. Dual-license with AGPL for commercial pieces per your IP strategy.

#### 6 months out (Oct 2026)

- **Multi-tasking**: preemptive scheduler live, KV cache swapping working, two or three cartridges can run concurrently with their capability sets enforced via per-task logit bias.
- **Self-hosting**: the LLM-OS can be used to generate training data for its next kernel version, by running a supervisor cartridge that monitors executions and emits cleaned trajectories. This is the Linux 0.11 moment — the system can rebuild itself.
- **skillos_robot integration**: the LLM-OS bytecode layer drops directly to the skillos_robot `AA 01 64 64 CB FF` layer via the `/dev/roclaw` cartridge. The physical robot becomes a first-class device in the OS, with no additional glue code.
- **Capability (extrapolating METR 4-month doubling from ~2h17min today, which is a cloud-model frontier)**: frontier cloud model horizon ~4–5 hours. Local Pi 5 model horizon (which lags frontier by ~12–18 months in benchmark terms historically): ~1–2 hours.
- **What to release**: `llm_os/v0.5` — equivalent to Linux 0.11, self-hosting, multi-cartridge.

#### 12 months out (April 2027)

- **Small-model tailwind**: Gemma 5, Qwen 4, and their successors — following the established trajectory of 3B-class models crossing capability thresholds held by the prior generation's 10× larger models — should put a 3B on-device model at roughly today's 30B-class capability. That is enough for meaningful software engineering tasks on a Pi 5.
- **Time horizon on Pi 5**: ~4–8 hour tasks at 50% reliability, assuming the trajectory holds. Cloud-mode LLM-OS: 1–3 day tasks.
- **Ecosystem**: if you open-source the ISA as a standard, third-party cartridges appear (this is what happened with MCP in 2024–2025 and is happening with the harness layer now per the 2026 surveys).
- **The interesting risk**: if METR extrapolation holds and compute doesn't hit a wall (see arxiv 2511.19492 for slowdown scenarios), the frontier is emitting usefully autonomous multi-day behavior. At that point the question stops being "can we build an LLM-OS" and starts being "how do we safely run one."
- **What to release**: `llm_os/v1.0` — production-ready. The Linux 1.0 moment. ~30 months from hobby to production.

### 6.3 Projection caveats

- METR's 131-day doubling is measured on code tasks with cloud frontier models. Your Pi-local model will lag. The *shape* of the curve — small on-device getting this year's frontier-minus-one — is well-supported by the Gemma/Qwen/Llama small-model trajectory.
- If there's a compute-scaling slowdown (Edelman/Ho, Sevilla et al. models) the 1-month horizon hits ~7 years later than simple extrapolation. For your 12-month window this matters less than it would for a 5-year projection.
- Benchmarks are contamination-prone (Berkeley RDI 2026 found agentic benchmarks exploitable to near-perfect without solving tasks). Treat horizon numbers as order-of-magnitude.

---

## 7. IP strategy (since you asked in a past conversation)

The LLM-OS concept is not IP-protectable on its own — Karpathy named it in 2023, and "LLM as kernel" is now common vocabulary. What *is* protectable:

- **The specific ISA you define** (the set of opcodes, their semantics, the grammar) — copyright protects the expression, trade secret protects the fine-tuning recipe.
- **The fine-tuned kernel model weights** — copyrightable as a compilation / derivative work.
- **The cartridge manifest format and capability protocol** — copyright + possibly a defensive patent on the capability-enforcement-via-logit-bias technique.
- **The closed-loop execution model** (`<|loop|>` / `<|break|>` with model-decided termination) — this has patent potential because it is genuinely novel; the Huxley-Gödel Machine and Darwin-Gödel Machine work is adjacent but does not frame the primitive this way.

Given your pattern of releasing concepts early, the move is: provisional patent on `<|loop|>`-style model-controlled closed-loop execution + open core (ISA and base cartridges Apache 2.0, kernel fine-tuning recipes and enterprise cartridges AGPL or commercial).

---

## 8. What to build first, concretely

A proposed 6-week sprint for v0.01, leaning hard on what SkillOS-mini already has:

**Week 1 — ISA definition + GBNF grammar.** Write `isa.gbnf`. Target the Qwen 3 2B tokenizer first (widely available, stable special tokens). Define the 13 opcodes as two-token sequences for now; defer tokenizer rebuild until fine-tuning. Validate the grammar against `llama-gbnf-validator`.

**Week 2 — bootloader + I/O daemon.** Write `bootloader.c` (~200 lines) and `iod.rs` (~500 lines). Use FIFOs for transport. Drive llama.cpp's server mode with `--grammar isa.gbnf --reasoning-format none --cache-prompt true`.

**Week 3 — cartridge adapter.** Port three SkillOS-mini cartridges (`cooking`, `residential-electrical`, `demo`) to the LLM-OS syscall ABI. This proves the ABI is sane. Each cartridge becomes `/cart/<name>/` with manifest, validators, allowed-opcode list.

**Week 4 — closed-loop validation.** Implement `<|loop|>` / `<|break|>` at the grammar level. Write a test that puts the Pi 5 through a 10-minute navigate-and-report task against a fixture environment. Measure token count, time, success rate.

**Week 5 — skillos_robot integration.** `/dev/roclaw` cartridge wrapping the existing 6-byte UDP ISA. The LLM-OS emits `<|call|>roclaw.forward{"left":150,"right":150}<|/call|>`; the cartridge translates to `AA 01 96 96 01 FF` and sends over UDP. The Dual-Brain Pi deployment runs end-to-end.

**Week 6 — cloud fallback + compactor + release.** Implement `<|fault|>` → cloud API. Ship the compactor as the swap daemon. Tag v0.01. Write the README. Put it on HuggingFace for the kernel model, GitHub for everything else.

After that, Week 7+ is the fine-tuning loop to get v0.1 (ISA-native model) by month 2.

---

## 9. The one-paragraph version

Your three projects converge on a single architecture. SkillOS-mini has the OS internals (blackboard = IPC, cartridges = drivers, schema-validated retry = protected mode, compactor = swap). skillos_robot has the ISA-emission proof (13 opcodes, 6-byte bytecode, one `memcpy`). llama.cpp has the substrate (GBNF at token level, logit bias, JSON-schema-to-grammar, samplers pipeline). The assembly is: define a 13-opcode ISA as GBNF over real tokenizer tokens, boot llama.cpp with that grammar and a 2B-class model, implement a ~500-line I/O daemon that executes syscall tokens against cartridges, and let the model control its own execution loops via explicit `<|loop|>` / `<|break|>` opcodes. On a Pi 5 this runs at ~8 Hz today, which is enough for a cognitive home-assistant OS. A cloud deployment of the same ISA runs at 60–200 Hz and does interactive work. v0.01 is 6 weeks of build. A fine-tuned kernel model is 2 months. Multi-tasking and self-hosting is 6 months. Linux took 30 months from hobby to 1.0; so will this, and the compute trajectory means the system will be substantially more capable than Linux 1.0 was at the equivalent milestone.

---

*Sources consulted during drafting include: llama.cpp master branch docs (grammars/README.md, tools/server/README.md), Karpathy's Nov 2023 LLM-OS X post, Shivance's 2023 HuggingFace post on LLM-OS with RLSF, METR's time-horizon paper (Kwa et al. 2025) and TH1.1 update (Jan 2026), multiple 2026 benchmark surveys (BenchLM, iternal.ai, CodeSOTA, Incremys), Stratosphere Labs Pi 5 benchmarks, Datature's Gemma 4 deep-dive, and the 2026 agent-harness survey framing the harness-as-infrastructure thesis. All three of your repos were read directly.*
