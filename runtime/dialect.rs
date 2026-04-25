//! Per-cartridge dialect framework (v0.1).
//!
//! A *dialect* is a compact textual notation that expands to a JSON value
//! conforming to a method's args schema. Refinement §2.3 documents this as
//! the most underweighted optimization in the design — at 8 Hz on Pi 5,
//! every char saved is ~1 ms of latency back, and motion calls are emitted
//! many times per task.
//!
//! Concrete example. The roclaw-motion-v1 dialect:
//!
//! ```text
//! F 150 150     → {"left":150,"right":150}     (forward)
//! B 80 80       → {"left":80,"right":80}       (backward)
//! ```
//!
//! 13 chars on the wire vs. 27 for the JSON form — a 2× saving on a hot
//! opcode path. With single-token opcodes (Month 2 fine-tune), the dialect
//! drops a typical syscall from ~10 tokens to ~6.
//!
//! v0.1 ships:
//! - The dialect parsing/expansion library here.
//! - One built-in dialect: `roclaw-motion-v1`.
//! - Manifest field `methods.<m>.dialect` so cartridges opt in per method.
//! - iod's `<|call|>` parser tries dialect expansion first; falls back to
//!   JSON if no dialect is registered for the (cart, method) pair.
//!
//! v0.5 wires this into the grammar swap so the model emits the compact
//! form natively (the grammar's `json` slot becomes the dialect's grammar
//! sub-rule for the duration of the call). Until then, dialect adoption
//! is opt-in: a wrapper cartridge can pre-translate tool calls.

use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum DialectError {
    #[error("unknown dialect '{0}'")]
    Unknown(String),
    #[error("dialect parse failed: {0}")]
    Parse(String),
}

/// Expand a dialect-encoded args string into a JSON value matching the
/// method's args schema.
///
/// Returns `Err(DialectError::Unknown)` if no dialect is registered for
/// `name` — caller should fall through to plain JSON parsing.
pub fn expand(name: &str, raw: &str) -> Result<Value, DialectError> {
    match name {
        "roclaw-motion-v1" => roclaw_motion_v1::expand(raw),
        "roclaw-rotate-v1" => roclaw_rotate_v1::expand(raw),
        "sim-world-step-v1" => sim_world_step_v1::expand(raw),
        other => Err(DialectError::Unknown(other.to_string())),
    }
}

// ────────────────────────────────────────────────────────────────────────
// roclaw-motion-v1
// ────────────────────────────────────────────────────────────────────────

mod roclaw_motion_v1 {
    use super::*;

    /// `<verb> <left> <right>` where verb is a one-char hint (F/B/L/R/S
    /// for forward/back/turn-left/turn-right/set — informational; the
    /// caller already routed via cart.method, the verb is just a check).
    pub fn expand(raw: &str) -> Result<Value, DialectError> {
        let trimmed = raw.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        // Allow either "F 150 150" or "150 150" — verb optional.
        let (left_s, right_s) = match parts.len() {
            2 => (parts[0], parts[1]),
            3 => (parts[1], parts[2]),
            _ => {
                return Err(DialectError::Parse(format!(
                    "expected '[verb] L R' (2 or 3 tokens), got: {trimmed:?}"
                )));
            }
        };
        let left: f64 = left_s.parse().map_err(|_| {
            DialectError::Parse(format!("left value not a number: {left_s}"))
        })?;
        let right: f64 = right_s.parse().map_err(|_| {
            DialectError::Parse(format!("right value not a number: {right_s}"))
        })?;
        Ok(json!({"left": left, "right": right}))
    }
}

// ────────────────────────────────────────────────────────────────────────
// roclaw-rotate-v1
// ────────────────────────────────────────────────────────────────────────

mod roclaw_rotate_v1 {
    use super::*;

    /// `<degrees> <speed>` — both required, no verb.
    pub fn expand(raw: &str) -> Result<Value, DialectError> {
        let parts: Vec<&str> = raw.trim().split_whitespace().collect();
        if parts.len() != 2 {
            return Err(DialectError::Parse(format!(
                "expected '<degrees> <speed>', got: {raw:?}"
            )));
        }
        let degrees: f64 = parts[0]
            .parse()
            .map_err(|_| DialectError::Parse(format!("degrees not a number: {}", parts[0])))?;
        let speed: f64 = parts[1]
            .parse()
            .map_err(|_| DialectError::Parse(format!("speed not a number: {}", parts[1])))?;
        Ok(json!({"degrees": degrees, "speed": speed}))
    }
}

// ────────────────────────────────────────────────────────────────────────
// sim-world-step-v1
// ────────────────────────────────────────────────────────────────────────

mod sim_world_step_v1 {
    use super::*;

    /// `N`/`E`/`S`/`W` — single char per step.
    pub fn expand(raw: &str) -> Result<Value, DialectError> {
        let dir = match raw.trim().to_ascii_uppercase().as_str() {
            "N" | "NORTH" => "north",
            "E" | "EAST" => "east",
            "S" | "SOUTH" => "south",
            "W" | "WEST" => "west",
            other => {
                return Err(DialectError::Parse(format!(
                    "expected N/E/S/W, got: {other:?}"
                )));
            }
        };
        Ok(json!({"dir": dir}))
    }
}

// ────────────────────────────────────────────────────────────────────────
// Dialect routing per (cart, method) — v0.1 hardcoded; v0.5 reads from
// manifest.methods.<m>.dialect.
// ────────────────────────────────────────────────────────────────────────

pub fn for_method(cart: &str, method: &str) -> Option<&'static str> {
    match (cart, method) {
        ("roclaw", "forward")
        | ("roclaw", "backward")
        | ("roclaw", "turn_left")
        | ("roclaw", "turn_right")
        | ("roclaw", "set_speed")
        | ("roclaw", "move_steps")
        | ("roclaw", "move_steps_r") => Some("roclaw-motion-v1"),
        ("roclaw", "rotate_cw") | ("roclaw", "rotate_ccw") => Some("roclaw-rotate-v1"),
        ("sim_world", "step") => Some("sim-world-step-v1"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_three_token_form() {
        let v = expand("roclaw-motion-v1", "F 150 150").unwrap();
        assert_eq!(v, json!({"left": 150.0, "right": 150.0}));
    }

    #[test]
    fn motion_two_token_form_no_verb() {
        let v = expand("roclaw-motion-v1", "200 100").unwrap();
        assert_eq!(v, json!({"left": 200.0, "right": 100.0}));
    }

    #[test]
    fn motion_rejects_garbage() {
        assert!(expand("roclaw-motion-v1", "not a number").is_err());
        assert!(expand("roclaw-motion-v1", "F 1").is_err());
    }

    #[test]
    fn rotate_two_token() {
        let v = expand("roclaw-rotate-v1", "90 100").unwrap();
        assert_eq!(v, json!({"degrees": 90.0, "speed": 100.0}));
    }

    #[test]
    fn sim_step_short_forms() {
        assert_eq!(
            expand("sim-world-step-v1", "N").unwrap(),
            json!({"dir": "north"})
        );
        assert_eq!(
            expand("sim-world-step-v1", "east").unwrap(),
            json!({"dir": "east"})
        );
    }

    #[test]
    fn for_method_routes_roclaw_motion() {
        assert_eq!(for_method("roclaw", "forward"), Some("roclaw-motion-v1"));
        assert_eq!(for_method("roclaw", "rotate_cw"), Some("roclaw-rotate-v1"));
        assert_eq!(for_method("sim_world", "step"), Some("sim-world-step-v1"));
        assert_eq!(for_method("cooking", "plan_menu"), None);
    }

    #[test]
    fn unknown_dialect_returns_unknown_error() {
        assert!(matches!(
            expand("not-real", "x"),
            Err(DialectError::Unknown(_))
        ));
    }

    #[test]
    fn motion_compresses_vs_json() {
        let dialect_form = "F 150 150";
        let json_form = r#"{"left":150,"right":150}"#;
        // Dialect should be at least 2x shorter — refinement §2.3 thesis.
        assert!(json_form.len() >= dialect_form.len() * 2);
    }
}
