//! Opcode → cartridge syscall dispatch.
//!
//! For v0.01, every cartridge method is a stub returning `{"ok":true,"stub":"v0.01"}`,
//! except `roclaw.*` which actually emits 6-byte UDP frames via the embedded
//! `BytecodeCompiler` port. The dispatcher's job is to:
//!   1. Look up the cartridge by name.
//!   2. Validate args against the per-method JSON Schema.
//!   3. Route to a method handler (built-in or stubbed).
//!   4. Return the result that gets injected back into the model's prompt.
//!
//! v0.1 will replace stubbed handlers with real implementations and compile
//! per-method schemas into GBNF sub-grammars (eliminating the runtime args
//! validation step on the happy path).

use crate::cartridge::{Cartridge, CartridgeRegistry};
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("unknown cartridge '{0}'")]
    UnknownCartridge(String),
    #[error("unknown method '{cart}.{method}'")]
    UnknownMethod { cart: String, method: String },
    #[error("schema violation in '{cart}.{method}': {errors:?}")]
    SchemaViolation {
        cart: String,
        method: String,
        errors: Vec<String>,
    },
    #[error("handler error in '{cart}.{method}': {message}")]
    HandlerError {
        cart: String,
        method: String,
        message: String,
    },
}

/// Result of a successful dispatch: the JSON value that gets wrapped into
/// `<|result|>…<|/result|>` and injected into the model's next prompt.
pub type DispatchResult = Result<Value, DispatchError>;

pub fn dispatch(
    registry: &CartridgeRegistry,
    cart_name: &str,
    method: &str,
    args: &Value,
) -> DispatchResult {
    let cart = registry
        .get(cart_name)
        .ok_or_else(|| DispatchError::UnknownCartridge(cart_name.to_string()))?;

    if !cart.manifest.methods.contains_key(method) {
        return Err(DispatchError::UnknownMethod {
            cart: cart_name.to_string(),
            method: method.to_string(),
        });
    }

    if let Err(e) = cart.validate_args(method, args) {
        let errors = match e {
            crate::cartridge::CartridgeError::SchemaViolation { errors, .. } => errors,
            other => vec![format!("{other}")],
        };
        return Err(DispatchError::SchemaViolation {
            cart: cart_name.to_string(),
            method: method.to_string(),
            errors,
        });
    }

    // Built-in handlers fire by (cart, method) tuple. v0.01 has only
    // `roclaw.*` implemented for real; everything else stubs.
    builtin_handler(cart, method, args).unwrap_or_else(|| stub_handler(cart_name, method))
}

/// Stub handler returning `{"ok":true,"stub":"v0.01","cart":"…","method":"…"}`.
fn stub_handler(cart: &str, method: &str) -> DispatchResult {
    Ok(json!({
        "ok": true,
        "stub": "v0.01",
        "cart": cart,
        "method": method,
    }))
}

/// Built-in dispatch table for cartridges whose semantics live in the
/// daemon binary (rather than as external processes). For v0.01 this is
/// only `roclaw` — see the `roclaw_handler` module.
fn builtin_handler(cart: &Cartridge, method: &str, args: &Value) -> Option<DispatchResult> {
    match cart.name() {
        "roclaw" => Some(roclaw_handler::dispatch(method, args)),
        "sim_world" => Some(sim_world_handler::dispatch(method, args)),
        _ => None,
    }
}

// ────────────────────────────────────────────────────────────────────────
// roclaw handler — see runtime/roclaw.rs (separate module file).
// ────────────────────────────────────────────────────────────────────────

pub mod roclaw_handler {
    use crate::dispatch::{DispatchError, DispatchResult};
    use serde_json::{json, Value};

    /// Public entry from `dispatch::dispatch()`.
    pub fn dispatch(method: &str, args: &Value) -> DispatchResult {
        // Translate (method, args) → BytecodeFrame, encode, send over UDP.
        // Returns `{"ok":true,"bytes":6,"hex":"AA 01 96 96 01 FF"}` on success.
        let frame = match crate::roclaw::compile(method, args) {
            Ok(b) => b,
            Err(e) => {
                return Err(DispatchError::HandlerError {
                    cart: "roclaw".into(),
                    method: method.into(),
                    message: e,
                })
            }
        };

        // Send. Errors from the UDP transport are surfaced but don't stop
        // the daemon — the model can decide via the result whether to retry.
        let send_result = crate::roclaw::send(&frame);
        let hex = crate::roclaw::format_hex(&frame);
        match send_result {
            Ok(()) => Ok(json!({
                "ok": true,
                "bytes": frame.len(),
                "hex": hex,
            })),
            Err(e) => Ok(json!({
                "ok": false,
                "bytes": frame.len(),
                "hex": hex,
                "error": e,
            })),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────
// sim_world handler — deterministic Markov-chain world for L1 e2e fixture.
// ────────────────────────────────────────────────────────────────────────

pub mod sim_world_handler {
    use crate::dispatch::{DispatchError, DispatchResult};
    use serde_json::{json, Value};
    use std::sync::Mutex;

    // Simple grid-world state. Goal: reach cell (3,0) starting from (0,0)
    // by emitting `move` calls with direction in {north, east, south, west}.
    static STATE: Mutex<(i32, i32)> = Mutex::new((0, 0));
    const GOAL_X: i32 = 3;
    const GOAL_Y: i32 = 0;

    pub fn reset() {
        *STATE.lock().unwrap() = (0, 0);
    }

    pub fn dispatch(method: &str, args: &Value) -> DispatchResult {
        match method {
            "step" => {
                let dir = args
                    .get("dir")
                    .and_then(Value::as_str)
                    .ok_or_else(|| DispatchError::HandlerError {
                        cart: "sim_world".into(),
                        method: method.into(),
                        message: "args.dir must be a string".into(),
                    })?;
                let mut s = STATE.lock().unwrap();
                let (mut x, mut y) = *s;
                match dir {
                    "north" => y += 1,
                    "south" => y -= 1,
                    "east" => x += 1,
                    "west" => x -= 1,
                    other => {
                        return Err(DispatchError::HandlerError {
                            cart: "sim_world".into(),
                            method: method.into(),
                            message: format!("unknown dir '{other}'"),
                        })
                    }
                }
                *s = (x, y);
                let at_goal = x == GOAL_X && y == GOAL_Y;
                Ok(json!({
                    "ok": true,
                    "x": x,
                    "y": y,
                    "at_goal": at_goal,
                }))
            }
            "observe" => {
                let s = STATE.lock().unwrap();
                Ok(json!({
                    "x": s.0,
                    "y": s.1,
                    "at_goal": s.0 == GOAL_X && s.1 == GOAL_Y,
                    "goal_x": GOAL_X,
                    "goal_y": GOAL_Y,
                }))
            }
            "reset" => {
                reset();
                Ok(json!({"ok": true}))
            }
            other => Err(DispatchError::HandlerError {
                cart: "sim_world".into(),
                method: other.into(),
                message: "unknown method".into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_cartridge_errors() {
        let reg = CartridgeRegistry::new();
        let err = dispatch(&reg, "ghost", "method", &json!({})).unwrap_err();
        match err {
            DispatchError::UnknownCartridge(n) => assert_eq!(n, "ghost"),
            _ => panic!("wrong error variant"),
        }
    }

    #[test]
    fn sim_world_step_advances_position() {
        sim_world_handler::reset();
        let r = sim_world_handler::dispatch("step", &json!({"dir": "east"})).unwrap();
        assert_eq!(r["x"], 1);
        assert_eq!(r["y"], 0);
        assert_eq!(r["at_goal"], false);
        let r2 = sim_world_handler::dispatch("observe", &json!({})).unwrap();
        assert_eq!(r2["x"], 1);
    }

    #[test]
    fn sim_world_reaches_goal_in_three_easts() {
        sim_world_handler::reset();
        sim_world_handler::dispatch("step", &json!({"dir": "east"})).unwrap();
        sim_world_handler::dispatch("step", &json!({"dir": "east"})).unwrap();
        let final_step = sim_world_handler::dispatch("step", &json!({"dir": "east"})).unwrap();
        assert_eq!(final_step["at_goal"], true);
    }
}
