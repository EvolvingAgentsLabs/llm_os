# summarize — full spec

Reference subagent cartridge (`subagent: true`). Demonstrates the v0.5 pattern: a cartridge whose body itself calls the LLM via the injected `LLMProxy`.

## Method

- `summarize({text, max_bullets?})` — returns `{ok, summary, input_chars, max_bullets}`.

## Routing

`subagent: true` cartridges are dispatched through `runtime/subagent.rs` instead of the regular handler table. The daemon constructs a [`CloudProxy`] from the env-var cloud config (per `docs/dual-brain-deployment.md`) and passes it to the subagent's `dispatch`. The subagent decides when (and whether) to call the proxy.

## Why this exists

The `<|fault|>` cloud-fallback path requires the *local* model to recognize it can't proceed and emit a fault. The subagent path lets a *cartridge* delegate to the cloud transparently — the local model sees a normal `<|call|>summarize.summarize` round-trip, never knowing the body was cloud-served.

This matters for tasks like compaction, where the local model running summarization would compete for the very KV cache being compacted.
