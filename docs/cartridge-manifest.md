# Cartridge Manifest — v0.01

A *cartridge* is a sealed bundle of methods callable from the LLM-OS via `<|call|>cart.method {…} <|/call|>`. Cartridges are the LLM-OS equivalent of loadable kernel modules: each one declares its own method surface (the syscalls it exposes), the JSON Schema for each method's arguments, and which ISA opcodes it is permitted to be a dispatch target of.

This document specifies the v0.01 manifest format. The format is forward-compatible with v0.1 (which adds compiled per-method GBNF sub-grammars) and v0.5 (subagent cartridges, capability enforcement via dynamic logit bias).

---

## Layout

Cartridges live under a hierarchical root, mirroring the SkillOS lazy-loading skill tree (refinement §2.2). On a Pi 5 with an 8K-effective context window, the hierarchy is the difference between "kernel + 2 cartridges fit" and "kernel + 8 cartridges fit."

```
cart/
  index.md                       Top-level index: lists domains, one line each.
  <domain>/
    index.md                     Per-domain index: lists cartridges, one line each.
    <name>/
      manifest.json              This document's spec.
      full.md                    Optional human/model spec (lazy-loaded).
      schemas/                   Per-method JSON Schemas referenced from manifest.
        <method>.args.schema.json
        [<method>.result.schema.json]   v0.1+
```

The daemon walks two levels at startup (`cart/<domain>/<name>/manifest.json`). Other layouts (deeper nesting, flat) are not supported in v0.01 — keep it predictable.

---

## Manifest schema

A `manifest.json` is a JSON object with the following keys.

| Field | Type | Required | Default | Notes |
|---|---|:---:|---|---|
| `name` | string | ✓ | — | Cartridge dispatch name. Used as the `cart` part of `<\|call\|>cart.method`. Must be a single lower-snake_case token; daemon enforces unique names per registry. |
| `version` | string | ✓ | — | Semver. Bumped on any breaking method change. |
| `description` | string | — | `""` | One-paragraph free text. Surfaces in `cart/<domain>/index.md` and the boot prompt. |
| `allowed_opcodes` | string[] | — | `["call"]` | Subset of the 13 ISA opcodes this cartridge can be a dispatch target of. v0.01: informational. v0.1: enforced via per-task logit bias. |
| `preferred_tier` | `"local"` \| `"cloud"` \| `"auto"` | — | `"auto"` | Routing hint mirroring skillos_mini's tier system. v0.01 always routes locally; v0.1 wires the proactive cloud path so cartridges that say `"cloud"` skip the local model entirely. |
| `methods` | object | ✓ | — | Map of method-name → method spec. See below. |
| `post_validators` | string[] | — | `[]` | Names of post-flow validators (resolved against the daemon's `BUILTIN_VALIDATORS` registry) fired on `<\|halt\|>` to check cross-blackboard invariants. Mirrors skillos_mini post-flow validators. v0.01 logs only; v0.1 enforces. |
| `subagent` | bool | — | `false` | Flags a cartridge that itself runs sub-inference via an injected LLM proxy. Routes through `runtime/subagent.rs` instead of the regular handler table. **v0.5+**: built-in subagents only (`summarize` is the reference). v1.0 adds dynamic loading. |

### Method spec

Each entry in `methods` is an object:

| Field | Type | Required | Default | Notes |
|---|---|:---:|---|---|
| `args_schema` | string (path) | ✓ | — | Path to a JSON Schema file relative to the cartridge dir. Schema is compiled at load time and validated at dispatch. |
| `result_schema` | string (path) | — | `null` | Optional result-shape schema. v0.01 doesn't enforce; v0.1 compiles to a sub-grammar applied to the `<\|result\|>` block. |
| `description` | string | — | `""` | Short description, included in `full.md` and used as model documentation. |

---

## Built-in handlers vs stubs

For v0.01, two cartridges have real handlers wired into the daemon binary:

- **`roclaw`** — methods compile to RoClaw 6-byte UDP frames via the Rust port of `BytecodeCompiler` (refinement §2.8). Frames go to `127.0.0.1:4210` (overridable via `LLM_OS_ROCLAW_ADDR`).
- **`sim_world`** — deterministic grid-world fixture used by the L1 acceptance test. Methods: `step({dir})`, `observe`, `reset`.

Every other cartridge is **stubbed** — `<|call|>cart.method {…} <|/call|>` returns `{"ok":true,"stub":"v0.01","cart":"…","method":"…"}`. Stubs let the daemon exercise the dispatch + validation path end-to-end without committing to per-method semantics that should land with v0.1 fine-tuning.

---

## Worked example

```jsonc
{
  "name": "cooking",
  "version": "0.0.1",
  "description": "Meal planning, recipe authoring, and shopping-list generation.",
  "allowed_opcodes": ["call"],
  "preferred_tier": "auto",
  "methods": {
    "plan_menu": {
      "args_schema": "schemas/plan_menu.args.schema.json",
      "description": "Produce a 7-day menu given household size and optional dietary notes."
    },
    "build_shopping_list": {
      "args_schema": "schemas/build_shopping_list.args.schema.json",
      "description": "Aggregate a weekly menu into an aisle-grouped shopping list."
    }
  },
  "post_validators": ["menu_complete"]
}
```

The cartridge dir then contains `cart/domestic/cooking/{manifest.json, schemas/*.json, full.md}`.

---

## What v0.01 explicitly does NOT do

- **GBNF compilation of per-method schemas.** All schema validation runs at dispatch time in the daemon. v0.1 compiles each method's `args_schema` (and later `result_schema`) into a GBNF sub-grammar swapped into the `<|call|>` arg slot, eliminating the runtime-validation retry path on the happy path.
- **Capability enforcement at decode.** `allowed_opcodes` is informational. v0.1 wires per-cartridge logit bias so a cartridge that doesn't permit `<|fault|>` cannot emit it.
- **Persistent state per cartridge.** `<|commit|>` writes to a per-session JSON file in v0.01. The cartridge-memory abstraction lands in v0.1.
- **Tier-aware proactive cloud routing.** `preferred_tier: "cloud"` is read but ignored in v0.01 — every call routes locally. v0.1 wires the proactive cloud path.
- **Dynamic subagent loading.** `subagent: true` cartridges are accepted in v0.5 but only resolve to built-ins. v1.0 adds WASM-sandboxed loading.
