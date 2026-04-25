//! RoClaw bytecode compiler + UDP transport — Rust port of
//! `RoClaw/src/2_qwen_cerebellum/bytecode_compiler.ts` and
//! `RoClaw/src/2_qwen_cerebellum/udp_transmitter.ts`.
//!
//! 13 opcodes (per the actual RoClaw source — the design doc's "14 with ACK"
//! claim is wrong; current `Opcode` const has 13 entries, no ACK). 6-byte
//! frames over UDP to `127.0.0.1:4210`. Frame layout:
//!
//! ```text
//! [0xAA] [OPCODE] [PARAM_L] [PARAM_R] [CHECKSUM] [0xFF]
//! ```
//!
//! `CHECKSUM = OPCODE ^ PARAM_L ^ PARAM_R` (XOR of the three middle bytes).

use serde_json::Value;
use std::net::UdpSocket;

pub const FRAME_START: u8 = 0xAA;
pub const FRAME_END: u8 = 0xFF;
pub const FRAME_SIZE: usize = 6;

/// 13 opcodes from RoClaw v1.1, byte values matching `bytecode_compiler.ts:20-34`.
pub mod opcode {
    pub const MOVE_FORWARD: u8 = 0x01;
    pub const MOVE_BACKWARD: u8 = 0x02;
    pub const TURN_LEFT: u8 = 0x03;
    pub const TURN_RIGHT: u8 = 0x04;
    pub const ROTATE_CW: u8 = 0x05;
    pub const ROTATE_CCW: u8 = 0x06;
    pub const STOP: u8 = 0x07;
    pub const GET_STATUS: u8 = 0x08;
    pub const SET_SPEED: u8 = 0x09;
    pub const MOVE_STEPS: u8 = 0x0A;
    pub const MOVE_STEPS_R: u8 = 0x0B;
    pub const LED_SET: u8 = 0x10;
    pub const RESET: u8 = 0xFE;
}

/// Default UDP destination — matches RoClaw's `udp_transmitter.ts:48`.
pub const DEFAULT_TARGET: &str = "127.0.0.1:4210";

fn checksum(opcode: u8, l: u8, r: u8) -> u8 {
    opcode ^ l ^ r
}

pub fn encode(opcode: u8, l: u8, r: u8) -> [u8; FRAME_SIZE] {
    [FRAME_START, opcode, l, r, checksum(opcode, l, r), FRAME_END]
}

pub fn format_hex(frame: &[u8]) -> String {
    frame
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Translate a cartridge method + JSON args into a 6-byte frame.
///
/// Handles the same name-shape mapping as `BytecodeCompiler.tryParseToolCall`:
/// motion methods take `speed_l`/`speed_r`, rotates take `degrees`/`speed`,
/// stop/status/reset take no args.
///
/// Args bytes are clamped to `[0, 255]` and fractional values in `[0, 1]`
/// are scaled to byte range (mirrors Gemini Robotics-ER's normalized output
/// per `bytecode_compiler.ts:380-388`).
pub fn compile(method: &str, args: &Value) -> Result<[u8; FRAME_SIZE], String> {
    let (op, l, r) = match method {
        "forward" | "move_forward" => motion_pair(opcode::MOVE_FORWARD, args, "left", "right")?,
        "backward" | "move_backward" => motion_pair(opcode::MOVE_BACKWARD, args, "left", "right")?,
        "turn_left" => motion_pair(opcode::TURN_LEFT, args, "left", "right")?,
        "turn_right" => motion_pair(opcode::TURN_RIGHT, args, "left", "right")?,
        "rotate_cw" => rotate_pair(opcode::ROTATE_CW, args)?,
        "rotate_ccw" => rotate_pair(opcode::ROTATE_CCW, args)?,
        "stop" => (opcode::STOP, 0, 0),
        "get_status" => (opcode::GET_STATUS, 0, 0),
        "set_speed" => motion_pair(opcode::SET_SPEED, args, "left", "right")?,
        "move_steps" => motion_pair(opcode::MOVE_STEPS, args, "left", "right")?,
        "move_steps_r" => motion_pair(opcode::MOVE_STEPS_R, args, "left", "right")?,
        "led_set" => motion_pair(opcode::LED_SET, args, "led", "value")?,
        "reset" => (opcode::RESET, 0, 0),
        other => return Err(format!("unknown roclaw method '{other}'")),
    };
    Ok(encode(op, l, r))
}

fn motion_pair(op: u8, args: &Value, k1: &str, k2: &str) -> Result<(u8, u8, u8), String> {
    let l = arg_byte(args, k1).unwrap_or(0);
    let r = arg_byte(args, k2).unwrap_or(0);
    Ok((op, l, r))
}

fn rotate_pair(op: u8, args: &Value) -> Result<(u8, u8, u8), String> {
    let degrees = arg_byte(args, "degrees").unwrap_or(0);
    let speed = arg_byte(args, "speed").unwrap_or(0);
    Ok((op, degrees, speed))
}

fn arg_byte(args: &Value, key: &str) -> Option<u8> {
    let v = args.get(key)?;
    let n = v.as_f64()?;
    let scaled = if (0.0..=1.0).contains(&n) && n.fract() != 0.0 {
        n * 255.0
    } else {
        n
    };
    Some(scaled.clamp(0.0, 255.0).round() as u8)
}

/// Send a frame over UDP to the configured RoClaw target.
///
/// The target address is read from `LLM_OS_ROCLAW_ADDR` if set, else
/// [`DEFAULT_TARGET`]. The send is best-effort — a missing destination is
/// not a daemon-fatal error; the model decides what to do via the result.
pub fn send(frame: &[u8]) -> Result<(), String> {
    let target = std::env::var("LLM_OS_ROCLAW_ADDR").unwrap_or_else(|_| DEFAULT_TARGET.into());
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("bind failed: {e}"))?;
    socket
        .send_to(frame, &target)
        .map_err(|e| format!("send_to({target}) failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn forward_150_150_matches_design_example() {
        // Design §1.3: AA 01 96 96 ?? FF — XOR(0x01, 0x96, 0x96) = 0x01
        let f = compile("forward", &json!({"left": 150, "right": 150})).unwrap();
        assert_eq!(f, [0xAA, 0x01, 0x96, 0x96, 0x01, 0xFF]);
        assert_eq!(format_hex(&f), "AA 01 96 96 01 FF");
    }

    #[test]
    fn stop_zero_args() {
        let f = compile("stop", &json!({})).unwrap();
        assert_eq!(f, [0xAA, 0x07, 0x00, 0x00, 0x07, 0xFF]);
    }

    #[test]
    fn rotate_cw_uses_degrees_speed() {
        let f = compile("rotate_cw", &json!({"degrees": 90, "speed": 100})).unwrap();
        assert_eq!(f[0], 0xAA);
        assert_eq!(f[1], 0x05);
        assert_eq!(f[2], 90);
        assert_eq!(f[3], 100);
        assert_eq!(f[5], 0xFF);
    }

    #[test]
    fn fractional_args_scale_to_byte_range() {
        // 0.5 * 255 = 127.5 → 128.
        let f = compile("forward", &json!({"left": 0.5, "right": 0.5})).unwrap();
        assert_eq!(f[2], 128);
        assert_eq!(f[3], 128);
    }

    #[test]
    fn integer_args_clamp_to_byte_range() {
        let f = compile("forward", &json!({"left": 1000, "right": -50})).unwrap();
        assert_eq!(f[2], 255);
        assert_eq!(f[3], 0);
    }

    #[test]
    fn unknown_method_errors() {
        assert!(compile("teleport", &json!({})).is_err());
    }

    #[test]
    fn checksum_xor_correct() {
        assert_eq!(checksum(0x01, 0x96, 0x96), 0x01);
        assert_eq!(checksum(0x07, 0x00, 0x00), 0x07);
    }
}
