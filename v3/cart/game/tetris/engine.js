// cart/game/tetris/engine.js
// Tetris game engine for LLM-OS v2.
// Ported from v1's demo/tetris-browser/index.html.
// Returns RAW state — no compileResult, no bestActions, no Program layer.
// The LLM is the planner now.

const COLS = 10, ROWS = 20;
const PIECE_TYPES = ['I', 'O', 'T', 'S', 'Z', 'J', 'L'];

const OFFSETS = {
  I: [[[0,0],[1,0],[-1,0],[2,0]],[[0,0],[0,-1],[0,1],[0,2]],[[0,0],[-1,0],[1,0],[-2,0]],[[0,0],[0,1],[0,-1],[0,-2]]],
  O: [[[0,0],[1,0],[0,1],[1,1]],[[0,0],[1,0],[0,1],[1,1]],[[0,0],[1,0],[0,1],[1,1]],[[0,0],[1,0],[0,1],[1,1]]],
  T: [[[0,0],[-1,0],[1,0],[0,-1]],[[0,0],[0,-1],[0,1],[1,0]],[[0,0],[-1,0],[1,0],[0,1]],[[0,0],[0,-1],[0,1],[-1,0]]],
  S: [[[0,0],[-1,0],[0,-1],[1,-1]],[[0,0],[0,-1],[1,0],[1,1]],[[0,0],[1,0],[0,1],[-1,1]],[[0,0],[0,1],[-1,0],[-1,-1]]],
  Z: [[[0,0],[1,0],[0,-1],[-1,-1]],[[0,0],[0,1],[1,0],[1,-1]],[[0,0],[-1,0],[0,1],[1,1]],[[0,0],[0,-1],[-1,0],[-1,1]]],
  J: [[[0,0],[-1,0],[1,0],[-1,-1]],[[0,0],[0,-1],[0,1],[1,-1]],[[0,0],[-1,0],[1,0],[1,1]],[[0,0],[0,-1],[0,1],[-1,1]]],
  L: [[[0,0],[-1,0],[1,0],[1,-1]],[[0,0],[0,-1],[0,1],[1,1]],[[0,0],[-1,0],[1,0],[-1,1]],[[0,0],[0,-1],[0,1],[-1,-1]]],
};

export class TetrisEngine {
  constructor() { this.reset(); }

  reset() {
    this.board = Array.from({ length: ROWS }, () => new Uint8Array(COLS));
    this.rng = 42;
    this.bag = [];
    this._fillBag();
    const first = this.bag.pop();
    const second = this.bag.pop();
    this.current = { type: first, x: 4, y: 0, rot: 0 };
    this.next = second;
    this.score = 0;
    this.lines = 0;
    this.gameOver = false;
    this.stepCount = 0;
  }

  _xorshift() {
    let x = this.rng >>> 0;
    x ^= (x << 13) >>> 0;
    x ^= (x >>> 7);
    x ^= (x << 17) >>> 0;
    this.rng = x >>> 0;
    return x >>> 0;
  }

  _fillBag() {
    const p = [...PIECE_TYPES];
    for (let i = p.length - 1; i > 0; i--) {
      const j = this._xorshift() % (i + 1);
      [p[i], p[j]] = [p[j], p[i]];
    }
    for (let i = p.length - 1; i >= 0; i--) this.bag.push(p[i]);
  }

  _offsets(type, rot) { return OFFSETS[type][rot % 4]; }

  _collides(type, rot, x, y) {
    for (const [dx, dy] of this._offsets(type, rot)) {
      const c = x + dx, r = y + dy;
      if (c < 0 || c >= COLS || r >= ROWS) return true;
      if (r < 0) continue;
      if (this.board[r][c] !== 0) return true;
    }
    return false;
  }

  _tryShift(dx, dy) {
    const nx = this.current.x + dx, ny = this.current.y + dy;
    if (!this._collides(this.current.type, this.current.rot, nx, ny)) {
      this.current.x = nx; this.current.y = ny; return true;
    }
    return false;
  }

  _tryRotate() {
    const nr = (this.current.rot + 1) % 4;
    if (!this._collides(this.current.type, nr, this.current.x, this.current.y)) {
      this.current.rot = nr; return true;
    }
    return false;
  }

  _lock() {
    const ch = this.current.type.charCodeAt(0);
    for (const [dx, dy] of this._offsets(this.current.type, this.current.rot)) {
      const c = this.current.x + dx, r = this.current.y + dy;
      if (r >= 0 && r < ROWS && c >= 0 && c < COLS) this.board[r][c] = ch;
    }
  }

  _clearLines() {
    let cleared = 0;
    const temp = Array.from({ length: ROWS }, () => new Uint8Array(COLS));
    let w = ROWS - 1;
    for (let r = ROWS - 1; r >= 0; r--) {
      if (this.board[r].every(c => c !== 0)) { cleared++; }
      else { temp[w] = this.board[r]; if (w > 0) w--; }
    }
    this.board = temp;
    if (cleared > 0) {
      const pts = [0, 100, 300, 500, 800][Math.min(cleared, 4)] || cleared * 200;
      this.score += pts * this.level;
      this.lines += cleared;
    }
    return cleared;
  }

  _spawn() {
    if (this.bag.length === 0) this._fillBag();
    const nn = this.bag.pop();
    this.current = { type: this.next, x: 4, y: 0, rot: 0 };
    this.next = nn;
    if (this._collides(this.current.type, this.current.rot, this.current.x, this.current.y)) {
      this.gameOver = true; return false;
    }
    return true;
  }

  get level() { return Math.floor(this.lines / 10) + 1; }

  /**
   * Dispatch a method call. Returns raw game state.
   * This is the cartridge I/O handler for the executor.
   */
  dispatch(method, args) {
    if (method === 'reset') { this.reset(); return this.state; }
    if (method === 'observe') { return this.state; }
    if (method === 'move') {
      if (this.gameOver) return this.state;
      const action = args.action;
      if (action === 'left') this._tryShift(-1, 0);
      else if (action === 'right') this._tryShift(1, 0);
      else if (action === 'down') {
        if (!this._tryShift(0, 1)) { this._lock(); this._clearLines(); this._spawn(); }
      }
      else if (action === 'rotate') this._tryRotate();
      else if (action === 'drop') {
        while (this._tryShift(0, 1)) {}
        this._lock(); this._clearLines(); this._spawn();
      }
      // Gravity: piece falls after every move
      if (!this.gameOver && action !== 'down' && action !== 'drop') {
        if (!this._tryShift(0, 1)) {
          this._lock(); this._clearLines(); this._spawn();
        }
      }
      this.stepCount++;
      return this.state;
    }
    return { error: `unknown method: ${method}` };
  }

  /** Raw game state — no compilation, no hints, no best_actions. */
  get state() {
    const boardStrs = this.board.map(row =>
      Array.from(row).map(c => c === 0 ? '.' : String.fromCharCode(c)).join('')
    );
    return {
      board: boardStrs,
      current: {
        type: this.current.type,
        x: this.current.x,
        y: this.current.y,
        rotation: this.current.rot,
      },
      next: this.next,
      score: this.score,
      lines: this.lines,
      level: this.level,
      game_over: this.gameOver,
      step: this.stepCount,
    };
  }
}
