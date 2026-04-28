# Usage

> Practical guide for someone running the kernel. If you want to *write*
> a cartridge, see [`TUTORIAL.md`](TUTORIAL.md). If you want to
> *understand* the architecture, see [`ARCHITECTURE.md`](ARCHITECTURE.md).

---

## Prerequisites

```
  ┌─────────────────────────────────────────────────────────────┐
  │  llama.cpp     built from source · with --grammar          │
  │  Rust          ≥ 1.75                                      │
  │  Python        ≥ 3.10  (validation scripts only)           │
  │  GGUF model    qwen-2.5-3b-q4 (default) or compatible      │
  │                                                             │
  │  Optional:                                                  │
  │  GEMINI_API_KEY  for cartridges with preferred_tier=cloud  │
  │  Pi 5            for the 8 Hz reference target             │
  └─────────────────────────────────────────────────────────────┘
```

Build llama.cpp first if you haven't:

```bash
git clone https://github.com/ggerganov/llama.cpp ~/llama.cpp
cd ~/llama.cpp
make -j  # or cmake-based; needs --grammar enabled
./llama-server --version    # smoke test
```

## One-shot bootstrap

```bash
git clone https://github.com/EvolvingAgentsLabs/llm_os.git
cd llm_os
./scripts/quickstart.sh
```

What that script does, step by step:

```
  ┌─[ scripts/quickstart.sh ]──────────────────────────────────┐
  │                                                             │
  │  1. cargo build --release            (runtime/)            │
  │  2. ./scripts/validate_grammar.sh    (12/12 fixtures)      │
  │  3. spawn llama-server                                      │
  │       --model qwen-2.5-3b-q4.gguf                          │
  │       --grammar grammar/isa.gbnf                            │
  │       --port 8081                                           │
  │  4. spawn iod                                               │
  │       --listen unix:///run/llm_os/iod                      │
  │       --backend http://localhost:8081                       │
  │       --cart-dir cart/                                      │
  │  5. POST /dispatch with smoke-test goal                     │
  │  6. print trace + Hz                                        │
  │                                                             │
  └─────────────────────────────────────────────────────────────┘
```

If everything goes well you should see:

```
[OK] llama-server   ▸ qwen-2.5-3b-q4.gguf · port 8081
[OK] iod            ▸ unix:///run/llm_os/iod
[OK] cartridges     ▸ 6 mounted
[OK] grammar        ▸ 12/12 fixtures green
▸ smoke test        ▸ 'summarize ./README.md'
▸ trace             ▸ trace_2026-04-26-1542
▸ ops               ▸ call(system.summarize) · halt(success)
▸ steady            ▸ 8.2 Hz · KV=14%
```

## Manual boot

If the quickstart script is too opinionated, here's the manual boot.

### terminal 1 — llama-server

```bash
~/llama.cpp/llama-server \
  --model ~/models/qwen-2.5-3b-q4.gguf \
  --grammar grammar/isa.gbnf \
  --port 8081 \
  --ctx-size 8192 \
  --n-gpu-layers 0      # CPU on Pi 5; flip up on x86
```

### terminal 2 — iod

```bash
RUST_LOG=info \
./target/release/iod \
  --listen unix:///run/llm_os/iod \
  --http localhost:8080 \
  --backend http://localhost:8081 \
  --cart-dir cart/
```

You can also use `--listen 0.0.0.0:8080` for a TCP socket if you don't
care about the Unix socket boundary (you should — see §6 capability).

### terminal 3 — dispatch a goal

```bash
curl -X POST http://localhost:8080/dispatch \
     -H 'Content-Type: application/json' \
     -d '{"goal":"summarize ./grammar/isa-spec.md"}'
```

Response:
```json
{
  "trace_id": "trace_2026-04-26-1542-a7f3",
  "tier": "local",
  "ops": [
    {"op":"call","cart":"system.summarize","args":{"path":"./grammar/isa-spec.md"},"ms":18},
    {"op":"halt","status":"success"}
  ],
  "kv_pct": 14,
  "hz": 8.2
}
```

## Endpoints reference

```
  ┌─[ iod HTTP API ]─────────────────────────────────────────────┐
  │                                                               │
  │  POST /dispatch                                               │
  │       body: {"goal":"...", "tier":"local|cloud|auto"}         │
  │       returns: trace + final state                            │
  │                                                               │
  │  GET  /trace?n=10                                             │
  │       returns: last N dispatch traces                          │
  │                                                               │
  │  GET  /trace/{trace_id}                                       │
  │       returns: full token-by-token trace + KV timeline        │
  │                                                               │
  │  GET  /policy                                                 │
  │       returns: current capability mask                         │
  │                                                               │
  │  POST /policy                                                 │
  │       body: {"allow":["system.*"], "deny":["io.*.shutdown"]}  │
  │       returns: new mask · takes effect on next dispatch        │
  │                                                               │
  │  GET  /cartridges                                             │
  │       returns: list of mounted cartridges + methods            │
  │                                                               │
  │  GET  /health                                                 │
  │       returns: liveness · KV util · backend Hz                 │
  │                                                               │
  └───────────────────────────────────────────────────────────────┘
```

## Tier selection · local vs cloud

Three ways to control which sampler runs the dispatch:

### per-request (`tier` in dispatch body)

```bash
curl -X POST http://localhost:8080/dispatch \
     -d '{"goal":"...", "tier":"cloud"}'
```

### per-cartridge (`preferred_tier` in manifest)

```json
{
  "name": "domestic.cooking.weekly_menu",
  "preferred_tier": "cloud",
  "methods": ["plan_week"]
}
```

Cartridges that explicitly declare `cloud` will pull the dispatch up
to the cloud sampler whenever they're invoked.

### auto (default)

The `auto` tier delegates to the cartridge's `preferred_tier`, falling
back to `local` if cloud isn't configured.

```
  ┌─[ TIER MATRIX ]────────────────────────────────────────────┐
  │                                                              │
  │  request    cart.preferred    chosen                         │
  │  ───────    ──────────────    ──────                         │
  │  local      *                 local                          │
  │  cloud      *                 cloud                          │
  │  auto       local             local                          │
  │  auto       cloud             cloud (or local if no key)     │
  │                                                              │
  └──────────────────────────────────────────────────────────────┘
```

Configure the cloud worker in `~/.config/llm_os/cloud.toml`:

```toml
[cloud]
provider = "gemini"
api_key  = "${GEMINI_API_KEY}"
model    = "gemini-2.5-flash"
endpoint = "https://generativelanguage.googleapis.com/v1beta"
```

## Capability · the policy mask

The default mask grants every mounted cartridge. To restrict, POST a
new mask:

```bash
curl -X POST http://localhost:8080/policy -d '{
  "allow": ["system.*", "sim.*"],
  "deny":  ["io.roclaw.shutdown", "domestic.residential-electrical.*"]
}'
```

This rebuilds the logit bias mask in the sampler **and** the daemon-side
allow/deny lists. The redundancy is intentional — see
[`ARCHITECTURE.md §6`](ARCHITECTURE.md).

Inspect the current mask:

```bash
curl http://localhost:8080/policy | jq
```

A typical output:

```json
{
  "version": 17,
  "allow": ["system.*", "sim.sim_world.*"],
  "deny":  ["io.roclaw.shutdown"],
  "default": "deny",
  "ops_banned": ["fork", "wait"],
  "logit_bias": { "fork_token": -1e9, "wait_token": -1e9 }
}
```

## Inspecting a dispatch

Every dispatch produces a trace. By default they live in
`~/.cache/llm_os/traces/`.

```
  ┌─[ trace_2026-04-26-1542-a7f3.json ]──────────────────────────┐
  │                                                               │
  │  {                                                            │
  │    "goal": "summarize ./grammar/isa-spec.md",                 │
  │    "tier": "local",                                           │
  │    "tokens_in": 142,                                          │
  │    "tokens_out": 87,                                          │
  │    "duration_ms": 4920,                                       │
  │    "hz_avg": 8.1,                                             │
  │    "kv_max_pct": 24,                                          │
  │    "ops": [                                                   │
  │      { "op": "think", "len_chars": 38, "kept": true },        │
  │      { "op": "call",  "cart": "system.summarize",             │
  │        "args": {"path":"..."}, "schema_ok": true,             │
  │        "result_ms": 18 },                                     │
  │      { "op": "halt",  "status": "success" }                   │
  │    ],                                                         │
  │    "compactions": 0,                                          │
  │    "schema_violations": 0,                                    │
  │    "faults": 0                                                │
  │  }                                                            │
  │                                                               │
  └───────────────────────────────────────────────────────────────┘
```

The trace is the single source of truth for performance + correctness
debugging. If `hz_avg` drops, look at `compactions` and
`schema_violations`. If a cartridge times out, look at `result_ms`
in its `op` entry.

## Troubleshooting

### "the sampler stalls"

Most likely a per-syscall grammar swap is taking too long
(see [`NEXT_STEPS.md §1`](NEXT_STEPS.md) for the v1.0 fix). Quick
diagnostic:

```bash
curl http://localhost:8080/trace?n=1 | jq '.ops[].grammar_swap_ms'
```

If you see > 100 ms per call, you're hitting the HTTP overhead. Cut to
the static grammar by passing `--no-per-method-gbnf` to `iod` until v1.0
ships.

### "schema violation loop"

Symptom: trace shows the same `<|call|>` repeatedly with `schema_ok:
false`. The model didn't understand why it failed.

```bash
# v0.5 escape hatch — set strict three-strikes:
iod ... --max-schema-strikes 3
```

After 3 strikes, the daemon emits `<|fault|>{"needs_cloud":true}` and
escalates to the cloud sampler. See
[`NEXT_STEPS.md §3`](NEXT_STEPS.md).

### "compaction broke my dispatch"

Symptom: a long-running task fails at ~70% KV utilization with a
grammar parse error.

The current compactor is not yet ISA-aware (see
[`NEXT_STEPS.md §2`](NEXT_STEPS.md)). Workarounds:

- Run with `--no-compact` to disable compaction (your dispatch must
  fit in the context window).
- Increase context: `--ctx-size 16384` if your model supports it.
- Break the goal into smaller dispatches.

### "permission denied for cartridge"

Check the policy mask:

```bash
curl http://localhost:8080/policy
```

If the cartridge is in `deny` or not in `allow`, update with:

```bash
curl -X POST http://localhost:8080/policy -d '{
  "allow": ["system.*", "io.roclaw.forward", "io.roclaw.rotate"]
}'
```

### "model emits malformed ISA tokens"

The bootstrap uses multi-token string patterns. Some models split
`<|call|>` into multiple tokens which the daemon catches but the
sampler bias misses.

```bash
# Check what's actually in the tokenizer:
./scripts/check_tokenizer.sh ~/models/qwen-2.5-3b-q4.gguf
```

If you see `<|call|>` listed as 3+ tokens, you're on a model that
needs the v1.0 single-token fine-tune
([`NEXT_STEPS.md §6`](NEXT_STEPS.md)). Daemon-side reject still works
in the meantime.

## Quick reference

```
  ┌─[ COMMANDS ]──────────────────────────────────────────────────────┐
  │                                                                    │
  │  ./scripts/quickstart.sh                  full bootstrap          │
  │  ./scripts/validate_grammar.sh            run 12/12 fixtures      │
  │  ./scripts/check_tokenizer.sh <gguf>      audit ISA token splits  │
  │  ./scripts/promote_traces.py              archive successful runs │
  │  cargo run --bin iod -- --help            iod CLI flags           │
  │                                                                    │
  │  curl :8080/dispatch -d '{"goal":"..."}'  start a run             │
  │  curl :8080/trace                         last traces             │
  │  curl :8080/policy                        capability mask         │
  │  curl :8080/cartridges                    mounted syscalls        │
  │  curl :8080/health                        liveness                │
  │                                                                    │
  └────────────────────────────────────────────────────────────────────┘
```

Now go author your own cartridge → [`TUTORIAL.md`](TUTORIAL.md).
