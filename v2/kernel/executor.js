// kernel/executor.js
// The LLM-OS v2 execution loop. This is the operating system.
//
// Unlike v1 (single-step: emit 1 opcode, dispatch, repeat from the demo),
// v2 runs a full program: the LLM emits opcodes, the executor parses and
// dispatches them, manages loop depth, persistent state (commit), I/O
// channels (read/write), and halts when the program ends.
//
// The LLM is the CPU. The executor is the OS. Cartridges are the hardware.

import { parseOpcode, formatResult, formatAck } from './dispatch.js';

/**
 * @typedef {object} ExecutorOpts
 * @property {import('./backend_openrouter.js').OpenRouterBackend} backend
 * @property {import('./cartridge.js').Cartridge[]} cartridges
 * @property {string} task - The user's task description
 * @property {number} [maxSteps=200] - Safety limit on total opcodes
 * @property {number} [maxLoopIterations=50] - Max iterations per loop
 * @property {function} [onOpcode] - Callback for each opcode: (op, step) => void
 * @property {function} [onResult] - Callback for each result: (result, step) => void
 * @property {function} [onThink] - Callback for think blocks: (text, step) => void
 * @property {object} [ioHandlers] - Map of fd -> {read, write} handlers
 */

export class Executor {
  /**
   * @param {ExecutorOpts} opts
   */
  constructor(opts) {
    this.backend = opts.backend;
    this.cartridges = new Map();
    for (const cart of opts.cartridges) {
      this.cartridges.set(cart.name, cart);
    }
    this.task = opts.task;
    this.maxSteps = opts.maxSteps ?? 200;
    this.maxLoopIterations = opts.maxLoopIterations ?? 50;
    this.onOpcode = opts.onOpcode ?? null;
    this.onResult = opts.onResult ?? null;
    this.onThink = opts.onThink ?? null;
    this.ioHandlers = opts.ioHandlers ?? {};

    // Execution state
    this.state = new Map();  // persistent state (commit store)
    this.messages = [];      // conversation messages
    this.stepCount = 0;
    this.loopStack = [];     // [{goal, startMsgIdx, iterations}]
    this.halted = false;
    this.haltStatus = null;
    this.trace = [];         // full execution trace
  }

  /**
   * Build the system prompt from loaded cartridges.
   */
  buildSystemPrompt() {
    const cartDescriptions = [...this.cartridges.values()]
      .map(c => c.buildISADescription())
      .join('\n\n');

    return `You are the LLM-OS kernel. You are a CPU that executes tasks by emitting ISA opcodes.

You emit ONE opcode per turn. After each opcode you receive a result or ack. Then emit the next opcode.

CRITICAL: All JSON arguments MUST use double-quoted keys. Write {"action":"drop"} NOT {action: "drop"}.

## ISA opcodes

  <|call|>cartridge.method {"key":"value"}<|/call|>   -- cartridge syscall (result follows)
  <|read|>fd=N len=N                                   -- read from I/O port (result follows)
  <|write|>fd=N {"key":"value"}                        -- write to I/O port (ack follows)
  <|wait|>fd=N                                         -- block on fd (result follows)
  <|loop|>goal=label                                   -- enter loop
  <|break|>                                            -- exit loop
  <|halt|>status=success|failure|partial                -- end program (depth 0 only)
  <|think|>reasoning<|/think|>                         -- inner monologue
  <|commit|>{"key":"value"}                            -- persist state
  <|fork|>goal=label                                   -- spawn subtask
  <|yield|>                                            -- yield CPU
  <|fault|>{"error":"msg"}                             -- raise exception (result follows)
  <|policy|>                                           -- query capabilities (result follows)

## Rules
1. Emit exactly ONE opcode per turn. Nothing else — no prose, no markdown, just the opcode.
2. <|halt|> only at loop depth 0.
3. Every <|loop|> needs a <|break|>.
4. JSON args must use double-quoted keys: {"action":"drop"} not {action: "drop"}.

## Available cartridges

${cartDescriptions}

## Committed state
${this.state.size > 0 ? JSON.stringify(Object.fromEntries(this.state)) : '(empty)'}`;
  }

  /**
   * Run the program to completion.
   * Returns {status, steps, trace, state}.
   */
  async run() {
    // Build initial messages
    this.messages = [
      { role: 'system', content: this.buildSystemPrompt() },
      { role: 'user', content: this.task },
    ];

    while (!this.halted && this.stepCount < this.maxSteps) {
      await this.step();
    }

    if (!this.halted) {
      this.haltStatus = 'max_steps_reached';
    }

    return {
      status: this.haltStatus,
      steps: this.stepCount,
      trace: this.trace,
      state: Object.fromEntries(this.state),
    };
  }

  /**
   * Execute one step: generate an opcode, parse it, dispatch it.
   */
  async step() {
    // Generate the next opcode
    const stopSequences = [
      '<|result|>', '<|/result|>', '<|ack|>',
      '\n<|call|>', '\n<|read|>', '\n<|write|>', '\n<|halt|>',
      '\n<|loop|>', '\n<|break|>', '\n<|fork|>',
      '\n<|yield|>', '\n<|commit|>', '\n<|fault|>', '\n<|policy|>',
    ];

    const response = await this.backend.generate(this.messages, {
      maxTokens: 300,
      stop: stopSequences,
      temperature: 0.3,
    });

    const rawText = response.content.trim();
    if (!rawText) {
      this.trace.push({ step: this.stepCount, type: 'error', detail: 'empty response' });
      this.halted = true;
      this.haltStatus = 'empty_response';
      return;
    }

    // Parse the opcode
    const op = parseOpcode(rawText);
    this.stepCount++;

    // Record trace
    this.trace.push({ step: this.stepCount, type: op.type, raw: rawText, parsed: op });

    // Callbacks
    if (op.think && this.onThink) this.onThink(op.think, this.stepCount);
    if (this.onOpcode) this.onOpcode(op, this.stepCount);

    // Dispatch based on opcode type
    switch (op.type) {
      case 'call':
        await this._handleCall(op);
        break;

      case 'halt':
        this._handleHalt(op);
        break;

      case 'read':
        await this._handleRead(op);
        break;

      case 'write':
        await this._handleWrite(op);
        break;

      case 'loop':
        this._handleLoop(op);
        break;

      case 'break':
        this._handleBreak(op);
        break;

      case 'commit':
        this._handleCommit(op);
        break;

      case 'think':
        // Think-only turn: append as assistant message, continue
        this.messages.push({ role: 'assistant', content: rawText });
        break;

      case 'fork':
        this._handleFork(op);
        break;

      case 'yield':
        // Yield: no-op in single-process mode, just continue
        this.messages.push({ role: 'assistant', content: rawText });
        break;

      case 'wait':
        await this._handleWait(op);
        break;

      case 'fault':
        await this._handleFault(op);
        break;

      case 'policy':
        await this._handlePolicy(op);
        break;

      default:
        // Unknown opcode — inject error as result and continue
        this.messages.push({ role: 'assistant', content: rawText });
        this.messages.push({
          role: 'user',
          content: formatResult({ error: 'unknown opcode', raw: rawText }),
        });
        break;
    }
  }

  // ── Opcode handlers ─────────────────────────────────────────────

  async _handleCall(op) {
    const cart = this.cartridges.get(op.cartridge);
    if (!cart) {
      const err = { error: `unknown cartridge: ${op.cartridge}` };
      this.messages.push({ role: 'assistant', content: op.raw });
      this.messages.push({ role: 'user', content: formatResult(err) });
      if (this.onResult) this.onResult(err, this.stepCount);
      return;
    }

    if (!cart.methods[op.method]) {
      const err = { error: `unknown method: ${op.cartridge}.${op.method}` };
      this.messages.push({ role: 'assistant', content: op.raw });
      this.messages.push({ role: 'user', content: formatResult(err) });
      if (this.onResult) this.onResult(err, this.stepCount);
      return;
    }

    // Dispatch to the cartridge engine
    let result;
    try {
      const handler = this.ioHandlers[`${op.cartridge}.${op.method}`]
                   || this.ioHandlers[op.cartridge];
      if (handler && typeof handler === 'object' && typeof handler.dispatch === 'function') {
        result = await handler.dispatch(op.method, op.args);
      } else if (typeof handler === 'function') {
        result = await handler(op.method, op.args);
      } else {
        result = { error: `no handler for ${op.cartridge}.${op.method}` };
      }
    } catch (err) {
      result = { error: err.message };
    }

    const resultBlock = formatResult(result);
    this.messages.push({ role: 'assistant', content: op.raw });
    this.messages.push({ role: 'user', content: resultBlock });
    if (this.onResult) this.onResult(result, this.stepCount);
  }

  _handleHalt(op) {
    if (this.loopStack.length > 0) {
      // Can't halt inside a loop — inject error and continue
      this.messages.push({ role: 'assistant', content: op.raw });
      this.messages.push({
        role: 'user',
        content: formatResult({ error: 'halt inside loop is illegal, use <|break|> first' }),
      });
      return;
    }
    this.halted = true;
    this.haltStatus = op.status;
    this.messages.push({ role: 'assistant', content: op.raw });
  }

  async _handleRead(op) {
    const handler = this.ioHandlers[`fd:${op.fd}`];
    let data;
    if (handler && typeof handler.read === 'function') {
      data = await handler.read(op.len);
    } else {
      data = { error: `no reader on fd=${op.fd}` };
    }
    const resultBlock = formatResult(data);
    this.messages.push({ role: 'assistant', content: op.raw });
    this.messages.push({ role: 'user', content: resultBlock });
    if (this.onResult) this.onResult(data, this.stepCount);
  }

  async _handleWrite(op) {
    const handler = this.ioHandlers[`fd:${op.fd}`];
    if (handler && typeof handler.write === 'function') {
      await handler.write(op.payload);
    }
    this.messages.push({ role: 'assistant', content: op.raw });
    this.messages.push({ role: 'user', content: formatAck() });
  }

  _handleLoop(op) {
    this.loopStack.push({
      goal: op.goal,
      startMsgIdx: this.messages.length,
      iterations: 0,
    });
    this.messages.push({ role: 'assistant', content: op.raw });
  }

  _handleBreak(op) {
    if (this.loopStack.length === 0) {
      // Break outside loop — inject error
      this.messages.push({ role: 'assistant', content: op.raw });
      this.messages.push({
        role: 'user',
        content: formatResult({ error: 'break outside loop is illegal' }),
      });
      return;
    }
    this.loopStack.pop();
    this.messages.push({ role: 'assistant', content: op.raw });
  }

  _handleCommit(op) {
    // Merge committed data into persistent state
    if (op.data && typeof op.data === 'object' && !op.data.__raw) {
      for (const [k, v] of Object.entries(op.data)) {
        this.state.set(k, v);
      }
    }
    this.messages.push({ role: 'assistant', content: op.raw });

    // Update system prompt with new state (rebuild it)
    this.messages[0] = { role: 'system', content: this.buildSystemPrompt() };
  }

  _handleFork(op) {
    // In single-process mode: log the fork as a trace entry.
    // Full multi-process support is a future extension.
    this.trace.push({
      step: this.stepCount,
      type: 'fork',
      goal: op.goal,
      note: 'single-process mode: fork logged but not spawned',
    });
    this.messages.push({ role: 'assistant', content: op.raw });
  }

  async _handleWait(op) {
    const handler = this.ioHandlers[`fd:${op.fd}`];
    let data;
    if (handler && typeof handler.wait === 'function') {
      data = await handler.wait();
    } else {
      data = { error: `no waiter on fd=${op.fd}` };
    }
    const resultBlock = formatResult(data);
    this.messages.push({ role: 'assistant', content: op.raw });
    this.messages.push({ role: 'user', content: resultBlock });
    if (this.onResult) this.onResult(data, this.stepCount);
  }

  async _handleFault(op) {
    // Fault: raise to error handler or return default recovery
    const result = { recovery: 'continue', fault: op.data };
    const resultBlock = formatResult(result);
    this.messages.push({ role: 'assistant', content: op.raw });
    this.messages.push({ role: 'user', content: resultBlock });
    if (this.onResult) this.onResult(result, this.stepCount);
  }

  async _handlePolicy(op) {
    // Return current capabilities: all loaded cartridge methods
    const capabilities = [];
    for (const [name, cart] of this.cartridges) {
      for (const method of Object.keys(cart.methods)) {
        capabilities.push(`call.${name}.${method}`);
      }
    }
    capabilities.push('read', 'write', 'loop', 'break', 'halt', 'commit', 'think', 'fork', 'yield', 'fault', 'policy');

    const result = { allowed: capabilities };
    const resultBlock = formatResult(result);
    this.messages.push({ role: 'assistant', content: op.raw });
    this.messages.push({ role: 'user', content: resultBlock });
    if (this.onResult) this.onResult(result, this.stepCount);
  }

  /**
   * Compact conversation history to stay within context limits.
   * Keeps system prompt, first user message, last N exchanges.
   */
  compact(keepLast = 20) {
    if (this.messages.length <= keepLast + 2) return;

    const system = this.messages[0];
    const task = this.messages[1];
    const recent = this.messages.slice(-keepLast);

    // Rebuild system prompt with current committed state
    system.content = this.buildSystemPrompt();

    this.messages = [system, task, ...recent];
  }
}
