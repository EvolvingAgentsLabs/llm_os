// kernel/sampler.js
// Token-trie-constrained sampler. Drives wllama at the logits level:
// at each step, looks up valid next tokens from the trie, picks the
// highest-probability one from the model's logits, falls back to the
// first valid token if none are in top-K.

export class Sampler {
  constructor(wllama, trie, opts = {}) {
    this.wllama = wllama;
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
    await this.wllama.kvClear();
    this.kvCacheLen = 0;
  }

  // Generate one complete opcode (or up to maxTokens if it stalls).
  // prompt: full current prompt string.
  // allowedOpcodes: optional Set<number> of opcode indices to allow.
  // Returns: { tokens, text, opcodeIndex, stalled, fellBackSteps }.
  async generate(prompt, { maxTokens = 100, allowedOpcodes = null, onProgress = null } = {}) {
    await this.wllama.samplingInit({ temp: this.temp, top_k: this.top_k, top_p: this.top_p });

    const promptTokens = await this.wllama.tokenize(prompt);
    const newTokens = promptTokens.slice(this.kvCacheLen);

    if (newTokens.length > 0) {
      const totalNeeded = this.kvCacheLen + newTokens.length + maxTokens;
      if (totalNeeded > this.maxContext) {
        await this.wllama.kvClear();
        this.kvCacheLen = 0;
        await this.wllama.decode(promptTokens, {});
        this.kvCacheLen = promptTokens.length;
      } else {
        await this.wllama.decode(newTokens, {});
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

      const logits = await this.wllama.getLogits(-1);
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

      await this.wllama.samplingAccept([bestToken]);
      await this.wllama.decode([bestToken], {});
    }

    this.kvCacheLen += generatedTokens.length;

    let text = '';
    let opcodeIndex = -1;
    if (generatedTokens.length > 0) {
      const bytes = await this.wllama.detokenize(generatedTokens);
      text = new TextDecoder().decode(bytes);
      opcodeIndex = this.trie.getOpcodeIndex(generatedTokens);
    }
    return { tokens: generatedTokens, text, opcodeIndex, stalled, fellBackSteps };
  }
}
