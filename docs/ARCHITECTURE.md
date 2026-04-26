<p align="center">
  <img src="assets/banner-architecture.svg" alt="ARCHITECTURE" width="100%"/>
</p>

<p align="center">
  <strong>llm_os</strong> &nbsp;//&nbsp; kernel architecture &nbsp;//&nbsp; <code>v0.5-rc1</code>
</p>

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

> Companion to [`README.md`](../README.md) and [`grammar/isa-spec.md`](../grammar/isa-spec.md).
> The README is the *pitch*, the ISA spec is the *contract*, this doc is
> the *map* — every layer, every data path, every safety invariant.

---

## ▸ §1 the OS-as-LLM mapping

The thesis: `llm_os` is not "an agent that uses an LLM". It is an OS
where the LLM itself plays the role of the CPU. Every primitive of a
classical OS has a corresponding primitive in this stack:

```mermaid
%%{init: {'theme':'base', 'themeVariables': {'primaryColor':'#18181b','primaryTextColor':'#e4e4e7','primaryBorderColor':'#ffffff','lineColor':'#a1a1aa','secondaryColor':'#27272a','tertiaryColor':'#000','background':'#000','mainBkg':'#18181b','clusterBkg':'#000','clusterBorder':'#a1a1aa','edgeLabelBackground':'#000','fontFamily':'ui-monospace, monospace'}}}%%
flowchart LR
    subgraph POSIX["POSIX side"]
        CPU[CPU instruction set]
        RAM[RAM]
        MMU[MMU / type safety]
        Swap[Swap]
        Ring[Ring 0 / Ring 3]
        SC[Syscall interface]
        Proc[/proc · /sys]
        Mod[Kernel module]
    end
    subgraph LLM_OS["llm_os side"]
        ISA["13-opcode ISA<br/>(GBNF tokens)"]
        KV[KV cache]
        GBNF[GBNF grammar<br/>sampler-enforced]
        Compact[KV compaction<br/>ISA-aware]
        LB[Logit bias mask]
        CALL["&lt;|call|&gt; opcode<br/>+ schema"]
        Trace[/trace · /policy<br/>iod endpoints]
        Cart["Cartridge<br/>manifest + schemas + handler"]
    end
    CPU -.-> ISA
    RAM -.-> KV
    MMU -.-> GBNF
    Swap -.-> Compact
    Ring -.-> LB
    SC -.-> CALL
    Proc -.-> Trace
    Mod -.-> Cart
```

The strongest claim: GBNF is not "a constraint we apply afterward",
it's the **type system enforced by the sampler at decode time**. Wrong
sequences are physically impossible to emit. Prompt engineering moves
to the compiler level.

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

## ▸ §2 the ring model

```mermaid
%%{init: {'theme':'base', 'themeVariables': {'primaryColor':'#18181b','primaryTextColor':'#e4e4e7','primaryBorderColor':'#ffffff','lineColor':'#a1a1aa','secondaryColor':'#27272a','background':'#000','mainBkg':'#18181b','clusterBkg':'#000','clusterBorder':'#a1a1aa','edgeLabelBackground':'#000','fontFamily':'ui-monospace, monospace'}}}%%
flowchart TB
    subgraph R3["Ring 3 · userland"]
        Cart[cartridges<br/>cart/system · io · sim · domestic]
    end
    subgraph R2["Ring 2 · iod kernel"]
        Iod[iod daemon · /dispatch]
        Sched[scheduler · multitask]
    end
    subgraph R1["Ring 1 · ISA enforcement"]
        Gram[grammar/isa.gbnf]
        Cap[capability mask · logit bias]
        Dial[dialect · token compression]
    end
    subgraph R0["Ring 0 · KV"]
        KV[KV cache · 8192 ctx]
        Swap[swap · ISA-aware compaction]
    end
    subgraph HW["HW · CPU"]
        Llama[llama.cpp sampler loop]
        Boot[bootloader.c]
    end

    Cart --> Iod
    Iod --> Sched
    Sched --> Gram
    Gram --> Cap
    Cap --> Dial
    Dial --> KV
    KV --> Swap
    Swap --> Llama
    Boot --> Llama
```

| Ring | Latency budget | Determinism | Purpose |
|---|---|---|---|
| **R3** · userland | ms (cartridge-side) | mixed (handler-defined) | syscall implementations |
| **R2** · iod kernel | sub-ms | hard | dispatch · schedule |
| **R1** · ISA | sampler step | **hard** (grammar) | every emitted token validated |
| **R0** · KV | constant per token | hard | working memory |
| **HW** · CPU | weights @ ~120 ms/token on Pi 5 | n/a | the LLM itself |

The deeper the ring, the harder the determinism. R1 is the sampler-
level guarantee that the LLM cannot emit a malformed instruction
sequence.

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

## ▸ §3 the dispatch loop

```mermaid
%%{init: {'theme':'base', 'themeVariables': {'primaryColor':'#18181b','primaryTextColor':'#e4e4e7','primaryBorderColor':'#ffffff','lineColor':'#a1a1aa','secondaryColor':'#27272a','background':'#000','mainBkg':'#18181b','actorBkg':'#18181b','actorBorder':'#ffffff','actorTextColor':'#e4e4e7','signalColor':'#a1a1aa','signalTextColor':'#e4e4e7','noteBkgColor':'#27272a','noteTextColor':'#bff7ff','noteBorderColor':'#00d4ff','activationBkgColor':'#ffffff','sequenceNumberColor':'#000','fontFamily':'ui-monospace, monospace'}}}%%
sequenceDiagram
    participant User
    participant Iod as iod (/dispatch)
    participant Llama as llama.cpp
    participant Parser as parser.rs
    participant Cap as capability.rs
    participant Cart as cartridge handler
    participant KV as KV cache

    User->>Iod: POST /dispatch {goal}
    Iod->>Cap: load policy mask
    Iod->>Llama: prompt + grammar=isa.gbnf
    loop sampler step (8 Hz on Pi 5)
        Llama->>Llama: sample next token (GBNF + bias)
        Llama-->>Parser: token
        Parser->>Parser: assemble Op event
        alt Op == &lt;|call|&gt;
            Parser->>Iod: call(cart.method, args)
            Iod->>Iod: schema validate
            Iod->>Cart: invoke handler
            Cart-->>Iod: result json
            Iod->>Llama: inject &lt;|result|&gt;…&lt;|/result|&gt;
        else Op == &lt;|halt|&gt;
            Parser->>Iod: program end
        else Op == &lt;|yield|&gt;
            Parser->>Iod: scheduler swap point
        end
        Llama->>KV: append tokens
        Note over KV: util ≥ 70% ⇒ compact
    end
    Iod-->>User: trace + final state
```

Three things to notice:
- **The grammar is the contract.** Every token Llama can emit is
  syntactically legal — invalid ISA sequences are decode-rejected.
- **The result block is predicted by the grammar.** The sampler knows
  shape `<|result|>…<|/result|>` will appear next; the daemon
  injects, the sampler resumes. Neither side is trusted alone.
- **Compaction may run mid-dispatch.** This is where state corruption
  lurks if not handled carefully — see §5 and
  [`NEXT_STEPS.md §2`](NEXT_STEPS.md).

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

## ▸ §4 cartridge anatomy

```mermaid
%%{init: {'theme':'base', 'themeVariables': {'primaryColor':'#18181b','primaryTextColor':'#e4e4e7','primaryBorderColor':'#ffffff','lineColor':'#a1a1aa','secondaryColor':'#27272a','background':'#000','mainBkg':'#18181b','clusterBkg':'#000','clusterBorder':'#a1a1aa','edgeLabelBackground':'#000','fontFamily':'ui-monospace, monospace'}}}%%
flowchart TB
    Manifest["manifest.json<br/>name · methods · preferred_tier · capabilities"]
    Schemas["schemas/*.schema.json<br/>JSON Schema draft-07"]
    Dialect["dialect.gbnf<br/>token-compressed call syntax"]
    Handler["handler.rs (v0.5)<br/>OR handler.wasm (v1.0)"]

    Manifest --> Schemas
    Manifest --> Dialect
    Manifest --> Handler
    Schemas -. validate .-> Handler
    Dialect -. compresses .-> Manifest
```

A concrete example — the `system.summarize` cartridge:

```
cart/system/summarize/
├── manifest.json                # name="system.summarize", methods=[run], preferred_tier="local"
├── schemas/
│   └── run.args.schema.json     # { path: string, max_chars?: int }
├── dialect.gbnf                 # optional · adds "S <path>" shorthand
└── (handler is in runtime/handlers.rs for v0.5; per-cart .wasm in v1.0)
```

When the LLM emits `<|call|>system.summarize {"path":"./README.md"} <|/call|>`:

1. **Parser** assembles the `Op::Call` event.
2. **Capability** check: is `system.summarize` in the current policy
   mask? (yes by default).
3. **Schema validation**: JSON args validated against
   `schemas/run.args.schema.json`. Mismatch → `<|fault|>` injected.
4. **Handler** runs (Rust function in v0.5).
5. **Result** wrapped in `<|result|>...<|/result|>` and re-injected.
6. **Sampler** resumes. Grammar predicted this exact shape so no
   re-priming is needed.

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

## ▸ §5 KV cache · the RAM model

```mermaid
%%{init: {'theme':'base', 'themeVariables': {'primaryColor':'#18181b','primaryTextColor':'#e4e4e7','primaryBorderColor':'#ffffff','lineColor':'#a1a1aa','secondaryColor':'#27272a','background':'#000','mainBkg':'#18181b','clusterBkg':'#000','clusterBorder':'#a1a1aa','edgeLabelBackground':'#000','fontFamily':'ui-monospace, monospace'}}}%%
flowchart LR
    subgraph KV["KV cache · 8192 ctx"]
        Sys["system prompt<br/>· pinned ·"]
        Isa["ISA opcodes<br/>· grammar context ·"]
        Work["working set<br/>tokens since last compact"]
        Sum["compacted summary<br/>(after compact)"]
        Free["free"]
    end

    Token[new token] --> Work
    Work -. ≥ 70% util .-> Compactor[swap.rs<br/>ISA-aware]
    Compactor --> Sum
    Sum --> Work

    style Sys fill:#ffffff,color:#000
    style Isa fill:#e4e4e7,color:#000
    style Work fill:#a1a1aa,color:#000
    style Sum fill:#52525b,color:#e4e4e7
    style Free fill:#000,color:#a1a1aa,stroke:#52525b,stroke-dasharray:4 2
```

### compaction (= swap)

When KV utilization crosses 70%, [`runtime/swap.rs`](../runtime/swap.rs)
asks the model to summarize the older portion of the working set,
replaces those tokens with the summary, and reinjects.

**The danger:** if the compactor drops a token sequence that includes
an unclosed `<|loop|>` or a pending `<|result|>` expectation, the
GBNF grammar will reject the next emission as malformed. The current
v0.5 implementation does not yet track this state — it's the
[`NEXT_STEPS.md §2`](NEXT_STEPS.md) work.

### v1.0 ISA-aware compactor (planned)

The compactor will preserve:
- exact current loop depth (the grammar's stack pointer)
- pending `<|result|>` / `<|ack|>` expectations
- in-flight subagent fork ids

before dropping any tokens. The summary that replaces older tokens is
prepended with explicit state markers so the sampler resumes coherently.

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

## ▸ §6 capability · the ring model in practice

```mermaid
%%{init: {'theme':'base', 'themeVariables': {'primaryColor':'#18181b','primaryTextColor':'#e4e4e7','primaryBorderColor':'#ffffff','lineColor':'#a1a1aa','background':'#000','mainBkg':'#18181b','clusterBkg':'#000','clusterBorder':'#a1a1aa','edgeLabelBackground':'#000','fontFamily':'ui-monospace, monospace'}}}%%
flowchart TB
    Policy["policy mask<br/>{ allow: [system.*, sim.*],<br/>  deny: [io.roclaw.shutdown] }"]
    Mask[capability.rs<br/>build logit bias]
    Sampler[llama.cpp sampler]
    Daemon[iod dispatch · daemon-side reject]

    Policy --> Mask
    Mask --> Sampler
    Sampler --> TokenStream[token stream]
    TokenStream --> Daemon
    Daemon -- "ok" --> Result[inject result]
    Daemon -- "denied" --> Reject["inject<br/>&lt;|result|&gt;{error:'access_denied'}&lt;|/result|&gt;"]
```

Two layers of enforcement, intentionally redundant:

1. **Logit bias** at the sampler — sets banned-opcode logits to −∞ so
   the model never tokenizes them.
2. **Daemon-side reject** — even if the bias is bypassed (multi-token
   merging, bootstrap models with weak token boundaries), the `iod`
   parser catches forbidden ops and synthesizes an `access_denied`
   result.

The redundancy matters because GBNF / logit bias is brittle on
bootstrap models with imperfect tokenizers. We trust the daemon-side
reject as the hard fence; the bias is a soft optimization. See
[`NEXT_STEPS.md §5`](NEXT_STEPS.md).

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

## ▸ §7 dialect · token economy as cycle optimization

At 8 Hz on a Pi 5, every token saved is a cycle saved. The dialect
layer compresses the wire format of high-frequency calls without
losing semantics:

```
verbose                     dialect              tokens saved
─────────                   ─────────            ──────────
{"left":150,"right":150}    F 150 150            7×
{"angle":45,"speed":80}     R 45 80              6×
{"action":"stop"}           S                    9×
```

Each cartridge can declare a `dialect.gbnf` adding shorthand syntax
that maps to its full JSON schema. The grammar accepts both forms;
the parser canonicalizes before schema validation.

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

## ▸ §8 dual-brain routing

```mermaid
%%{init: {'theme':'base', 'themeVariables': {'primaryColor':'#18181b','primaryTextColor':'#e4e4e7','primaryBorderColor':'#ffffff','lineColor':'#a1a1aa','secondaryColor':'#27272a','background':'#000','mainBkg':'#18181b','clusterBkg':'#000','clusterBorder':'#a1a1aa','edgeLabelBackground':'#000','fontFamily':'ui-monospace, monospace'}}}%%
flowchart LR
    Goal[/dispatch] --> Iod[iod]
    Iod --> Manifest{cart.preferred_tier?}
    Manifest -- "local" --> Local[llama.cpp · Pi 5]
    Manifest -- "cloud" --> Sub[subagent.rs]
    Sub --> Cloud[Gemini 2.5 Flash · cloud]
    Local --> Result1[result]
    Cloud --> Result2[result]
    Result1 --> Iod
    Result2 --> Iod
```

Each cartridge declares `preferred_tier: "local" | "cloud"` in its
manifest. The subagent path uses the same ISA — the cloud worker
emits identical opcodes; only the sampler is remote. This means a
goal that needs heavier planning (e.g. multi-step reasoning over a
large knowledge base) escalates without a separate code path. See
[`runtime/cloud.rs`](../runtime/cloud.rs) and
[`runtime/subagent.rs`](../runtime/subagent.rs).

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

## ▸ §9 cross-cutting invariants

The five non-negotiables, encoded in tests:

1. **Grammar invariants** (sampler-enforced). `<|halt|>` only at depth 0,
   every `<|loop|>` matched, every `<|read|>`/`<|call|>`/`<|wait|>`/
   `<|fault|>`/`<|policy|>` followed by a `<|result|>` block. See
   [`grammar/tests/`](../grammar/tests/).
2. **Schema validation before handler.** No cartridge handler runs on
   unvalidated args. Period.
3. **Result injection is grammar-predicted.** The sampler doesn't
   re-prime; it expects the result shape and resumes mid-stream.
4. **Capability is checked twice** (logit bias + daemon-side reject).
5. **Cartridge handlers are pure** with respect to KV state. Side
   effects go through declared schemas; no out-of-band mutation.

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

## ▸ §10 file map

```
runtime/
├── bootloader.c            ← C bootstrap; mmaps GGUF
├── lib.rs                  ← library root
├── iod_main.rs             ← daemon entry point
├── iod.rs                  ← /dispatch /trace /policy endpoints
├── parser.rs               ← token stream → Op events
├── dispatch.rs             ← Op::Call → handler
├── handlers.rs             ← bootstrap in-process handlers (→ WASM in v1.0)
├── cartridge.rs            ← manifest + schema loading
├── capability.rs           ← logit bias + policy mask
├── dialect.rs              ← per-cart dialect compression
├── scheduler.rs            ← yield / multitask context-switch
├── multitask.rs            ← fork / wait / branched KV
├── swap.rs                 ← KV compaction (will be ISA-aware)
├── subagent.rs             ← cloud delegation
├── cloud.rs                ← cloud HTTP client
├── roclaw.rs               ← roclaw cartridge bridge
├── tool_parser.rs          ← legacy tool-call parser (compat)
├── schema_to_gbnf.rs       ← compile JSON Schema → per-call GBNF
├── compile_schemas_main.rs ← build-time schema compilation entry
├── mock_server.rs          ← test-only mock llama-server
└── tests/                  ← rust integration tests

grammar/
├── isa.gbnf                ← the 13-opcode ISA
├── isa-spec.md             ← semantics + invariants
└── tests/
    ├── legal/              ← must parse
    └── illegal/            ← must NOT parse

cart/
├── system/{summarize,demo}/
├── io/roclaw/
├── sim/sim_world/
└── domestic/{cooking,residential-electrical}/
```

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

## ▸ §11 what's not here yet

- **In-llama.cpp grammar swap** — currently 3 HTTP requests per
  syscall when per-method GBNF activates. v1.0 fix in
  [`NEXT_STEPS.md §1`](NEXT_STEPS.md).
- **ISA-aware compactor** — see §5 above.
- **WASM cartridge sandbox** — handlers run in-process; v1.0 moves to
  `wasmtime`. See [`NEXT_STEPS.md §4`](NEXT_STEPS.md).
- **Single-token tokenizer extension** — current bootstrap uses
  multi-token string patterns. The v1.0 fine-tune will register all
  18 ISA tokens as strict single tokens. See
  [`NEXT_STEPS.md §6`](NEXT_STEPS.md).
- **Three-strikes escalation** — schema-violation loops can burn
  context. The fix is in [`NEXT_STEPS.md §3`](NEXT_STEPS.md).

These are *intentionally* incomplete. The roadmap is in
[`NEXT_STEPS.md`](NEXT_STEPS.md).

<p align="center">
  <img src="assets/divider.svg" alt="" width="100%"/>
</p>

<p align="center">
  <img src="assets/mark.svg" alt="" width="48"/>
</p>

<p align="center">
  <sub><code>// ARCH.MAP // 13 OPCODES · 5 INVARIANTS · 4 RINGS</code></sub>
</p>
