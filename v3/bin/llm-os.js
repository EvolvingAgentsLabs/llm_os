#!/usr/bin/env node
// bin/llm-os.js
// LLM-OS v3 CLI runner.
// Boots the kernel, loads cartridges, executes a task, and optionally
// exports the execution trace as a JSONL fine-tuning dataset.
//
// Usage:
//   OPENROUTER_API_KEY=sk-... node bin/llm-os.js --cart cart/game/tetris --task "Play tetris"
//   OPENROUTER_API_KEY=sk-... node bin/llm-os.js --cart cart/game/tetris --task "Play tetris" --export-dataset traces/run1.jsonl

import { parseArgs } from 'node:util';
import { resolve } from 'node:path';
import { writeFile, mkdir } from 'node:fs/promises';
import { dirname } from 'node:path';
import { OpenRouterBackend } from '../kernel/backend_openrouter.js';
import { loadCartridge } from '../kernel/cartridge.js';
import { Executor } from '../kernel/executor.js';
import { TetrisEngine } from '../cart/game/tetris/engine.js';

// ── Parse CLI args ──────────────────────────────────────────────

const { values: args } = parseArgs({
  options: {
    cart:             { type: 'string', multiple: true, default: [] },
    task:             { type: 'string', default: '' },
    model:            { type: 'string', default: 'google/gemini-3.1-flash-lite' },
    'max-steps':      { type: 'string', default: '100' },
    temperature:      { type: 'string', default: '0.3' },
    verbose:          { type: 'boolean', default: false },
    'export-dataset': { type: 'string', default: '' },
    'export-steps':   { type: 'boolean', default: false },
    'runs':           { type: 'string', default: '1' },
    help:             { type: 'boolean', default: false },
  },
  strict: false,
});

if (args.help || !args.task) {
  console.log(`
LLM-OS v3 — Dataset generation via fast models.

Usage:
  OPENROUTER_API_KEY=sk-... node bin/llm-os.js [options]

Options:
  --cart <path>              Path to cartridge directory (can specify multiple)
  --task <string>            Task description for the LLM to execute
  --model <string>           OpenRouter model ID (default: google/gemini-3.1-flash-lite)
  --max-steps <n>            Max opcodes before forced halt (default: 100)
  --temperature <n>          Sampling temperature (default: 0.3)
  --verbose                  Print full trace details
  --export-dataset <path>    Export execution as JSONL training data
  --export-steps             Export step-level examples (one per opcode)
  --runs <n>                 Number of runs to execute (for dataset generation)
  --help                     Show this help

Examples:
  # Single run
  node bin/llm-os.js --cart cart/game/tetris --task "Play tetris." --verbose

  # Generate dataset: 10 runs, export to JSONL
  node bin/llm-os.js --cart cart/game/tetris \\
    --task "Play tetris. Observe, think, rotate and move pieces, then drop." \\
    --runs 10 --export-dataset dataset/tetris_traces.jsonl

  # Step-level dataset for per-opcode fine-tuning
  node bin/llm-os.js --cart cart/game/tetris \\
    --task "Play tetris." \\
    --runs 5 --export-dataset dataset/tetris_steps.jsonl --export-steps
`);
  process.exit(0);
}

// ── Resolve paths relative to v3/ root ──────────────────────────

const V3_ROOT = resolve(import.meta.dirname, '..');

// ── Boot ────────────────────────────────────────────────────────

async function boot() {
  const numRuns = parseInt(args.runs) || 1;
  const isDatasetMode = !!args['export-dataset'];

  console.log('╔══════════════════════════════════════════════╗');
  console.log('║          LLM-OS v3 — Booting...             ║');
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
  const cartridgePaths = [];
  for (const cartPath of args.cart) {
    cartridgePaths.push(resolve(V3_ROOT, cartPath, 'manifest.json'));
  }

  if (cartridgePaths.length === 0) {
    console.error('[boot] error: no cartridges loaded. Use --cart <path>');
    process.exit(1);
  }

  // 3. Run loop
  const allDataset = [];
  let totalSteps = 0;
  let successCount = 0;

  for (let run = 1; run <= numRuns; run++) {
    if (numRuns > 1) {
      console.log(`\n══ Run ${run}/${numRuns} ═══════════════════════════════`);
    }

    // Load fresh cartridges + engines per run
    const cartridges = [];
    const ioHandlers = {};

    for (const fullPath of cartridgePaths) {
      const cart = await loadCartridge(fullPath);
      cartridges.push(cart);

      if (run === 1) {
        console.log(`[boot] cartridge "${cart.name}" — methods: ${cart.methodNames().join(', ')}`);
      }

      if (cart.name === 'tetris') {
        const engine = new TetrisEngine();
        ioHandlers['tetris'] = (method, cArgs) => engine.dispatch(method, cArgs);
      }

      if (cart.name === 'terminal') {
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
              return { ok: true };
            default:
              return { error: `unknown terminal method: ${method}` };
          }
        };

        ioHandlers['fd:1'] = {
          write: (payload) => {
            if (payload.x !== undefined && payload.y !== undefined) {
              const x = payload.x, y = payload.y;
              const ch = payload.char || payload.text || ' ';
              for (let i = 0; i < ch.length && x + i < 80; i++) {
                if (y >= 0 && y < 24) screen[y][x + i] = ch[i];
              }
            }
          },
        };

        ioHandlers['fd:0'] = {
          read: (len) => ({ key: '', note: 'non-interactive mode' }),
        };
      }
    }

    // Create executor
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

    // Run
    if (!isDatasetMode || numRuns === 1) {
      console.log('── Execution ──────────────────────────────────');
      console.log();
    }

    const result = await executor.run();
    totalSteps += result.steps;
    if (result.status === 'success') successCount++;

    // Print summary
    console.log();
    console.log(`  status: ${result.status} | steps: ${result.steps}`);

    // Export dataset
    if (isDatasetMode) {
      const examples = args['export-steps']
        ? executor.exportStepDataset(6)
        : executor.exportDataset();
      allDataset.push(...examples);
      console.log(`  exported: ${examples.length} training example(s)`);
    }
  }

  // Write dataset file
  if (isDatasetMode && allDataset.length > 0) {
    const outPath = resolve(V3_ROOT, args['export-dataset']);
    await mkdir(dirname(outPath), { recursive: true });
    const jsonl = allDataset.map(ex => JSON.stringify(ex)).join('\n') + '\n';
    await writeFile(outPath, jsonl, 'utf-8');

    console.log();
    console.log('── Dataset ────────────────────────────────────');
    console.log(`  file:     ${outPath}`);
    console.log(`  examples: ${allDataset.length}`);
    console.log(`  runs:     ${numRuns} (${successCount} success)`);
    console.log(`  steps:    ${totalSteps} total`);
    console.log(`  format:   JSONL (OpenAI chat fine-tune format)`);
    console.log();
  }

  // Final summary for multi-run
  if (numRuns > 1 && !isDatasetMode) {
    console.log();
    console.log('── Summary ────────────────────────────────────');
    console.log(`  runs:     ${numRuns}`);
    console.log(`  success:  ${successCount}/${numRuns}`);
    console.log(`  steps:    ${totalSteps} total`);
    console.log();
  }
}

boot().catch(err => {
  console.error(`[fatal] ${err.message}`);
  if (args.verbose) console.error(err.stack);
  process.exit(1);
});
