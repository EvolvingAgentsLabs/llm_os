#!/usr/bin/env node
// bin/llm-os.js
// LLM-OS v2 CLI runner.
// Boots the kernel, loads cartridges, and executes a task.
//
// Usage:
//   OPENROUTER_API_KEY=sk-... node bin/llm-os.js --cart cart/game/tetris --task "Play tetris"
//   OPENROUTER_API_KEY=sk-... node bin/llm-os.js --cart cart/terminal --task "Write hello world"
//   OPENROUTER_API_KEY=sk-... node bin/llm-os.js --task "Play tetris" --model google/gemma-4-31b-it

import { parseArgs } from 'node:util';
import { resolve, join } from 'node:path';
import { OpenRouterBackend } from '../kernel/backend_openrouter.js';
import { loadCartridge } from '../kernel/cartridge.js';
import { Executor } from '../kernel/executor.js';
import { TetrisEngine } from '../cart/game/tetris/engine.js';

// ── Parse CLI args ──────────────────────────────────────────────

const { values: args } = parseArgs({
  options: {
    cart:        { type: 'string', multiple: true, default: [] },
    task:        { type: 'string', default: '' },
    model:       { type: 'string', default: 'google/gemma-4-31b-it' },
    'max-steps': { type: 'string', default: '100' },
    temperature: { type: 'string', default: '0.3' },
    verbose:     { type: 'boolean', default: false },
    help:        { type: 'boolean', default: false },
  },
  strict: false,
});

if (args.help || !args.task) {
  console.log(`
LLM-OS v2 — The LLM is the CPU.

Usage:
  OPENROUTER_API_KEY=sk-... node bin/llm-os.js [options]

Options:
  --cart <path>        Path to cartridge directory (can specify multiple)
  --task <string>      Task description for the LLM to execute
  --model <string>     OpenRouter model ID (default: google/gemma-4-31b-it)
  --max-steps <n>      Max opcodes before forced halt (default: 100)
  --temperature <n>    Sampling temperature (default: 0.3)
  --verbose            Print full trace details
  --help               Show this help

Examples:
  node bin/llm-os.js --cart cart/game/tetris --task "Play tetris. Clear as many lines as possible."
  node bin/llm-os.js --cart cart/terminal --task "Print hello world to the screen."
  node bin/llm-os.js --cart cart/game/tetris --cart cart/terminal --task "Play tetris and show the board."
`);
  process.exit(0);
}

// ── Resolve paths relative to v2/ root ──────────────────────────

const V2_ROOT = resolve(import.meta.dirname, '..');

// ── Boot ────────────────────────────────────────────────────────

async function boot() {
  console.log('╔══════════════════════════════════════════════╗');
  console.log('║          LLM-OS v2 — Booting...             ║');
  console.log('╚══════════════════════════════════════════════╝');
  console.log();

  // 1. Initialize backend
  console.log(`[boot] model: ${args.model}`);
  const backend = new OpenRouterBackend({
    model: args.model,
    temperature: parseFloat(args.temperature),
    maxTokens: 512,
  });

  // 2. Load cartridges
  const cartridges = [];
  const ioHandlers = {};

  for (const cartPath of args.cart) {
    const fullPath = resolve(V2_ROOT, cartPath, 'manifest.json');
    console.log(`[boot] loading cartridge: ${fullPath}`);
    const cart = await loadCartridge(fullPath);
    cartridges.push(cart);
    console.log(`[boot] cartridge "${cart.name}" loaded — methods: ${cart.methodNames().join(', ')}`);

    // Wire up game engines for known cartridges
    if (cart.name === 'tetris') {
      const engine = new TetrisEngine();
      ioHandlers['tetris'] = (method, cArgs) => engine.dispatch(method, cArgs);
      console.log('[boot] tetris engine attached');
    }

    if (cart.name === 'terminal') {
      // Terminal I/O handler — writes to stdout for now
      const screen = Array.from({ length: 24 }, () => Array(80).fill(' '));
      let cursorX = 0, cursorY = 0;

      ioHandlers['terminal'] = (method, cArgs) => {
        switch (method) {
          case 'clear':
            for (let r = 0; r < 24; r++) screen[r].fill(' ');
            cursorX = 0; cursorY = 0;
            return { ok: true };
          case 'cursor':
            cursorX = cArgs.x ?? 0;
            cursorY = cArgs.y ?? 0;
            return { ok: true, x: cursorX, y: cursorY };
          case 'print': {
            const text = cArgs.text || '';
            for (let i = 0; i < text.length && cursorX + i < 80; i++) {
              screen[cursorY][cursorX + i] = text[i];
            }
            cursorX += text.length;
            if (cursorX >= 80) { cursorX = 0; cursorY = Math.min(cursorY + 1, 23); }
            return { ok: true, cursor: { x: cursorX, y: cursorY } };
          }
          case 'bell':
            process.stdout.write('\x07');
            return { ok: true };
          default:
            return { error: `unknown terminal method: ${method}` };
        }
      };

      // Wire fd:1 for <|write|> opcodes to the screen
      ioHandlers['fd:1'] = {
        write: (payload) => {
          if (payload.x !== undefined && payload.y !== undefined) {
            const x = payload.x, y = payload.y;
            const ch = payload.char || payload.text || ' ';
            for (let i = 0; i < ch.length && x + i < 80; i++) {
              if (y >= 0 && y < 24) screen[y][x + i] = ch[i];
            }
          }
          // Dump screen to stdout
          if (args.verbose) {
            console.log('┌' + '─'.repeat(80) + '┐');
            for (const row of screen) {
              console.log('│' + row.join('') + '│');
            }
            console.log('└' + '─'.repeat(80) + '┘');
          }
        },
      };

      // Wire fd:0 for <|read|> opcodes — non-interactive, returns empty
      ioHandlers['fd:0'] = {
        read: (len) => ({ key: '', note: 'non-interactive mode, no keyboard input' }),
      };

      console.log('[boot] terminal I/O attached (80x24 screen)');
    }
  }

  if (cartridges.length === 0) {
    console.error('[boot] error: no cartridges loaded. Use --cart <path>');
    process.exit(1);
  }

  // 3. Create executor
  console.log(`[boot] task: "${args.task}"`);
  console.log(`[boot] max steps: ${args['max-steps']}`);
  console.log();

  const executor = new Executor({
    backend,
    cartridges,
    task: args.task,
    maxSteps: parseInt(args['max-steps']),
    onOpcode: (op, step) => {
      const prefix = `[step ${String(step).padStart(3)}]`;
      if (op.type === 'call') {
        console.log(`${prefix} <|call|>${op.cartridge}.${op.method} ${JSON.stringify(op.args)}<|/call|>`);
      } else if (op.type === 'halt') {
        console.log(`${prefix} <|halt|>status=${op.status}`);
      } else if (op.type === 'loop') {
        console.log(`${prefix} <|loop|>goal=${op.goal}`);
      } else if (op.type === 'break') {
        console.log(`${prefix} <|break|>`);
      } else if (op.type === 'commit') {
        console.log(`${prefix} <|commit|>${JSON.stringify(op.data)}`);
      } else if (op.type === 'read') {
        console.log(`${prefix} <|read|>fd=${op.fd} len=${op.len}`);
      } else if (op.type === 'write') {
        console.log(`${prefix} <|write|>fd=${op.fd} ${JSON.stringify(op.payload)}`);
      } else {
        console.log(`${prefix} <|${op.type}|>`);
      }
    },
    onResult: (result, step) => {
      if (args.verbose) {
        const json = JSON.stringify(result);
        const truncated = json.length > 200 ? json.slice(0, 200) + '...' : json;
        console.log(`         → ${truncated}`);
      }
    },
    onThink: (text, step) => {
      console.log(`  [think] ${text}`);
    },
    ioHandlers,
  });

  // 4. Run
  console.log('── Execution ──────────────────────────────────');
  console.log();

  const result = await executor.run();

  // 5. Summary
  console.log();
  console.log('── Result ─────────────────────────────────────');
  console.log(`  status: ${result.status}`);
  console.log(`  steps:  ${result.steps}`);
  if (Object.keys(result.state).length > 0) {
    console.log(`  state:  ${JSON.stringify(result.state)}`);
  }

  // Print trace summary
  const typeCounts = {};
  for (const t of result.trace) {
    typeCounts[t.type] = (typeCounts[t.type] || 0) + 1;
  }
  console.log(`  trace:  ${Object.entries(typeCounts).map(([k,v]) => `${k}:${v}`).join(' ')}`);
  console.log();
}

boot().catch(err => {
  console.error(`[fatal] ${err.message}`);
  if (args.verbose) console.error(err.stack);
  process.exit(1);
});
