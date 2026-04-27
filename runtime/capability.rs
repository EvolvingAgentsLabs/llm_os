//! Capability enforcement at decode time via logit bias (v0.5).
//!
//! When a task is bound to a particular cartridge subset, the daemon
//! constructs a `logit_bias` array banning the opcodes the task isn't
//! authorized to emit. llama-server applies the bias during sampling, so
//! the model literally cannot emit a banned token.
//!
//! ## Bootstrap-kernel caveats
//!
//! In the v0.01/v0.1-rc1 bootstrap kernels, opcodes tokenize to 4–12
//! tokens each (see `docs/tokenization-report.md`). Banning the entire
//! opcode means banning all constituent tokens — but those tokens overlap
//! with non-opcode usage (`<`, `|`, `read`, `>`). True opcode banning at
//! decode requires single-token opcodes, which only the v0.1-final
//! fine-tuned kernel has.
//!
//! For bootstrap kernels, the v0.5 capability layer:
//! - **Bans the most-discriminating constituent token** of each opcode
//!   (e.g. for `<|read|>`, ban `read` only when the previous tokens
//!   suggest opcode context — but llama-server's logit_bias is stateless
//!   per request, so we can only do unconditional bans).
//! - **Logs the false-positive risk** at startup so users know.
//!
//! For fine-tuned kernels (v0.1 final + later), the bias is exact: one
//! token id per opcode, banned with `false`.
//!
//! ## Lookup
//!
//! Token IDs are queried once per kernel via llama-server's `/tokenize`
//! endpoint. Cached for the daemon lifetime; the kernel doesn't change
//! mid-run.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use crate::parser::Opcode;

#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    #[error("tokenizer query failed: {0}")]
    TokenizerHttp(String),
    #[error("tokenizer returned no tokens for '{0}'")]
    EmptyTokenization(String),
    #[error("malformed tokenizer response: {0}")]
    Malformed(String),
}

/// All 18 opcode + envelope tokens. The set of tokens any cartridge
/// might need to emit/receive (v0.5 currently bans only opcode tokens
/// for the calling task; the envelope tokens are daemon-injected only).
pub const ALL_OPCODE_STRINGS: &[&str] = &[
    "<|read|>",
    "<|write|>",
    "<|call|>",
    "<|/call|>",
    "<|yield|>",
    "<|fork|>",
    "<|wait|>",
    "<|loop|>",
    "<|break|>",
    "<|halt|>",
    "<|think|>",
    "<|/think|>",
    "<|commit|>",
    "<|fault|>",
    "<|policy|>",
];

/// Map an [`Opcode`] to its emit-side string.
pub fn opcode_string(op: Opcode) -> &'static str {
    match op {
        Opcode::Read => "<|read|>",
        Opcode::Write => "<|write|>",
        Opcode::Call => "<|call|>",
        Opcode::Yield => "<|yield|>",
        Opcode::Fork => "<|fork|>",
        Opcode::Wait => "<|wait|>",
        Opcode::Loop => "<|loop|>",
        Opcode::Break => "<|break|>",
        Opcode::Halt => "<|halt|>",
        Opcode::Think => "<|think|>",
        Opcode::Commit => "<|commit|>",
        Opcode::Fault => "<|fault|>",
        Opcode::Policy => "<|policy|>",
        Opcode::State => "<|state|>",
    }
}

#[derive(Debug, Serialize)]
struct TokenizeRequest<'a> {
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct TokenizeResponse {
    tokens: Vec<i64>,
}

/// Query llama-server's `/tokenize` endpoint for the token IDs of each
/// opcode string. Returns a map opcode-string → first-token-id.
///
/// First-token-id is what we use for banning: in the bootstrap kernels
/// the opcode starts with `<` (or `<|` if the tokenizer merges them) and
/// banning that prefix-token nukes all opcodes equally — useless. So
/// we use the **second token** when the first is shared across opcodes.
/// Pragmatic: this is "best-effort capability enforcement on bootstrap";
/// fine-tuned kernels make it exact.
pub fn fetch_token_map(
    server_url: &str,
    timeout: Duration,
) -> Result<HashMap<String, Vec<i64>>, CapabilityError> {
    let mut out = HashMap::new();
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    for op in ALL_OPCODE_STRINGS {
        let url = format!("{}/tokenize", server_url.trim_end_matches('/'));
        let body = TokenizeRequest { content: op };
        let resp: TokenizeResponse = agent
            .post(&url)
            .send_json(&body)
            .map_err(|e| CapabilityError::TokenizerHttp(format!("{op}: {e}")))?
            .into_json()
            .map_err(|e| CapabilityError::Malformed(format!("{op}: {e}")))?;
        if resp.tokens.is_empty() {
            return Err(CapabilityError::EmptyTokenization(op.to_string()));
        }
        out.insert(op.to_string(), resp.tokens);
    }
    Ok(out)
}

/// Construct the `logit_bias` array banning the listed opcodes.
///
/// `false` in JSON means "ban entirely" (per llama-server's logit_bias
/// schema). For multi-token opcodes, we ban the **most distinctive
/// token** — heuristic: the longest token among the constituents that
/// is unique across the opcode set. If no unique token exists, we fall
/// through to banning all constituent tokens (which has false positives
/// — see module docs).
pub fn build_logit_bias(
    token_map: &HashMap<String, Vec<i64>>,
    banned_opcodes: &[&str],
) -> Vec<serde_json::Value> {
    use serde_json::Value;

    // Build a frequency map across all opcodes so we can identify
    // tokens that are unique per opcode (= safe to ban without
    // affecting other opcodes).
    let mut token_count: HashMap<i64, u32> = HashMap::new();
    for tokens in token_map.values() {
        for &t in tokens {
            *token_count.entry(t).or_insert(0) += 1;
        }
    }

    let mut bias = Vec::new();
    for op in banned_opcodes {
        let tokens = match token_map.get(*op) {
            Some(t) => t,
            None => continue,
        };
        // Prefer the unique tokens. If none, ban them all (acknowledging
        // false-positive risk).
        let unique: Vec<i64> = tokens
            .iter()
            .copied()
            .filter(|t| token_count.get(t).copied().unwrap_or(0) == 1)
            .collect();
        let to_ban = if !unique.is_empty() {
            unique
        } else {
            tokens.clone()
        };
        for t in to_ban {
            bias.push(Value::Array(vec![
                Value::Number(serde_json::Number::from(t)),
                Value::Bool(false),
            ]));
        }
    }
    bias
}

/// Compute the set of opcodes a task is allowed to emit, given the
/// active cartridge's manifest.
///
/// Default-deny logic: a cartridge's `allowed_opcodes` is the white-list
/// of *target* opcodes (what cart can be the target of). Currently every
/// cartridge needs `call` to be invocable. Other opcodes are
/// task-scoped, not cartridge-scoped, so v0.5 bans nothing extra unless
/// the daemon is launched with `--ban-opcodes`.
pub fn compute_banned_opcodes(extra_bans: &[String]) -> Vec<&str> {
    let bans: Vec<&str> = extra_bans.iter().map(String::as_str).collect();
    bans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_token_preferred_when_available() {
        let mut m = HashMap::new();
        // <|read|> tokens [1, 2, 3]; <|write|> tokens [1, 2, 4]
        // Token 3 is unique to read; 4 unique to write; 1+2 are shared.
        m.insert("<|read|>".into(), vec![1, 2, 3]);
        m.insert("<|write|>".into(), vec![1, 2, 4]);
        let bias = build_logit_bias(&m, &["<|read|>"]);
        // Should contain ONLY token 3 (the unique one).
        assert_eq!(bias.len(), 1);
        let arr = bias[0].as_array().unwrap();
        assert_eq!(arr[0].as_i64(), Some(3));
        assert_eq!(arr[1].as_bool(), Some(false));
    }

    #[test]
    fn falls_back_to_all_constituents_when_no_unique() {
        let mut m = HashMap::new();
        m.insert("<|read|>".into(), vec![1, 2]);
        m.insert("<|write|>".into(), vec![1, 2]); // identical tokenization
        let bias = build_logit_bias(&m, &["<|read|>"]);
        // No unique → ban both constituents.
        assert_eq!(bias.len(), 2);
    }

    #[test]
    fn opcode_string_round_trip() {
        assert_eq!(opcode_string(Opcode::Read), "<|read|>");
        assert_eq!(opcode_string(Opcode::Halt), "<|halt|>");
        assert_eq!(opcode_string(Opcode::Fault), "<|fault|>");
    }
}
