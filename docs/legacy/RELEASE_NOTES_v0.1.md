# LLM-OS v0.1-rc1 — Release Notes

*Tag: `v0.1-rc1`. Released 2026-04-25, 2 days after `v0.01`. The "rc" denotes that the kernel fine-tune itself runs on the user's GPU host per [`docs/fine-tune-recipe.md`](docs/fine-tune-recipe.md); v0.1 final tag waits for measured kernel-v0.1 numbers landing in [`docs/projection-rebaseline-2026-06.md`](docs/projection-rebaseline-2026-06.md).*

---

## What's new vs. v0.01

| Area | v0.01 | v0.1-rc1 |
|---|---|---|
| Cartridge handlers (`cooking`, `residential_electrical`, `demo`) | Stubs returning `{"ok":true,"stub":"v0.01",...}` | **Real Rust implementations** in `runtime/handlers.rs`. Cooking emits 7-day menus + shopping lists + recipes. Residential-electrical does IEC-60364 circuit sizing. Demo does djb2/fnv1a hashes + echo. |
| Per-cartridge args grammar | Runtime JSON-schema validation only | **Compiled GBNF sub-grammars** per method. `runtime/schema_to_gbnf.rs` + `compile_schemas` binary materialize them as `.gbnf` artifacts beside each schema. v0.5 wires them into mid-stream grammar swap; v0.1 uses them for documentation + the v0.5 prep path. |
| Cloud routing | `<\|fault\|>{"needs_cloud":true}` only (reactive) | **Proactive tier routing** in `iod.rs`. Cartridges with `preferred_tier: "cloud"` are routed straight to the cloud provider; the call result becomes the model's `<\|result\|>`. Refinement §2.5 wired. |
| Dialect framework | Not present | **`runtime/dialect.rs`** with three built-in dialects: `roclaw-motion-v1`, `roclaw-rotate-v1`, `sim-world-step-v1`. `F 150 150` expands to `{"left":150,"right":150}` — refinement §2.3's 2× compression is now real (waiting on v0.5 grammar swap to be emitted natively). |
| Trace collection | Not present | iod accepts `--trace <jsonl-path>` and appends one line per task per the format documented in `docs/fine-tune-recipe.md` §1. The fine-tune dataset feedstock. |
| Tokenizer rebuild | Not present | `scripts/rebuild_tokenizer.py` (dry-run + diagnostic) + `docs/fine-tune-recipe.md` §2 (the full HF→GGUF round-trip recipe). |
| Fine-tune recipe | Not present | `docs/fine-tune-recipe.md` — DPO with LoRA, hyperparams that have worked on similar 2B-class ISA fine-tunes, acceptance gate. |
| Projection re-baseline | Scheduled for v0.5 in original design | **Done at v0.1-prep** per refinement R7: `docs/projection-rebaseline-2026-04-25.md`. v0.1-final re-baseline (with measured numbers) gates the v0.1 final tag. |

---

## What ships in this tag

### New runtime modules

- `runtime/handlers.rs` (~430 lines, 4 tests) — real cartridge bodies.
- `runtime/schema_to_gbnf.rs` (~280 lines, 8 tests) — JSON Schema → GBNF compiler covering object/array/string/number/integer/boolean/enum/const/$ref. Constructs outside the v0.1 subset fall through to runtime validation only.
- `runtime/dialect.rs` (~180 lines, 7 tests) — three built-in dialects + the routing table.
- `runtime/compile_schemas_main.rs` — `compile_schemas` binary that materializes per-method GBNF artifacts on disk.

### iod.rs changes

- New `--trace <path>` flag; emits one JSONL line per task on completion (success/partial/failure) including: goal, final prompt, raw stream, status, step count, cartridges used, wall-clock seconds.
- `handle_call` separated out and now consults `cartridge.preferred_tier`. If `"cloud"` and a cloud key is configured, the call is proxied to the cloud provider via `runtime/cloud.rs`.

### Cargo.toml changes

- Added `compile_schemas` binary target.

### New docs

- `docs/cartridge-manifest.md` — already in v0.01; unchanged.
- `docs/fine-tune-recipe.md` — **new**. The recipe.
- `docs/projection-rebaseline-2026-04-25.md` — **new**. v0.1-prep re-baseline.

### New scripts

- `scripts/rebuild_tokenizer.py` — diagnostic tokenizer-rebuild script + write-path documentation.

---

## What's still deferred to v0.5

Mid-stream grammar swap is the load-bearing v0.5 piece. The compiled per-method GBNFs sit on disk after `cargo run --release --bin compile_schemas` but iod doesn't yet swap them in mid-task. v0.5's iod will, on entering a `<|call|>cart.method ` segment, switch the request grammar to `<|call|>cart.method <args-sub-grammar> <|/call|>` so the model literally cannot emit invalid args.

Other v0.5 items:
- Preemptive scheduler with mid-stream `<|yield|>` injection.
- KV-cache swap with multi-task isolation.
- Self-hosting trace pipeline (Linux-0.11 moment).
- Capability enforcement at decode via dynamic logit bias.
- Subagent cartridges (the `subagent: true` manifest field reserved in v0.01 starts having semantics).

---

## How to actually run v0.1

The same `bash scripts/quickstart.sh` flow works, with one extra step at the end:

```bash
# 0–4: build everything (same as v0.01)
bash scripts/quickstart.sh

# 5: materialize per-method GBNF artifacts (new in v0.1)
./runtime/target/release/compile_schemas --cart cart
# wrote N sub-grammars; 0 fell back to runtime validation

# 6: run a task — every cartridge now has a real body
./runtime/target/release/iod \
    --server  http://127.0.0.1:8080 \
    --grammar grammar/isa.gbnf \
    --cart    cart \
    --goal    "Plan a weekly menu for a household of 4." \
    --trace   traces/$(date +%Y%m%d).jsonl
```

The `--trace` flag accumulates the dataset for the fine-tune. Plan to run ≥ 5000 successful tasks before invoking `docs/fine-tune-recipe.md`.

To use the proactive cloud route, set `preferred_tier: "cloud"` on a cartridge's manifest and configure `LLM_OS_CLOUD_URL`/`LLM_OS_CLOUD_KEY`/`LLM_OS_CLOUD_MODEL` per `docs/dual-brain-deployment.md`.

---

## Real findings from the v0.1 sprint

1. **GBNF compilation is mostly straightforward but `additionalProperties: false` is a footgun.** v0.1 ignores it (every emitted sub-grammar is essentially "additionalProperties: true" because the GBNF subset can't easily reject extra keys). v0.5 needs `oneOf`-style enumeration of allowed key sets.
2. **`preferred_tier: "cloud"` makes the cartridge JSON-schema-validation step still happen locally even though the call body runs in the cloud.** This is correct — the local schema is the ABI contract, and it should apply regardless of where the body executes.
3. **Trace lines for task budgets longer than ~30 minutes can exceed JSONL's reasonable line length** (multi-MB single lines hurt downstream tooling). v0.5 should split by step or by `<|loop|>` boundary instead of one line per task.

---

## v0.1 final gate

Before tagging `v0.1` (no `-rc`):

- [ ] User runs `docs/fine-tune-recipe.md` end-to-end on their GPU host.
- [ ] `bash scripts/check_tokenizer.sh` against the new kernel shows every opcode → 1 token.
- [ ] `bash tests/e2e/navigate_and_report.sh` against the new kernel shows ≥ 3× wall-clock improvement vs v0.01.
- [ ] `docs/projection-rebaseline-2026-06.md` lands with measured numbers.
