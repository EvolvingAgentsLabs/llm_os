// kernel/sampler.js
// Token-trie-constrained sampler. At each step, looks up valid next
// tokens from the trie, picks the highest-probability one from the
// model's logits, falls back to the first valid token if none are in
// top-K.
//
// The sampler talks to a Backend interface (JSDoc typedef below). The
// raw wllama instance is itself a valid Backend — no wrapper needed for
// the browser demo case. Other deployments (wllama-in-Web-Worker, a
// future native llama.cpp FFI binding, etc.) implement the interface
// and pass it in.

/**
 * @typedef {Object} Backend
 *
 * Methods the sampler calls. Any implementation that exposes these — the
 * raw wllama from `@wllama/wllama`, a worker proxy, an FFI binding —
 * works as a Backend.
 *
 * @property {(text: string) => Promise<number[]>} tokenize
 *   Convert a string to token IDs.
 * @property {(tokens: number[]) => Promise<Uint8Array>} detokenize
 *   Convert token IDs back to UTF-8 bytes.
 * @property {(tokens: number[], opts?: object) => Promise<unknown>} decode
 *   Feed tokens through the model, advancing the KV cache.
 * @property {(opts: {temp?: number, top_k?: number, top_p?: number}) => Promise<unknown>} samplingInit
 *   Initialize sampling parameters.
 * @property {(tokens: number[]) => Promise<unknown>} samplingAccept
 *   Mark tokens as accepted into the sampling state.
 * @property {(idx: number) => Promise<Array<{token: number, p: number}>>} getLogits
 *   Return logits as {token, p} entries, top-K. -1 means "all available".
 * @property {() => Promise<unknown>} kvClear
 *   Clear the KV cache.
 */

export class Sampler {
  /**
   * @param {Backend} backend
   * @param {import('./token_trie.js').TokenTrie} trie
   * @param {{maxContext?: number, temp?: number, top_k?: number, top_p?: number}} [opts]
   */
  constructor(backend, trie, opts = {}) {
    this.backend = backend;
    this.trie = trie;
    this.maxContext = opts.maxContext ?? 3800;
    this.temp = opts.temp ?? 0.5;
    this.top_k = opts.top_k ?? 0;
    this.top_p = opts.top_p ?? 1.0;
    // KV-cache state
    this.kvCacheLen = 0;
  }

  resetKv() { this.kvCacheLen = 0; }

  async kvClear() {
    await this.backend.kvClear();
    this.kvCacheLen = 0;
  }

  // Generate one complete opcode (or up to maxTokens if it stalls).
  // prompt: full current prompt string.
  // allowedOpcodes: optional Set<number> of opcode indices to allow.
  // Returns: { tokens, text, opcodeIndex, stalled, fellBackSteps }.
  async generate(prompt, { maxTokens = 100, allowedOpcodes = null, onProgress = null } = {}) {
    await this.backend.samplingInit({ temp: this.temp, top_k: this.top_k, top_p: this.top_p });

    const promptTokens = await this.backend.tokenize(prompt);
    const newTokens = promptTokens.slice(this.kvCacheLen);

    if (newTokens.length > 0) {
      const totalNeeded = this.kvCacheLen + newTokens.length + maxTokens;
      if (totalNeeded > this.maxContext) {
        await this.backend.kvClear();
        this.kvCacheLen = 0;
        await this.backend.decode(promptTokens, {});
        this.kvCacheLen = promptTokens.length;
      } else {
        await this.backend.decode(newTokens, {});
        this.kvCacheLen = promptTokens.length;
      }
    }

    const generatedTokens = [];
    let fellBackSteps = 0;
    let stalled = false;

    for (let step = 0; step < maxTokens; step++) {
      const validSet = this.trie.getValidNextTokens(generatedTokens, allowedOpcodes);
      if (validSet.size === 0) {
        if (this.trie.isComplete(generatedTokens)) break;
        stalled = true;
        break;
      }

      const logits = await this.backend.getLogits(-1);
      let bestToken = -1;
      let bestProb = -1;
      for (const entry of logits) {
        if (validSet.has(entry.token) && entry.p > bestProb) {
          bestProb = entry.p;
          bestToken = entry.token;
        }
      }
      if (bestToken < 0) {
        // None of the valid tokens were in top-K. Pick first-valid-by-iteration —
        // deterministic but non-strategic. Track this so callers can flag low-quality runs.
        bestToken = [...validSet][0];
        fellBackSteps++;
      }

      generatedTokens.push(bestToken);
      if (onProgress) onProgress({ step, tokenCount: generatedTokens.length });

      if (this.trie.isComplete(generatedTokens)) break;

      await this.backend.samplingAccept([bestToken]);
      await this.backend.decode([bestToken], {});
    }

    this.kvCacheLen += generatedTokens.length;

    let text = '';
    let opcodeIndex = -1;
    if (generatedTokens.length > 0) {
      const bytes = await this.backend.detokenize(generatedTokens);
      text = new TextDecoder().decode(bytes);
      opcodeIndex = this.trie.getOpcodeIndex(generatedTokens);
    }
    return { tokens: generatedTokens, text, opcodeIndex, stalled, fellBackSteps };
  }
}
