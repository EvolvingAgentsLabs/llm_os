# LLM-OS kernel

A reusable, embeddable kernel for grammar-constrained LLM execution. Pure JS, zero build step, no bundler required.

The kernel is what makes LLM-OS an OS rather than a chatbot: it enforces, at the sampler level, that every emitted token sequence is a syntactically valid syscall against a declared cartridge. Wrong opcodes are physically impossible to emit — not filtered, not retried, never sampled.

## Modules

| File | Role |
|---|---|
| [token_trie.js](token_trie.js) | Trie of token-ID sequences. Insert opcode token sequences; query valid next tokens at each generation step. |
| [cartridge.js](cartridge.js) | Cartridge class: load a manifest, build the trie via the active model's tokenizer, expose method/halt opcode index sets. |
| [sampler.js](sampler.js) | Backend-agnostic sampler. Picks the highest-probability valid next token at each step; tracks fallback rate. Takes any Backend that exposes `tokenize`/`detokenize`/`decode`/`samplingInit`/`samplingAccept`/`getLogits`/`kvClear` — the raw wllama instance from `@wllama/wllama` is itself a valid Backend (no wrapper needed for the browser demo case). |
| [dispatch.js](dispatch.js) | Parse generated text into `{type: "call", cartridge, method, args}` or `{type: "halt", status}`. |
| [schemas/cartridge.manifest.schema.json](schemas/cartridge.manifest.schema.json) | JSON Schema for cartridge manifests. |
| [index.js](index.js) | Re-exports. Single import point: `import { Cartridge, Sampler, parseOpcode } from './kernel/index.js'`. |

## Usage

```js
import { Wllama } from 'https://cdn.jsdelivr.net/npm/@wllama/wllama@2.4.0/esm/index.js';
import { Cartridge, Sampler, parseOpcode, formatResult } from './kernel/index.js';

// 1. Load model. The wllama instance IS a valid Backend.
const wllama = new Wllama({ /* paths */ });
await wllama.loadModelFromHF('LiquidAI/LFM2.5-350M-GGUF', 'LFM2.5-350M-Q4_K_M.gguf');

// 2. Define cartridge
const manifest = {
  name: 'echo',
  methods: {
    say: {
      opcodes: [
        '<|call|>echo.say {"text":"hello"}<|/call|>\n',
        '<|call|>echo.say {"text":"world"}<|/call|>\n',
      ],
    },
  },
};
const cartridge = new Cartridge(manifest);
await cartridge.build(s => wllama.tokenize(s));

// 3. Sample. Any Backend works — wllama, a worker proxy, a future FFI binding.
const sampler = new Sampler(wllama, cartridge.trie);
const allowed = cartridge.methodIndices('say');  // restrict to .say opcodes
const result = await sampler.generate(prompt, { allowedOpcodes: allowed });

// 4. Parse
const op = parseOpcode(result.text);
// op = { type: 'call', cartridge: 'echo', method: 'say', args: { text: 'hello' }, ... }
```

## Backend interface

`Sampler` does not depend on `@wllama/wllama` directly. It calls a Backend
that implements seven methods:

```ts
interface Backend {
  tokenize(text: string): Promise<number[]>;
  detokenize(tokens: number[]): Promise<Uint8Array>;
  decode(tokens: number[], opts?: object): Promise<unknown>;
  samplingInit(opts: { temp?: number; top_k?: number; top_p?: number }): Promise<unknown>;
  samplingAccept(tokens: number[]): Promise<unknown>;
  getLogits(idx: number): Promise<Array<{ token: number; p: number }>>;
  kvClear(): Promise<unknown>;
}
```

`@wllama/wllama` exposes exactly these methods, so it is a Backend by
duck typing — pass the wllama instance directly as the first arg of
`new Sampler(...)`.

For deployments where wllama runs inside a Web Worker (e.g.
`skillos_mini`'s `mobile/src/lib/llm/local/wllama_worker.ts`), implement
the same interface as a thin proxy that forwards each call across the
worker message channel. This keeps the kernel transport-agnostic.

## Manifest example

```json
{
  "name": "tetris",
  "version": "0.1.0",
  "methods": {
    "move": {
      "summary": "Move the active piece.",
      "opcodes": [
        "<|call|>tetris.move {\"action\":\"left\"}<|/call|>\n",
        "<|call|>tetris.move {\"action\":\"right\"}<|/call|>\n",
        "<|call|>tetris.move {\"action\":\"down\"}<|/call|>\n",
        "<|call|>tetris.move {\"action\":\"rotate\"}<|/call|>\n",
        "<|call|>tetris.move {\"action\":\"drop\"}<|/call|>\n"
      ]
    },
    "observe": { "opcodes": ["<|call|>tetris.observe {}<|/call|>\n"] },
    "reset":   { "opcodes": ["<|call|>tetris.reset {}<|/call|>\n"]   }
  }
}
```

## Design constraints (currently)

- **Enumerable opcodes only.** Every opcode the model can emit must be declared as a complete string in the manifest. Templated opcodes (free-form JSON inside `{...}`) are not yet supported — they need a different sampling strategy (e.g., grammar-driven JSON synthesis inside the trie's free slots).
- **Tokenizer compatibility.** The trie is built by tokenizing opcode strings against the active model's tokenizer. Models that tokenize `<|call|>` etc. as multi-token sequences will work, but suffer fallback-rate degradation (the trie's valid-next set rarely intersects the top-K logits). LFM 2.5 is the canonical "works well" reference; Qwen 2.5 0.5B and Gemma 4 do not without a wider logit window.
- **Single cartridge per session.** The current `Cartridge` class wraps one manifest. Multi-cartridge sessions need a `CartridgeRegistry` that builds a unified trie — slated for the next iteration.

## Testing

The kernel is consumed by [`demo/tetris-browser/index.html`](../demo/tetris-browser/index.html). If the Tetris demo loads and plays, the kernel works. A standalone test harness is a future task.
