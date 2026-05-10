// kernel/cartridge.js
// A Cartridge declares a set of opcode strings the LLM-CPU is allowed to
// emit. The kernel turns the cartridge manifest into a TokenTrie at
// load-time by tokenizing each opcode string with the active model's
// tokenizer.
//
// Manifest schema: kernel/schemas/cartridge.manifest.schema.json

import { TokenTrie } from './token_trie.js';

export class Cartridge {
  constructor(manifest) {
    this.manifest = manifest;
    this.name = manifest.name;
    this.version = manifest.version ?? '0.0.0';
    this.description = manifest.description ?? '';
    this.methods = manifest.methods ?? {};
    this.halt = manifest.halt ?? [
      '<|halt|>status=success\n',
      '<|halt|>status=failure\n',
      '<|halt|>status=partial\n',
    ];
    // Maps populated after build():
    this.methodOpcodeIndices = new Map();   // methodName -> Set<opcodeIndex>
    this.haltOpcodeIndices = new Set();
    this.trie = null;
  }

  // Build the trie. tokenize is an async function (string -> Promise<number[]>).
  async build(tokenize) {
    this.trie = new TokenTrie();
    for (const [methodName, methodDef] of Object.entries(this.methods)) {
      const indices = new Set();
      for (const opcodeString of methodDef.opcodes ?? []) {
        const tokens = await tokenize(opcodeString);
        const idx = this.trie.insert(opcodeString, tokens, `${this.name}.${methodName}`);
        indices.add(idx);
      }
      this.methodOpcodeIndices.set(methodName, indices);
    }
    for (const haltString of this.halt) {
      const tokens = await tokenize(haltString);
      const idx = this.trie.insert(haltString, tokens, `__halt__`);
      this.haltOpcodeIndices.add(idx);
    }
    return this.trie;
  }

  // Convenience: opcode index sets for phase control
  allMethodIndices() {
    const all = new Set();
    for (const set of this.methodOpcodeIndices.values()) {
      for (const idx of set) all.add(idx);
    }
    return all;
  }

  methodIndices(...methodNames) {
    const result = new Set();
    for (const name of methodNames) {
      const set = this.methodOpcodeIndices.get(name);
      if (set) for (const idx of set) result.add(idx);
    }
    return result;
  }

  haltIndices() {
    return new Set(this.haltOpcodeIndices);
  }
}

// Validate a manifest against the schema. Returns {ok: true} or {ok: false, errors: [...]}.
// Lightweight validator — covers the structural invariants that matter for the trie.
export function validateManifest(manifest) {
  const errors = [];
  if (!manifest || typeof manifest !== 'object') {
    return { ok: false, errors: ['manifest must be an object'] };
  }
  if (typeof manifest.name !== 'string' || !manifest.name.length) {
    errors.push('manifest.name is required (non-empty string)');
  }
  if (!manifest.methods || typeof manifest.methods !== 'object') {
    errors.push('manifest.methods is required (object)');
  } else {
    for (const [methodName, methodDef] of Object.entries(manifest.methods)) {
      if (!methodDef.opcodes || !Array.isArray(methodDef.opcodes) || methodDef.opcodes.length === 0) {
        errors.push(`method "${methodName}": opcodes must be a non-empty array`);
        continue;
      }
      for (let i = 0; i < methodDef.opcodes.length; i++) {
        const op = methodDef.opcodes[i];
        if (typeof op !== 'string' || !op.length) {
          errors.push(`method "${methodName}".opcodes[${i}]: must be a non-empty string`);
        }
      }
    }
  }
  if (manifest.halt !== undefined) {
    if (!Array.isArray(manifest.halt)) {
      errors.push('manifest.halt must be an array of strings');
    } else {
      for (let i = 0; i < manifest.halt.length; i++) {
        if (typeof manifest.halt[i] !== 'string') {
          errors.push(`manifest.halt[${i}]: must be a string`);
        }
      }
    }
  }
  return errors.length ? { ok: false, errors } : { ok: true };
}
