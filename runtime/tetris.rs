//! Turn-based Tetris game engine for the `game/tetris` cartridge.
//!
//! The LLM is the player: it observes the board, picks an action, and the
//! engine advances one step. No real-time gravity — every change is driven
//! by an explicit `move` call.

use crate::dispatch::{DispatchError, DispatchResult};
use serde_json::{json, Value};
use std::sync::Mutex;

// ── Piece definitions ────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum PieceType {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

const ALL_PIECES: [PieceType; 7] = [
    PieceType::I,
    PieceType::O,
    PieceType::T,
    PieceType::S,
    PieceType::Z,
    PieceType::J,
    PieceType::L,
];

impl PieceType {
    fn letter(self) -> char {
        match self {
            PieceType::I => 'I',
            PieceType::O => 'O',
            PieceType::T => 'T',
            PieceType::S => 'S',
            PieceType::Z => 'Z',
            PieceType::J => 'J',
            PieceType::L => 'L',
        }
    }

    /// Return the 4 cell offsets (col, row) for a given rotation (0-3).
    /// Origin is the pivot cell at (0,0). SRS-style offsets, no wall kicks.
    fn offsets(self, rot: u8) -> [(i32, i32); 4] {
        let r = (rot % 4) as usize;
        match self {
            PieceType::I => [
                [( 0, 0), ( 1, 0), (-1, 0), ( 2, 0)],  // 0: horizontal
                [( 0, 0), ( 0,-1), ( 0, 1), ( 0, 2)],  // 1: vertical
                [( 0, 0), (-1, 0), ( 1, 0), (-2, 0)],  // 2: horizontal flipped
                [( 0, 0), ( 0, 1), ( 0,-1), ( 0,-2)],  // 3: vertical flipped
            ][r],
            PieceType::O => [
                [(0, 0), (1, 0), (0, 1), (1, 1)]; 4
            ][r],
            PieceType::T => [
                [( 0, 0), (-1, 0), ( 1, 0), ( 0,-1)],  // 0: T up
                [( 0, 0), ( 0,-1), ( 0, 1), ( 1, 0)],  // 1: T right
                [( 0, 0), (-1, 0), ( 1, 0), ( 0, 1)],  // 2: T down
                [( 0, 0), ( 0,-1), ( 0, 1), (-1, 0)],  // 3: T left
            ][r],
            PieceType::S => [
                [( 0, 0), (-1, 0), ( 0,-1), ( 1,-1)],  // 0
                [( 0, 0), ( 0,-1), ( 1, 0), ( 1, 1)],  // 1
                [( 0, 0), ( 1, 0), ( 0, 1), (-1, 1)],  // 2
                [( 0, 0), ( 0, 1), (-1, 0), (-1,-1)],  // 3
            ][r],
            PieceType::Z => [
                [( 0, 0), ( 1, 0), ( 0,-1), (-1,-1)],  // 0
                [( 0, 0), ( 0, 1), ( 1, 0), ( 1,-1)],  // 1
                [( 0, 0), (-1, 0), ( 0, 1), ( 1, 1)],  // 2
                [( 0, 0), ( 0,-1), (-1, 0), (-1, 1)],  // 3
            ][r],
            PieceType::J => [
                [( 0, 0), (-1, 0), ( 1, 0), (-1,-1)],  // 0
                [( 0, 0), ( 0,-1), ( 0, 1), ( 1,-1)],  // 1
                [( 0, 0), (-1, 0), ( 1, 0), ( 1, 1)],  // 2
                [( 0, 0), ( 0,-1), ( 0, 1), (-1, 1)],  // 3
            ][r],
            PieceType::L => [
                [( 0, 0), (-1, 0), ( 1, 0), ( 1,-1)],  // 0
                [( 0, 0), ( 0,-1), ( 0, 1), ( 1, 1)],  // 1
                [( 0, 0), (-1, 0), ( 1, 0), (-1, 1)],  // 2
                [( 0, 0), ( 0,-1), ( 0, 1), (-1,-1)],  // 3
            ][r],
        }
    }
}

// ── Game state ───────────────────────────────────────────────────────

const COLS: usize = 10;
const ROWS: usize = 20;

#[derive(Clone, Debug)]
struct FallingPiece {
    kind: PieceType,
    x: i32,
    y: i32,
    rot: u8,
}

#[derive(Clone, Debug)]
struct TetrisGame {
    board: [[u8; COLS]; ROWS],
    current: FallingPiece,
    next: PieceType,
    bag: Vec<PieceType>,
    score: u64,
    lines: u64,
    game_over: bool,
    rng_state: u64,
}

static STATE: Mutex<Option<TetrisGame>> = Mutex::new(None);

// ── RNG (xorshift64) ────────────────────────────────────────────────

fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

// ── Core game logic ──────────────────────────────────────────────────

fn new_game(seed: u64) -> TetrisGame {
    let mut rng = seed;
    let mut bag = Vec::with_capacity(14);
    fill_bag(&mut bag, &mut rng);
    let current_kind = bag.pop().unwrap();
    let next_kind = bag.pop().unwrap();
    TetrisGame {
        board: [[0; COLS]; ROWS],
        current: FallingPiece {
            kind: current_kind,
            x: 4,
            y: 0,
            rot: 0,
        },
        next: next_kind,
        bag,
        score: 0,
        lines: 0,
        game_over: false,
        rng_state: rng,
    }
}

fn fill_bag(bag: &mut Vec<PieceType>, rng: &mut u64) {
    let mut pieces = ALL_PIECES;
    // Fisher-Yates shuffle
    for i in (1..pieces.len()).rev() {
        let j = (xorshift64(rng) as usize) % (i + 1);
        pieces.swap(i, j);
    }
    // Push in reverse so pop() yields the first shuffled element
    for p in pieces.iter().rev() {
        bag.push(*p);
    }
}

fn collides(board: &[[u8; COLS]; ROWS], kind: PieceType, rot: u8, x: i32, y: i32) -> bool {
    for (dx, dy) in kind.offsets(rot) {
        let col = x + dx;
        let row = y + dy;
        if col < 0 || col >= COLS as i32 || row >= ROWS as i32 {
            return true;
        }
        // Cells above the board are allowed (pieces spawn at the top edge)
        if row < 0 {
            continue;
        }
        if board[row as usize][col as usize] != 0 {
            return true;
        }
    }
    false
}

fn try_shift(game: &mut TetrisGame, dx: i32, dy: i32) -> bool {
    let nx = game.current.x + dx;
    let ny = game.current.y + dy;
    if !collides(&game.board, game.current.kind, game.current.rot, nx, ny) {
        game.current.x = nx;
        game.current.y = ny;
        true
    } else {
        false
    }
}

fn try_rotate(game: &mut TetrisGame) -> bool {
    let new_rot = (game.current.rot + 1) % 4;
    if !collides(
        &game.board,
        game.current.kind,
        new_rot,
        game.current.x,
        game.current.y,
    ) {
        game.current.rot = new_rot;
        true
    } else {
        false
    }
}

fn lock_piece(game: &mut TetrisGame) {
    let letter = game.current.kind.letter() as u8;
    for (dx, dy) in game.current.kind.offsets(game.current.rot) {
        let col = (game.current.x + dx) as usize;
        let row = (game.current.y + dy) as usize;
        if row < ROWS && col < COLS {
            game.board[row][col] = letter;
        }
    }
}

fn clear_lines(game: &mut TetrisGame) -> u64 {
    let mut cleared = 0u64;
    let mut write = ROWS - 1;
    // Scan from bottom to top, compact non-full rows downward
    let mut temp = [[0u8; COLS]; ROWS];
    for r in (0..ROWS).rev() {
        let full = game.board[r].iter().all(|&c| c != 0);
        if !full {
            temp[write] = game.board[r];
            if write > 0 {
                write -= 1;
            }
        } else {
            cleared += 1;
        }
    }
    game.board = temp;

    if cleared > 0 {
        let level = game.level();
        let points = match cleared {
            1 => 100,
            2 => 300,
            3 => 500,
            4 => 800,
            n => n * 200, // shouldn't happen, but safe
        };
        game.score += points * level;
        game.lines += cleared;
    }
    cleared
}

fn spawn(game: &mut TetrisGame) -> bool {
    if game.bag.is_empty() {
        fill_bag(&mut game.bag, &mut game.rng_state);
    }
    let next_next = game.bag.pop().unwrap();
    game.current = FallingPiece {
        kind: game.next,
        x: 4,
        y: 0,
        rot: 0,
    };
    game.next = next_next;

    // Check if the new piece collides immediately → game over
    if collides(
        &game.board,
        game.current.kind,
        game.current.rot,
        game.current.x,
        game.current.y,
    ) {
        game.game_over = true;
        return false;
    }
    true
}

impl TetrisGame {
    fn level(&self) -> u64 {
        self.lines / 10 + 1
    }
}

// ── Board serialization ──────────────────────────────────────────────

fn board_to_strings(board: &[[u8; COLS]; ROWS]) -> Vec<String> {
    board
        .iter()
        .map(|row| {
            row.iter()
                .map(|&c| if c == 0 { '.' } else { c as char })
                .collect()
        })
        .collect()
}

fn state_to_json(game: &TetrisGame) -> Value {
    json!({
        "board": board_to_strings(&game.board),
        "current": {
            "type": game.current.kind.letter().to_string(),
            "x": game.current.x,
            "y": game.current.y,
            "rotation": game.current.rot,
        },
        "next": game.next.letter().to_string(),
        "score": game.score,
        "lines": game.lines,
        "level": game.level(),
        "game_over": game.game_over,
    })
}

// ── Dispatch entry point ─────────────────────────────────────────────

pub fn dispatch(method: &str, args: &Value) -> DispatchResult {
    match method {
        "move" => do_move(args),
        "observe" => do_observe(),
        "reset" => do_reset(),
        other => Err(DispatchError::HandlerError {
            cart: "tetris".into(),
            method: other.into(),
            message: "unknown method".into(),
        }),
    }
}

fn ensure_game() -> Result<(), DispatchError> {
    let mut lock = STATE.lock().unwrap();
    if lock.is_none() {
        *lock = Some(new_game(42));
    }
    Ok(())
}

fn do_move(args: &Value) -> DispatchResult {
    ensure_game()?;
    let mut lock = STATE.lock().unwrap();
    let game = lock.as_mut().unwrap();

    if game.game_over {
        return Ok(state_to_json(game));
    }

    let action = args
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| DispatchError::HandlerError {
            cart: "tetris".into(),
            method: "move".into(),
            message: "args.action must be a string".into(),
        })?;

    match action {
        "left" => {
            try_shift(game, -1, 0);
        }
        "right" => {
            try_shift(game, 1, 0);
        }
        "down" => {
            if !try_shift(game, 0, 1) {
                lock_piece(game);
                clear_lines(game);
                spawn(game);
            }
        }
        "rotate" => {
            try_rotate(game);
        }
        "drop" => {
            // Hard drop: move down until collision, then lock
            while try_shift(game, 0, 1) {}
            lock_piece(game);
            clear_lines(game);
            spawn(game);
        }
        other => {
            return Err(DispatchError::HandlerError {
                cart: "tetris".into(),
                method: "move".into(),
                message: format!("unknown action '{other}'"),
            });
        }
    }

    Ok(state_to_json(game))
}

fn do_observe() -> DispatchResult {
    ensure_game()?;
    let lock = STATE.lock().unwrap();
    let game = lock.as_ref().unwrap();
    Ok(state_to_json(game))
}

fn do_reset() -> DispatchResult {
    let mut lock = STATE.lock().unwrap();
    *lock = Some(new_game(42));
    let game = lock.as_ref().unwrap();
    Ok(state_to_json(game))
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> TetrisGame {
        new_game(42)
    }

    #[test]
    fn spawn_places_piece_at_top_center() {
        let game = fresh();
        assert_eq!(game.current.x, 4);
        assert_eq!(game.current.y, 0);
        assert!(!game.game_over);
    }

    #[test]
    fn move_left_shifts_piece() {
        let mut game = fresh();
        let old_x = game.current.x;
        try_shift(&mut game, -1, 0);
        assert_eq!(game.current.x, old_x - 1);
    }

    #[test]
    fn move_right_shifts_piece() {
        let mut game = fresh();
        let old_x = game.current.x;
        try_shift(&mut game, 1, 0);
        assert_eq!(game.current.x, old_x + 1);
    }

    #[test]
    fn move_down_shifts_piece() {
        let mut game = fresh();
        let old_y = game.current.y;
        try_shift(&mut game, 0, 1);
        assert_eq!(game.current.y, old_y + 1);
    }

    #[test]
    fn wall_collision_prevents_movement() {
        let mut game = fresh();
        // Push piece all the way left until it can't go further
        for _ in 0..20 {
            try_shift(&mut game, -1, 0);
        }
        let x_at_wall = game.current.x;
        let moved = try_shift(&mut game, -1, 0);
        assert!(!moved);
        assert_eq!(game.current.x, x_at_wall);
    }

    #[test]
    fn floor_collision_prevents_movement() {
        let mut game = fresh();
        // Push piece all the way down until it can't go further
        for _ in 0..25 {
            try_shift(&mut game, 0, 1);
        }
        let y_at_floor = game.current.y;
        let moved = try_shift(&mut game, 0, 1);
        assert!(!moved);
        assert_eq!(game.current.y, y_at_floor);
    }

    #[test]
    fn rotation_cycles_through_4_states() {
        let mut game = fresh();
        let r0 = game.current.rot;
        try_rotate(&mut game);
        assert_eq!(game.current.rot, (r0 + 1) % 4);
        try_rotate(&mut game);
        assert_eq!(game.current.rot, (r0 + 2) % 4);
        try_rotate(&mut game);
        assert_eq!(game.current.rot, (r0 + 3) % 4);
        try_rotate(&mut game);
        assert_eq!(game.current.rot, r0);
    }

    #[test]
    fn line_clearing_works() {
        let mut game = fresh();
        // Fill bottom row completely
        for c in 0..COLS {
            game.board[ROWS - 1][c] = b'X';
        }
        let cleared = clear_lines(&mut game);
        assert_eq!(cleared, 1);
        assert_eq!(game.score, 100); // 100 * level 1
        // Bottom row should now be empty (compacted)
        assert!(game.board[ROWS - 1].iter().all(|&c| c == 0));
    }

    #[test]
    fn tetris_scores_800() {
        let mut game = fresh();
        // Fill bottom 4 rows completely
        for r in (ROWS - 4)..ROWS {
            for c in 0..COLS {
                game.board[r][c] = b'X';
            }
        }
        let cleared = clear_lines(&mut game);
        assert_eq!(cleared, 4);
        assert_eq!(game.score, 800); // 800 * level 1
    }

    #[test]
    fn game_over_when_spawn_blocked() {
        let mut game = fresh();
        // Fill the top rows so spawning is impossible
        for r in 0..4 {
            for c in 0..COLS {
                game.board[r][c] = b'X';
            }
        }
        let ok = spawn(&mut game);
        assert!(!ok);
        assert!(game.game_over);
    }

    #[test]
    fn observe_returns_valid_board_state() {
        do_reset().unwrap();
        let result = do_observe().unwrap();
        assert!(result.get("board").is_some());
        assert!(result.get("current").is_some());
        assert!(result.get("next").is_some());
        assert_eq!(result["game_over"], false);
        let board = result["board"].as_array().unwrap();
        assert_eq!(board.len(), 20);
        assert_eq!(board[0].as_str().unwrap().len(), 10);
    }

    #[test]
    fn reset_clears_everything() {
        // Play a bit then reset
        do_reset().unwrap();
        do_move(&json!({"action": "drop"})).unwrap();
        do_move(&json!({"action": "drop"})).unwrap();
        let after_play = do_observe().unwrap();
        assert!(after_play["score"].as_u64().unwrap() >= 0);

        // Reset
        let result = do_reset().unwrap();
        assert_eq!(result["score"], 0);
        assert_eq!(result["lines"], 0);
        assert_eq!(result["level"], 1);
        assert_eq!(result["game_over"], false);
    }

    #[test]
    fn bag_always_has_all_7_pieces() {
        let mut rng = 42u64;
        let mut bag = Vec::new();
        fill_bag(&mut bag, &mut rng);
        assert_eq!(bag.len(), 7);
        // Check all 7 piece types are present
        for pt in &ALL_PIECES {
            assert!(bag.contains(pt), "bag missing {:?}", pt);
        }
    }

    #[test]
    fn hard_drop_locks_and_spawns_next() {
        do_reset().unwrap();
        let before = do_observe().unwrap();
        let first_type = before["current"]["type"].as_str().unwrap().to_string();
        let next_type = before["next"].as_str().unwrap().to_string();

        let after = do_move(&json!({"action": "drop"})).unwrap();
        // Current piece should now be what was "next"
        assert_eq!(after["current"]["type"].as_str().unwrap(), next_type);
        // Board should have some non-empty cells from the locked piece
        let board = after["board"].as_array().unwrap();
        let has_locked = board.iter().any(|row| {
            row.as_str().unwrap().contains(&first_type)
        });
        assert!(has_locked, "dropped piece should be locked on board");
    }
}
