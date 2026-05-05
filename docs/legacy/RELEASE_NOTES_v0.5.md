# LLM-OS v0.5-rc1 — Release Notes

*Tag: `v0.5-rc1`. Released 2026-04-25 (same day as v0.1-rc1; v0.5 ships the OS-internals work that doesn't require the kernel fine-tune to be done first). Compared to the original Month-6 design timeline, this is roughly 5 months early — but the multi-task and self-hosting pieces all need real-hardware validation before the v0.5-final tag.*

---

## What's new vs. v0.1-rc1

| Area | v0.1-rc1 | v0.5-rc1 |
|---|---|---|
| Per-method GBNF artifacts | Compiled and materialized; daemon doesn't use them mid-stream | Compiled grammars **surfaced inside schema-violation errors** so the model sees the exact expected shape on retry. True decode-time mid-stream swap is specced in `docs/grammar-swap-design.md` for v1.0. |
| Scheduler | Wall-clock-only check inside `run_task` | **`runtime/scheduler.rs`** with unified wall + token budgets, soft-preempt threshold (logged) at 80%, hard-preempt that force-injects `<|halt|>status=partial`. Equivalent to the Windows 3.1→95 jump (design §2). |
| Multi-task | Single task per iod process | **`runtime/multitask.rs`**. `Task` + `run_all` spawn N worker threads against one llama-server using `id_slot` for KV-cache isolation. `--parallel N` on the server side maps slot-per-task. |
| Capability enforcement | Manifest `allowed_opcodes` was informational | **`runtime/capability.rs`** queries llama-server's `/tokenize` for opcode token IDs, builds a `logit_bias` array banning unauthorized opcodes. Prefers unique constituent tokens to minimize false positives on bootstrap kernels. |
| Subagent cartridges | `subagent: true` was rejected by the loader | **`runtime/subagent.rs`** + `cart/system/summarize/`. Subagent cartridges get an injected `LLMProxy` and can call `proxy.chat()` to do sub-inference. Built-in subagents only in v0.5; dynamic loading is v1.0. |
| Self-hosting trace pipeline | Trace flag exists; no curation | **`scripts/promote_traces.py`** — reads JSONL traces, filters successful trajectories, dedupes, emits DPO triples ready for the fine-tune. The Linux-0.11 moment: the OS curates its own training data. |

---

## What ships

### New runtime modules

- `runtime/scheduler.rs` (~150 lines, 4 tests) — unified wall+token budgets, hard preemption.
- `runtime/multitask.rs` (~110 lines, 1 test) — thread-per-task pool against llama-server slots.
- `runtime/capability.rs` (~180 lines, 3 tests) — logit-bias construction with unique-token preference.
- `runtime/subagent.rs` (~170 lines, 3 tests) — Subagent trait + LLMProxy + CloudProxy + summarize built-in.

### iod.rs changes

- `Daemon::run_task` consults `Scheduler` before each segment; hard preempt force-injects `<|halt|>status=partial` regardless of model preference.
- `DaemonConfig` adds `max_tokens_per_task` (default 32 768) and `slot_id` (Option, used by multi-task layer).
- `CompletionRequest` includes `id_slot` (skipped if None for back-compat with single-task v0.1 servers).
- `Statement::Call` for `subagent: true` cartridges routes through `runtime::subagent` with a CloudProxy.
- Schema-violation results now include `expected_grammar` from the per-method compiled GBNF.

### New cartridge

- `cart/system/summarize/` — reference subagent. `summarize({text, max_bullets?})` → `{ok, summary, ...}`.

### New scripts/docs

- `scripts/promote_traces.py` — JSONL traces → DPO triples. Idempotent (sorted + content-hashed).
- `docs/grammar-swap-design.md` — full spec for the v1.0 decode-time mid-stream swap.

---

## What's still deferred to v1.0

Per `docs/grammar-swap-design.md`:

- **True decode-time mid-stream grammar swap.** The phase machine (A→B→C→D) is specced. Implementation gated on measurement: if v0.5 traces show > 2% schema-violation rate, v1.0 prioritizes; if < 0.5%, defer to v1.5.
- **Soft-preempt logit-bias toward halt.** v0.5 logs the soft-preempt event; v1.0 wires the actual bias once kernel-specific halt-token IDs are known via the capability layer.
- **Dynamic subagent loading** — WASM-sandboxed cartridges loaded from disk. v0.5 only resolves built-ins.
- **Recursive subagent calls** — subagent A invokes subagent B. Capability propagation rules need design before implementation.
- **Soft scheduler with round-robin across slots.** v0.5 uses thread-per-task; v1.0 may switch to a true scheduler once measurements show whether per-thread cost matters on Pi 5.

---

## How to use the new pieces

### Multi-task

Start llama-server with `--parallel 4`:

```bash
~/llama.cpp/build/bin/llama-server \
    --model ~/models/gemma-4-e2b-q4.gguf \
    --port 8080 --parallel 4 --ctx-size 32768
```

Then run multiple tasks concurrently from a Rust caller:

```rust
use llm_os_runtime::iod::DaemonConfig;
use llm_os_runtime::multitask::{from_goals, run_all};

let cfg = DaemonConfig::default();
let tasks = from_goals(
    vec![
        "Plan a weekly menu for 4.".into(),
        "Drive skillos_robot forward 50cm.".into(),
        "Reach (3,0) in sim_world.".into(),
    ],
    cfg,
);
let results = run_all(tasks).unwrap();
for r in results {
    println!("{}: {:?}", r.goal, r.outcome);
}
```

### Subagent cartridges

Already wired — just emit a regular `<|call|>summarize.summarize` from any task. The daemon detects `subagent: true` on the cartridge manifest and routes through the injected proxy. Cloud creds come from the standard `LLM_OS_CLOUD_*` env vars (`docs/dual-brain-deployment.md`).

### Trace promotion

After collecting ≥ 5 000 traces:

```bash
python3 scripts/promote_traces.py \
    --traces traces/*.jsonl \
    --out out/dpo.jsonl \
    --min-success 3 \
    --max-pairs-per-goal 10
```

Then feed `out/dpo.jsonl` into the fine-tune per `docs/fine-tune-recipe.md` §4.

### Capability enforcement

Currently used by daemon code paths only — there's no CLI flag yet. v1.0 adds `iod --ban-opcodes fault,fork` to construct a per-task logit_bias from the cartridge manifest. The infrastructure (`runtime/capability.rs`) is in place; CLI wiring waits for measured per-task overhead to confirm the tokenize-roundtrip cost is acceptable on Pi 5.

---

## Real findings from the v0.5 sprint

1. **Bootstrap-kernel capability enforcement has irreducible false positives.** With multi-token opcodes, banning the unique-constituent token works for the common case (5–8 of the 13 opcodes), but ~5 opcodes share constituent tokens with each other and with non-opcode usage. The fine-tuned kernel (post-v0.1) makes this exact.
2. **Mid-stream grammar swap costs more than expected.** The simplest implementation requires 3 server requests per call (pre-call, name-resolve, args). At ~50 ms per round-trip on Pi 5, that's a 15% latency tax on every call. Whether it's worth paying depends on the v0.5 measurement of schema-violation rate; design doc updated accordingly.
3. **Multi-task isolation via `id_slot` works but llama-server's slot pool is sized at server startup.** Means you can't dynamically scale concurrency — restart server to change `--parallel N`. Documented in `RELEASE_NOTES_v0.5` "How to use" section.

---

## v0.5 final gate

Before tagging `v0.5` (no `-rc`):

- [ ] User runs `tests/e2e/navigate_and_report.sh` modified to launch 4 concurrent runs against `--parallel 4`. All 40 succeed.
- [ ] `runtime/capability.rs` benchmarked end-to-end: tokenize-roundtrip cost per task ≤ 100 ms on Pi 5.
- [ ] Subagent path measured: a `summarize` call's added latency vs raw cloud call ≤ 50 ms.
- [ ] Trace promotion run on ≥ 5 000 real traces; output DPO file passes `jq` schema check.
- [ ] Schema-violation rate measured from real traces; informs whether to land v1.0 mid-stream swap or defer.
