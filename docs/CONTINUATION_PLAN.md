# LLM-OS — Continuation Plan (v0.01 → v0.1)

*Author: project bootstrap, 2026-04-24. Companion to [`design/llm-os-design.md`](../design/llm-os-design.md) and the high-level [`plan.md`](plan.md). This document is the operational playbook: what to do next, in what order, with what acceptance criteria.*

---

## 0. Current state — what is on disk vs. what design §8 calls for

The repository is scaffolded but unpowered. The grammar exists; the substrate that consumes it does not.

| Area | Design §8 calls for | What exists today | Gap |
|---|---|---|---|
| ISA grammar | `isa.gbnf` over Qwen 3 2B tokenizer, multi-token opcodes | [`grammar/isa.gbnf`](../grammar/isa.gbnf) drafted with all 13 opcodes + 5 invariants | **Not validated.** No `llama-gbnf-validator` run yet; tokenization parity unchecked. |
| ISA spec | Opcode table + invariants + legal/illegal examples | [`grammar/isa-spec.md`](../grammar/isa-spec.md), [`grammar/tests/`](../grammar/tests/) (6 legal + 6 illegal fixtures) | Fixtures exist on disk but have never been mechanically checked. |
| Bootloader | `bootloader.c` (~200 lines) — fork llama-server, FIFOs, boot prompt | `runtime/` is **empty** | Entire component missing. |
| I/O daemon | `iod.rs` (~500 lines) — opcode dispatch, KV reinjection, scheduler hook | `runtime/` is **empty** | Entire component missing. |
| Cartridges | `cooking`, `residential-electrical`, `demo` ported from `skillos_mini` | None | Untouched. |
| RoClaw bridge | `/dev/roclaw` cartridge → 6-byte UDP frame | None | Untouched; depends on `RoClaw/sim/` being reachable. |
| Cloud fallback | `<\|fault\|>` → cloud API dispatch | None | Untouched. |
| Compactor | Promote `skillos_mini` compactor to swap daemon | None | Untouched. |
| Scripts | `scripts/validate_grammar.sh` (referenced from README) | `scripts/` is **empty** | Referenced but missing — README is broken. |

**Net assessment:** The thinking is done (design doc + ISA spec are thorough). The lowest-risk, highest-leverage next move is to make the grammar mechanically verifiable on this host so every subsequent component has a contract it can be tested against. Without grammar validation, every other piece is built blind.

---

## 1. The critical path

The pieces have a strict dependency order. Skipping ahead means redoing work.

```
   ┌──────────────────────┐
   │ G. Grammar validated │   ← unblocks everything; do this first
   └─────────┬────────────┘
             │
   ┌─────────▼────────────┐
   │ T. Tokenizer parity  │   ← cheap check; flushes out opcode-string bugs
   └─────────┬────────────┘
             │
   ┌─────────▼────────────┐         ┌─────────────────────────┐
   │ B. Bootloader        │────────▶│ D. I/O daemon (skeleton)│
   └──────────────────────┘         └────────────┬────────────┘
                                                 │
                              ┌──────────────────┼─────────────────────┐
                              │                  │                     │
                  ┌───────────▼─────┐   ┌────────▼────────┐   ┌────────▼─────────┐
                  │ C. Cartridge    │   │ L. Closed-loop  │   │ R. RoClaw bridge │
                  │   adapter       │   │   fixture       │   │ /dev/roclaw      │
                  └─────────────────┘   └─────────────────┘   └──────────────────┘
                              │                  │                     │
                              └──────────────┬───┴─────────────────────┘
                                             │
                              ┌──────────────▼──────────────┐
                              │ F. Cloud fallback + S. swap │
                              └──────────────┬──────────────┘
                                             │
                              ┌──────────────▼──────────────┐
                              │ V. v0.01 tag                │
                              └─────────────────────────────┘
```

**Critical path length: G → T → B → D → C → L → R → F/S → V.** Anything not on this path is parallelizable.

---

## 2. Work breakdown — week-by-week with acceptance criteria

Each item below has: **deliverable** (concrete file or behavior), **acceptance** (how to know it's done), **risk** (what could go wrong), and **dep** (what blocks it).

### Week 1 (now → +7 days) — Mechanically verify the grammar

#### G1. `scripts/validate_grammar.sh`
- **dep:** none.
- **deliverable:** shell script that runs `llama-gbnf-validator` on every fixture in `grammar/tests/legal/` (must pass) and `grammar/tests/illegal/` (must fail), exits non-zero on any mismatch, prints a one-line summary `legal: 6/6 pass, illegal: 6/6 fail`.
- **acceptance:** `./scripts/validate_grammar.sh` exits 0 on a host with `llama-gbnf-validator` on `$PATH`; exits 2 with a clear "validator not found, see README §Quickstart" message otherwise.
- **risk:** `llama-gbnf-validator` exit codes vary across llama.cpp versions. Pin to current `master` or whichever commit ships single-token grammar references.

#### G2. Run the validator and fix what fails
- **dep:** G1.
- **deliverable:** validator passes on all 12 fixtures.
- **acceptance:** `./scripts/validate_grammar.sh` returns 0 on a Pi 5 dev box or laptop with llama.cpp built. Any fixture that doesn't behave as classified gets either (a) the grammar fixed to match the design intent, or (b) the fixture re-classified with a written justification appended to `isa-spec.md §3/§4`.
- **risk:** The grammar's `result-block` rule may be too permissive (opaque text content); illegal fixture #4 ("call without result-block") may parse anyway because the next line's text is consumed as result content. Inspect carefully — this is the kind of bug that defeats the whole "grammar is a contract" claim of `isa-spec.md §2`.

#### T1. Tokenizer parity sanity check
- **dep:** none (parallel with G).
- **deliverable:** `scripts/check_tokenizer.sh` — pipes each opcode string (`<|read|>`, `<|write|>`, …, `<|/result|>`) through `llama-tokenize` against the chosen base model (Qwen 3 2B and Gemma 4 E2B) and writes `docs/tokenization-report.md` with token counts per opcode.
- **acceptance:** report shows opcode token counts for both models. If any opcode tokenizes inconsistently across runs (e.g., leading-space sensitivity), flag in the report.
- **risk:** Qwen 3 2B base vs Instruct may differ on special-character handling; this is `plan.md` open question #1.
- **why this matters:** the bootstrap claim is "multi-token opcodes are fine because we'll fine-tune later" — but if an opcode tokenizes to 12 tokens instead of 5, the per-syscall cost on Pi 5 doubles and the throughput math in design §6.2 changes.

### Week 2 (+8 → +14 days) — Bootloader + I/O daemon skeleton

#### B1. `runtime/bootloader.c`
- **dep:** G2.
- **deliverable:** ~200-line C program that:
  1. Parses `--model`, `--grammar`, `--ctx-size`, `--port` flags.
  2. `fork()`s `llama-server` with `--grammar grammar/isa.gbnf --reasoning-format none --cache-prompt true` and the parsed flags.
  3. Opens FIFOs `/tmp/llmos.in` and `/tmp/llmos.out`.
  4. Streams the boot prompt (a system-prompt fragment that loads ISA awareness + initial cartridge manifests) to the server's `/v1/completions` endpoint, with `stream: true`.
  5. Waits for the model to emit `<|ready|>` (a sentinel grammar adds for v0.01) and signals success on stdout.
  6. Forwards subsequent FIFO traffic to/from the server.
- **acceptance:** running `./runtime/bootloader --model /path/to/qwen3-2b-q4.gguf` results in `<|ready|>` being printed within 30 s on a Pi 5.
- **risk:** `cache_prompt: true` semantics in current llama.cpp differ from the docs in subtle ways (cache-key derivation by full prefix, not just system prompt). If the boot prompt is regenerated on every connection, the "L1 cache" claim of design §4.4 breaks.
- **decision needed:** is the bootloader a one-shot (exits after `<|ready|>`, leaves llama-server running) or a long-running parent (forwards FIFO traffic forever)? Recommend **one-shot** for v0.01 — keeps it ≤200 lines. The I/O daemon owns the long-running connection.

#### D1. `runtime/iod.rs`
- **dep:** B1.
- **deliverable:** ~500-line Rust program that:
  1. Connects to the running llama-server's streaming endpoint.
  2. Parses the SSE stream incrementally; runs a small state machine that tracks loop depth and pending result-blocks.
  3. On each opcode (one of the 13 + `<|ready|>`):
     - **`<|read|>`/`<|write|>`/`<|wait|>`** — fd table dispatch (initial table: 0=stdin, 1=stdout, 2=stderr, 3+=cartridge fd's).
     - **`<|call|>`** — cartridge dispatch (Week 3); for v0.01 stub returns `<|result|>{"stub":true}<|/result|>`.
     - **`<|loop|>`/`<|break|>`** — increment/decrement depth counter; assert grammar invariants I1–I3 hold (defense in depth).
     - **`<|halt|>`** — terminate task, write final status to fd 1.
     - **`<|fault|>`** — Week 6 (cloud fallback); for v0.01 print and exit non-zero.
     - **`<|policy|>`** — return current allowed-opcode set; daemon does NOT yet update logit bias mid-stream (Month 2+ feature).
     - **`<|think|>`/`<|commit|>`/`<|fork|>`/`<|yield|>`** — log only for v0.01; no behavior.
  4. Result-block reinjection: after dispatching an opcode that requires a result, daemon writes `<|result|>…<|/result|>` back to the model via the prompt-continuation API (llama-server `/v1/completions` with `prompt: <continuation>`).
- **acceptance:** an end-to-end fixture run (see L1 below) where the model emits a 5-step program and the daemon dispatches all opcodes with no schema or grammar violations.
- **risk:** **result-block reinjection is the riskiest piece in the whole sprint.** llama-server's KV reuse only works if the prompt continuation is appended exactly as it would have been during continuous decode. Any tokenization difference breaks cache reuse and we silently fall back to a full reprocess — turning 8 Hz into 1 Hz. Test by comparing token counts in the server's `cache_n` field across daemon-injected vs. natively-decoded result blocks.
- **decision needed:** Rust vs Go for the daemon. `plan.md` says Rust. Rust gives smaller binaries and tighter FFI for future SIMD opcode parsers; Go gives faster prototyping. Recommend **Rust** for the long-term reasons but acknowledge a 1-week velocity tax in Week 2.

### Week 3 (+15 → +21 days) — Cartridge adapter

#### C1. `/cart/<name>/manifest.json` schema
- **dep:** D1.
- **deliverable:** `docs/cartridge-manifest.md` — the v0.01 manifest schema. Required fields: `name`, `version`, `allowed_opcodes` (subset of the 13), `methods` (each with JSON Schema for args + result), `binary_path` (optional Python helper).
- **acceptance:** schema compiles to valid JSON Schema and is validatable against itself.

#### C2. Port `cooking`, `residential-electrical`, `demo` from `skillos_mini`
- **dep:** C1, access to `skillos_mini` (no local clone — clone fresh per memory note).
- **deliverable:** `cartridges/cooking/`, `cartridges/residential-electrical/`, `cartridges/demo/` with `manifest.json` + Python validators copied verbatim. Method bodies stubbed to return `{"ok":true,"stub":"v0.01"}`; full method implementations land in v0.1.
- **acceptance:** each cartridge loads via the I/O daemon's `<|call|>` dispatch and returns the stubbed result without grammar errors.
- **risk:** `skillos_mini` cartridges depend on the blackboard for cross-call state; `<|call|>` is stateless per-call. The blackboard becomes a v0.1 cartridge (`/cart/blackboard/`) — for v0.01 cartridges that need state use `<|commit|>` with explicit JSON.

#### C3. Per-cartridge JSON Schema validation in `iod.rs`
- **dep:** C1.
- **deliverable:** `iod` validates `<|call|>` args against the cartridge's `methods.<method>.args_schema` before dispatch; on failure, emits `<|result|>{"error":"schema_violation","details":[…]}<|/result|>` and lets the model retry.
- **acceptance:** sending malformed args produces a recoverable error the model sees in the next decode step.
- **deferred to v0.1:** compile per-cartridge schemas to GBNF and load them as call-arg sub-grammars — eliminates the retry path entirely (design §3.2). For v0.01, runtime validation is sufficient.

### Week 4 (+22 → +28 days) — Closed-loop validation

#### L1. End-to-end navigate-and-report fixture
- **dep:** C2.
- **deliverable:** `tests/e2e/navigate_and_report.sh` — invokes the bootloader + I/O daemon against a fixture cartridge `/cart/sim_world/` (deterministic Markov-chain world). Goal: model emits a `<|loop|>` that calls `sim_world.step` 10× and `<|break|>`s when the goal predicate is met.
- **acceptance:**
  - 10 consecutive runs all `<|halt|>status=success`.
  - Average wall time ≤ 10 minutes on Pi 5 8GB with Qwen 3 2B Q4.
  - Token count per run logged; should be < context size.
  - Zero grammar violations (validated by tee'ing the model output through `llama-gbnf-validator` post hoc).
- **risk:** model fails to terminate the loop — emits `<|break|>` only when manually nudged. This would show the fundamental "model-controlled closed loop" claim of design §4.8 needs explicit prompt scaffolding for bootstrap models. Mitigation: add a system-prompt clause `"Emit <|break|> the moment your stop condition is met. Do not deliberate."` Track whether the fine-tuned kernel model (Month 2) needs the same scaffolding.

### Week 5 (+29 → +35 days) — RoClaw integration

#### R1. `/cart/roclaw/` bridge cartridge
- **dep:** D1, RoClaw repo's `sim/` reachable.
- **deliverable:** cartridge whose methods (`forward`, `turn_left`, `turn_right`, `stop`, `get_camera`, …) map directly to RoClaw 6-byte UDP frames per `RoClaw/src/llmunix-core/` ISA. Runs `roclaw_bridge.py` (or a Rust port) as a subprocess listening on the same UDP port the RoClaw sim uses.
- **acceptance:**
  - Manual: `<|call|>roclaw.forward {"left":150,"right":150}<|/call|>` results in the RoClaw sim moving forward 150cm.
  - Automated: an end-to-end test from the LLM-OS issuing 10 navigation calls produces 10 valid 6-byte UDP frames at the bridge layer.
- **risk:** `roclaw_bridge.py` lives in `skillos/`, not `RoClaw/`. The bridge's UDP protocol may have drifted from RoClaw's. Verify by running `RoClaw/sim/build_scene.py` first, then the bridge, and capturing one frame with `tcpdump` to confirm the byte layout matches `AA 01 …` per design §1.3.

#### R2. Dual-brain Pi deployment validation
- **dep:** R1.
- **deliverable:** `docs/dual-brain-deployment.md` — runbook for: Pi 5 cerebellum (Qwen 3 2B local, ~8 Hz) + cloud cortex (Claude Opus 4.6 or Kimi K2.5, every 10–30s). The cloud cortex emits `<|fork|>goal=…` calls that the cerebellum executes.
- **acceptance:** runs end-to-end on a Pi 5 with a 30-minute navigation-and-report task. Latency budget: cerebellum ≤ 6s/syscall, cortex ≤ 15s/strategic-decision.
- **deferred:** preemptive scheduler that interrupts the cerebellum when the cortex emits a higher-priority goal — that's design §4.6 / Month 6.

### Week 6 (+36 → +42 days) — Cloud fallback + compactor + tag

#### F1. `<|fault|>` → cloud API dispatch
- **dep:** D1, `<|fault|>` opcode wired in iod.
- **deliverable:** when the model emits `<|fault|>{"needs_cloud":true,…}<|/fault|>`, daemon (a) compacts current KV state via the compactor (S1), (b) sends compacted state + fault payload to a cloud model (provider-agnostic — use the OpenAI-compatible endpoint shape), (c) injects the response as `<|result|>…<|/result|>`.
- **acceptance:** a fixture program that intentionally hits an unrecoverable schema-violation triggers cloud fallback and the model receives a useful continuation token within ≤ 5s additional latency.
- **risk:** cloud provider API surface drift. Decision: target the OpenAI-compatible endpoint shape so the same code works against `claude.ai`, `kimi`, and self-hosted vLLM behind a thin adapter.

#### S1. Compactor as swap daemon
- **dep:** access to `skillos_mini/compactor.py` (currently in skillos_mini repo per 2026-04-23 split).
- **deliverable:** port `compactor.py` into `runtime/swap.py` (or keep as Python subprocess called from `iod`). Trigger: KV-cache utilization ≥ 75%. Output: a compacted prompt prefix that gets re-pinned as the L1 cache.
- **acceptance:** running L1's 10-minute fixture with `--ctx-size 8192` (deliberately undersized) triggers ≥ 1 compaction without losing task progress.
- **risk:** compaction is itself an LLM call — on Pi 5 it could take 30+ seconds. Acceptable for v0.01; for v0.1 consider running the compactor on a separate small model in parallel (this is the "compaction daemon in the background" pattern noted in design §5).

#### V1. Tag v0.01
- **dep:** all of the above.
- **deliverable:**
  - `git tag v0.01`.
  - `RELEASE_NOTES.md` summarizing what works, what's stubbed, what's deferred to v0.1.
  - HuggingFace upload of the chosen base model + grammar (no fine-tune yet — that's v0.1).
  - GitHub release with built binaries for Pi 5 (aarch64) and macOS (arm64).
- **acceptance:** a stranger can `git clone && ./scripts/quickstart.sh` and have the LLM-OS run a "hello, list this directory" task in under 10 minutes from a cold checkout.

---

## 3. Decision log — resolutions to `plan.md` open questions

| `plan.md` question | Proposed resolution | Justification |
|---|---|---|
| Qwen 3 2B Instruct vs base | **Instruct** for v0.01 | Bootstrap models need prompt-following; base models will need extensive few-shot to emit ISA tokens. Switch to base+fine-tune for v0.1. |
| Single long `<|loop|>` session vs many halting programs | **Many halting programs** | Each user task is one program; inter-task state lives in cartridges (`<|commit|>`). Avoids context-window growth; matches Unix `exec` model rather than CICS-style long sessions. |
| JSON-Schema→GBNF per cartridge: v0.1 or v0.5? | **v0.1, alongside the kernel fine-tune** | Fine-tune adds the special tokens; same release adds compiled per-cartridge sub-grammars. Both are tokenizer-rebuild-class changes; ship together. |

---

## 4. Risk register

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | Result-block reinjection breaks KV cache reuse, dropping throughput from 8 Hz to ~1 Hz on Pi 5 | Medium | High | Week 2: instrument `cache_n` from llama-server; assert ≥ 95% cache reuse before declaring D1 done. |
| R2 | Bootstrap Qwen 3 2B Instruct can't reliably terminate `<|loop|>` blocks without extensive prompting | Medium | Medium | Week 4: explicit "stop the moment your goal is met" scaffolding; if still flaky, accept it and ship v0.01 with caveat — the fix is the v0.1 fine-tune. |
| R3 | `llama-gbnf-validator` exit-code semantics drift between llama.cpp versions | Low | Low | Pin to a specific commit in `scripts/validate_grammar.sh`; document in README. |
| R4 | RoClaw `roclaw_bridge.py` lives in `skillos/` (per memory) but its UDP protocol drifted from current RoClaw firmware | Medium | Medium | Week 5 day 1: capture one frame with tcpdump, compare byte-for-byte to RoClaw spec; if drift exists, port from RoClaw repo's bridge instead. |
| R5 | Cloud-API drift breaks `<|fault|>` fallback | Low | Low | Use OpenAI-compatible endpoint shape; isolate provider quirks in a thin adapter file. |
| R6 | Compactor latency on Pi 5 (30s+) makes user-perceived behavior poor | High | Low (v0.01) | Acceptable for v0.01 — document. v0.1 runs compactor on a separate small model in parallel. |
| R7 | METR/capability extrapolation is wrong; small-on-device 2B doesn't track frontier as design §6 predicts | Low (12mo horizon) | Medium | Doesn't matter for v0.01–v0.5; matters for v1.0 sales pitch. Re-baseline projections at v0.5. |
| R8 | Hidden grammar bug — illegal fixtures pass because `result-block` is too permissive | Medium | High | G2's job to catch this; if found, tighten grammar before any other work proceeds. |

---

## 5. Out of scope for this sprint (v0.01)

These are explicitly deferred. Calling them out so they don't sneak in:

- **Tokenizer rebuild + kernel fine-tune** — Month 2, paired with v0.1.
- **Preemptive scheduler with mid-stream `<|yield|>` injection** — Month 6, v0.5.
- **KV-cache swap with multi-task isolation** — Month 6, v0.5.
- **Self-hosting trace pipeline** (Linux-0.11 moment) — Month 6, v0.5.
- **Per-cartridge GBNF compilation** — v0.1.
- **Capability enforcement via dynamic logit bias** — v0.1 (currently the daemon only logs `<|policy|>`; doesn't yet re-mask).
- **Fork semantics** — v0.5. v0.01 logs `<|fork|>` and treats it as a structured note.
- **`<|commit|>` to persistent memory** — v0.01 commits to a per-session JSON file; the cartridge-memory abstraction lands in v0.1.
- **Provisional patent on closed-loop primitive** — separate track from engineering; coordinate with EvolvingAgentsLabs IP strategy noted in design §7.

---

## 6. What "done" looks like for v0.01

A user with a Raspberry Pi 5, a Qwen 3 2B Q4 GGUF, and this repo can:

1. Clone the repo.
2. Run `./scripts/quickstart.sh`.
3. Within 10 minutes (cold), see the LLM-OS boot, accept a goal in natural language, decompose it into ISA opcodes, dispatch them to cartridges, and `<|halt|>status=success`.

That is the v0.01 acceptance test. Linux 0.01 booted to a shell. We boot to a working ISA dispatcher with three cartridges and a robot bridge.

---

## 7. After v0.01 — fast-forward to v0.1 (target: +60 days from today)

The fine-tune. Specifically:

- Collect ≥ 5,000 valid execution traces from v0.01 deployments (synthetic + real if any users exist).
- Add 18 special tokens to Qwen 3 2B (or Gemma 4 E2B) tokenizer.
- Post-train via DPO/GRPO with reward = `successful_syscall + task_completion + grammar_validity`.
- Re-run all v0.01 acceptance tests; expect 5–10× throughput improvement for syscall-dense workloads (design §5 and §6.2).
- Ship `kernel-gemma4-e2b-isa-v1.gguf` to HuggingFace under Apache 2.0.
- Tag `v0.1`.

The v0.1 release is the moment the LLM-OS becomes interesting to people outside this lab — it's when the tokens-per-syscall economics flip from "tolerable" to "competitive with anything else on a Pi."
