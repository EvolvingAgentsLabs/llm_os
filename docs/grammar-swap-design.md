# Mid-stream Grammar Swap — Design (v1.0 work)

*v0.5 ships the compiled per-method GBNFs and uses them in two ways: (1) materialized as `.gbnf` artifacts on disk via `compile_schemas`, (2) surfaced inside the `<|result|>` payload on schema-violation errors so the model sees the exact expected shape on retry. This document specifies the **true decode-time mid-stream swap** that v1.0 will land. Capturing it now so the v1.0 implementation can refer to a stable spec rather than rediscovering the design.*

---

## 0. Why this is hard

The v0.01 / v0.1 daemon issues one streaming request per "segment" with a single grammar applied for the whole segment. To apply a per-method args sub-grammar **only for the args portion of a call**, the daemon must:

1. Detect that the model is about to emit `<|call|>cart.method args <|/call|>`.
2. Stop generation right after `<|call|>cart.method ` (so the cart and method are known).
3. Re-issue with a custom grammar that allows only the per-method args + the close tag.
4. Resume the original ISA grammar after the close tag.

Each call thus costs **3 server requests instead of 1**: one to emit up to `<|call|>cart.method `, one to emit args + `<|/call|>`, one to emit the daemon-injected `<|result|>` and continue. The KV cache survives via `cache_prompt: true` (refinement R1) but the per-request overhead is real (~20–50 ms per round-trip on Pi 5).

The reward: **zero schema-violation retries on the happy path**. With the v0.5 daemon-side validation path, schema violations cost a `<|result|>{"error":"schema_violation",...}<|/result|>` injection plus a full retry segment — easily 200–800 tokens wasted per violation. The break-even is roughly: if more than 1 in 50 calls violate the schema in v0.5 measurement, v1.0 swap pays for itself.

---

## 1. Phase machine

```
        ┌───────────────────────────────────┐
        │ Phase A: ISA-default              │
        │ grammar = isa.gbnf                │
        │ stop    = ["<|call|>"]            │
        └────────────────┬──────────────────┘
                         │ model emits <|call|>
                         ▼
        ┌───────────────────────────────────┐
        │ Phase B: call-name                │
        │ grammar = "<|call|>" cart-name    │
        │           "." method-name " "     │
        │ stop    = [" "]  (after method)   │
        └────────────────┬──────────────────┘
                         │ daemon now knows (cart, method)
                         │ → look up compiled args grammar
                         ▼
        ┌───────────────────────────────────┐
        │ Phase C: call-args                │
        │ grammar = <args-subgrammar>       │
        │           " " "<|/call|>"         │
        │ stop    = ["<|/call|>"]           │
        └────────────────┬──────────────────┘
                         │ daemon dispatches call,
                         │ injects <|result|>...<|/result|>
                         ▼
        ┌───────────────────────────────────┐
        │ Phase D: result-resume            │
        │ grammar = isa.gbnf                │
        │ (back to Phase A on next segment) │
        └───────────────────────────────────┘
```

Cycle: `A → B → C → D → A`. Other opcodes (read/write/wait/think/halt/...) stay in Phase A end-to-end since their args are constrained enough by the parent ISA grammar.

---

## 2. Grammar construction at runtime

Phase B grammar (small, static):

```gbnf
root         ::= "<|call|>" cart-name "." method-name " "
cart-name    ::= [a-z] [a-z0-9_]*
method-name  ::= [a-z] [a-z0-9_]*
```

Phase C grammar (constructed per call):

```gbnf
# === Loaded from compiled args sub-grammar ===
{{compiled_subgrammar_body}}

# === Wrapper appended by the daemon ===
root ::= {{subgrammar_root}} " " "<|/call|>"
```

The subgrammar's `root` rule is renamed to `args-root` and the wrapper's `root` references it. (The compile_schemas binary already produces a top-level `root` rule; v1.0 needs to emit named root rules so multiple sub-grammars can coexist if a call nesting situation arises.)

---

## 3. Dialect integration

Per refinement §2.3, dialects expand a compact textual notation into JSON. With Phase C in place, the args sub-grammar can be the **dialect grammar** instead of the JSON sub-grammar:

```gbnf
# roclaw-motion-v1 dialect:
root ::= ("F" | "B" | "L" | "R") " " byte " " byte
byte ::= [0-9]+
```

Then the model emits `F 150 150` natively; the daemon expands via [`crate::dialect::expand`] before validating against the JSON Schema; the cartridge handler runs as usual. **The 2× compression becomes a 2× decode-time speedup**, not just a wire-format optimization.

---

## 4. Implementation cost

Estimated v1.0 changes to land this:

| Module | Lines added | Risk |
|---|---|---|
| `runtime/iod.rs` — phase machine in `run_task` | ~150 | High (reorder of segment loop) |
| `runtime/iod.rs` — `complete_one_segment` overload taking `(grammar, stop)` | ~50 | Low |
| `runtime/schema_to_gbnf.rs` — emit `args-root` named rule, not `root` | ~10 | Low |
| `runtime/cartridge.rs` — store dialect-grammar variant alongside JSON variant | ~30 | Low |
| New: `docs/grammar-swap-spec.md` (this doc, hardened with measurements) | ~100 | — |
| Integration tests | ~200 | High |

Total: ~540 lines + tests. Two-week sprint at v1.0 pace.

---

## 5. Acceptance gate

v1.0 ships with measured numbers showing:

1. Per-call latency in strict mode ≤ 1.4× lenient mode on Pi 5 (3 requests vs 1 per call, but fewer retries).
2. Schema-violation retry rate drops from v0.5 baseline (target: ≥ 95% reduction).
3. Dialect-emitting calls are ≥ 2× faster than JSON-emitting calls when both are constrained by their respective sub-grammars.
4. `bash tests/e2e/navigate_and_report.sh --strict-call-args` passes 10/10 with no regressions.

---

## 6. Why ship v0.5 without this

The v0.5 daemon-side validation path with `expected_grammar` surfaced in retry messages reduces violation pain without the implementation cost of phase-machine swap. The break-even depends on measured violation rate. v0.5's `--trace` collection lets us measure this directly: count `expected_grammar` results in trace JSONLs after a week of operation. If > 2%, prioritize v1.0 swap; if < 0.5%, defer to v1.5.

This is the principle from refinement §3: don't add features for hypothetical future requirements. Three-request-per-call mid-stream swap is a real cost we shouldn't pay until the data says we have to.
