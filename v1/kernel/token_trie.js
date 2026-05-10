// kernel/token_trie.js
// Token-trie grammar engine. Pre-tokenize all valid opcode strings, build a
// trie of token-ID sequences, and use it at sample time to constrain the
// model's output to grammar-valid completions.
//
// This sidesteps wllama bug #168 (its grammar state machine doesn't advance
// reliably for opcode-heavy grammars). We work at token-ID level — no text
// matching, no detokenize round-trips during sampling.

class TrieNode {
  constructor() {
    this.children = new Map();   // tokenId -> TrieNode
    this.isEnd = false;
    this.opcodeIndex = -1;       // which opcode this completes
  }
}

export class TokenTrie {
  constructor() {
    this.root = new TrieNode();
    this.opcodes = [];           // [{ string, tokens, opcodeIndex, label }]
  }

  // Insert one opcode. `tokens` is the model's tokenization of `string`.
  // `label` is an optional cartridge-method tag for routing diagnostics.
  insert(string, tokens, label = null) {
    const opcodeIndex = this.opcodes.length;
    let node = this.root;
    for (const tok of tokens) {
      if (!node.children.has(tok)) node.children.set(tok, new TrieNode());
      node = node.children.get(tok);
    }
    node.isEnd = true;
    node.opcodeIndex = opcodeIndex;
    this.opcodes.push({ string, tokens, opcodeIndex, label });
    return opcodeIndex;
  }

  // Valid next token IDs given tokens generated so far.
  // allowedSet: optional Set<number> of opcode indices to restrict to.
  getValidNextTokens(generatedTokens, allowedSet = null) {
    let node = this.root;
    for (const tok of generatedTokens) {
      if (!node.children.has(tok)) return new Set();
      node = node.children.get(tok);
    }
    if (!allowedSet) return new Set(node.children.keys());
    const result = new Set();
    for (const [tokId, child] of node.children) {
      if (this._leadsToAllowed(child, allowedSet)) result.add(tokId);
    }
    return result;
  }

  _leadsToAllowed(node, allowedSet) {
    if (node.isEnd && allowedSet.has(node.opcodeIndex)) return true;
    for (const child of node.children.values()) {
      if (this._leadsToAllowed(child, allowedSet)) return true;
    }
    return false;
  }

  isComplete(generatedTokens) {
    let node = this.root;
    for (const tok of generatedTokens) {
      if (!node.children.has(tok)) return false;
      node = node.children.get(tok);
    }
    return node.isEnd;
  }

  getOpcodeIndex(generatedTokens) {
    let node = this.root;
    for (const tok of generatedTokens) {
      if (!node.children.has(tok)) return -1;
      node = node.children.get(tok);
    }
    return node.opcodeIndex;
  }

  getOpcodeString(opcodeIndex) {
    return this.opcodes[opcodeIndex]?.string ?? null;
  }

  getOpcodeLabel(opcodeIndex) {
    return this.opcodes[opcodeIndex]?.label ?? null;
  }

  size() { return this.opcodes.length; }
}
