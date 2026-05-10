// kernel/cartridge.js
// v2 Cartridge: declarative program definition with JSON schema args.
// Unlike v1 (which enumerated every possible argument as a separate trie
// entry), v2 cartridges declare methods with argument schemas. The kernel
// generates GBNF grammar or uses structured output to constrain args.

import { readFile } from 'node:fs/promises';
import { join, dirname } from 'node:path';

export class Cartridge {
  /**
   * @param {object} manifest - Parsed manifest JSON
   * @param {string} [basePath] - Directory containing the manifest (for resolving schema refs)
   */
  constructor(manifest, basePath = '.') {
    this.manifest = manifest;
    this.basePath = basePath;
    this.name = manifest.name;
    this.version = manifest.version ?? '0.0.0';
    this.description = manifest.description ?? '';
    this.methods = manifest.methods ?? {};
    this.ports = manifest.ports ?? {};
    this.halt = manifest.halt ?? ['success', 'failure', 'partial'];
    // Resolved schemas (loaded from files or inline)
    this.resolvedSchemas = new Map();
  }

  /**
   * Load and resolve all argument schemas from files.
   */
  async resolveSchemas() {
    for (const [methodName, methodDef] of Object.entries(this.methods)) {
      if (methodDef.args_schema) {
        if (typeof methodDef.args_schema === 'string') {
          // It's a file reference — load it
          const schemaPath = join(this.basePath, methodDef.args_schema);
          try {
            const raw = await readFile(schemaPath, 'utf-8');
            this.resolvedSchemas.set(methodName, JSON.parse(raw));
          } catch (err) {
            console.warn(`[cartridge] failed to load schema ${schemaPath}: ${err.message}`);
            this.resolvedSchemas.set(methodName, {});
          }
        } else {
          // Inline schema object
          this.resolvedSchemas.set(methodName, methodDef.args_schema);
        }
      } else {
        // No schema — empty args
        this.resolvedSchemas.set(methodName, { type: 'object', properties: {} });
      }
    }
  }

  /**
   * Get the list of method names.
   */
  methodNames() {
    return Object.keys(this.methods);
  }

  /**
   * Get the argument schema for a method.
   */
  getSchema(methodName) {
    return this.resolvedSchemas.get(methodName) || {};
  }

  /**
   * Get method summary/description.
   */
  getSummary(methodName) {
    return this.methods[methodName]?.summary ?? '';
  }

  /**
   * Build the ISA description for the system prompt.
   * Lists all methods with their argument schemas in a compact format
   * the LLM can use to emit correct opcodes.
   */
  buildISADescription() {
    const lines = [`Cartridge: ${this.name} (${this.description})`];
    lines.push('');
    lines.push('Methods:');

    for (const [methodName, methodDef] of Object.entries(this.methods)) {
      const schema = this.resolvedSchemas.get(methodName) || {};
      const args = schema.properties
        ? Object.entries(schema.properties).map(([k, v]) => {
            const enumStr = v.enum ? ` (one of: ${v.enum.join(', ')})` : '';
            const typeStr = v.type || 'any';
            return `${k}: ${typeStr}${enumStr}`;
          }).join(', ')
        : '{}';
      const summary = methodDef.summary || '';
      lines.push(`  <|call|>${this.name}.${methodName} {${args}}<|/call|>  -- ${summary}`);
    }

    if (this.ports && Object.keys(this.ports).length > 0) {
      lines.push('');
      lines.push('I/O Ports:');
      for (const [portName, portDef] of Object.entries(this.ports)) {
        lines.push(`  fd=${portDef.fd} "${portName}" (${portDef.type}) -- ${portDef.description || ''}`);
      }
    }

    lines.push('');
    lines.push(`Halt statuses: ${this.halt.join(', ')}`);

    return lines.join('\n');
  }
}

/**
 * Load a cartridge from a manifest file path.
 */
export async function loadCartridge(manifestPath) {
  const raw = await readFile(manifestPath, 'utf-8');
  const manifest = JSON.parse(raw);
  const cart = new Cartridge(manifest, dirname(manifestPath));
  await cart.resolveSchemas();
  return cart;
}

/**
 * Validate a manifest. Returns {ok: true} or {ok: false, errors: [...]}.
 */
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
      if (!methodDef.summary && !methodDef.args_schema) {
        errors.push(`method "${methodName}": needs at least summary or args_schema`);
      }
    }
  }
  return errors.length ? { ok: false, errors } : { ok: true };
}
