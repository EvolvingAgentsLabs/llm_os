// kernel/dispatch.js
// Parse generated text into a structured opcode record. The token-trie
// guarantees the text matches one of the registered opcode strings, so
// parsing is regex-clean — no JSON-repair, no schema retry.

const CALL_RE = /<\|call\|>([a-zA-Z_][\w-]*)\.([a-zA-Z_][\w-]*)\s*([\s\S]*?)\s*<\|\/call\|>/;
const HALT_RE = /<\|halt\|>(?:status=)?(\w+)/;
const THINK_RE = /<\|think\|>([\s\S]*?)<\|\/think\|>/;

export function parseOpcode(text) {
  const think = text.match(THINK_RE);
  const halt = text.match(HALT_RE);
  if (halt) {
    return {
      type: 'halt',
      status: halt[1],
      think: think ? think[1].trim() : null,
      raw: text,
    };
  }
  const call = text.match(CALL_RE);
  if (call) {
    let args = {};
    const argsStr = call[3].trim();
    if (argsStr.length) {
      try { args = JSON.parse(argsStr); }
      catch { args = { __parse_error: argsStr }; }
    }
    return {
      type: 'call',
      cartridge: call[1],
      method: call[2],
      args,
      think: think ? think[1].trim() : null,
      raw: text,
    };
  }
  return { type: 'unknown', think: think ? think[1].trim() : null, raw: text };
}

// Format a result block for injection back into the prompt after a call.
export function formatResult(payload) {
  const text = typeof payload === 'string' ? payload : JSON.stringify(payload);
  return `<|result|>${text}<|/result|>\n`;
}
