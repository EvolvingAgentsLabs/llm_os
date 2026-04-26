//! Context compactor — Rust port of
//! `skillos_mini/mobile/src/lib/llm/compactor.ts`.
//!
//! Two modes:
//! - **fifo** — drop the oldest non-recent messages, no LLM call (deterministic).
//! - **llm**  — summarize removed messages via an LLM call (better fidelity, slower).
//!
//! v0.01 ships fifo-as-default (deterministic + fast on Pi 5); llm mode is
//! opt-in via [`compact_async`]. v0.1 will run the compactor on a
//! background sub-model so it doesn't stall the kernel.
//!
//! Threshold: 70% of the model's effective context window (refinement §2.9).
//! Hardcoded model→window catalog mirrors the TS `MODEL_CONTEXT_WINDOWS`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

pub const DEFAULT_RATIO: f64 = 0.7;
pub const MIN_THRESHOLD: usize = 8_000;

// ─── ISA state tracking for compaction safety (§2 NEXT_STEPS) ────────────

/// Extracted ISA state from the token history before compaction. The compactor
/// walks the dropped window and captures this; it's then serialized as a
/// `<|state|>{...}<|/state|>` preamble prepended to the summary so the GBNF
/// state machine can resume coherently.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IsaState {
    pub loop_depth: u32,
    /// File descriptors with pending `<|result|>` (from unmatched read/call/wait/fault/policy).
    pub pending_results: Vec<String>,
    /// Unmatched `<|write|>` awaiting `<|ack|>`.
    pub pending_acks: Vec<String>,
    /// `<|fork|>` with no terminating `<|halt|>`.
    pub open_forks: Vec<String>,
    /// Active `<|loop|>` goals (innermost last).
    pub open_loops: Vec<String>,
}

impl IsaState {
    /// Returns true if there is any ISA state that must survive compaction.
    pub fn is_nontrivial(&self) -> bool {
        self.loop_depth > 0
            || !self.pending_results.is_empty()
            || !self.pending_acks.is_empty()
            || !self.open_forks.is_empty()
            || !self.open_loops.is_empty()
    }

    /// Serialize as the `<|state|>{...}<|/state|>` preamble string.
    pub fn to_preamble(&self) -> String {
        let payload = json!({
            "loop_depth": self.loop_depth,
            "pending_results": self.pending_results,
            "pending_acks": self.pending_acks,
            "open_forks": self.open_forks,
            "open_loops": self.open_loops,
        });
        format!("<|state|>{}<|/state|>\n", payload)
    }
}

/// Walk a sequence of message contents and extract the ISA state at the
/// boundary. Scans for opcode tokens and tracks nesting/pending state.
pub fn extract_isa_state(messages: &[ChatMessage]) -> IsaState {
    let mut state = IsaState::default();
    for msg in messages {
        for line in msg.content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("<|loop|>goal=") {
                state.loop_depth += 1;
                if let Some(goal) = trimmed.strip_prefix("<|loop|>goal=") {
                    state.open_loops.push(goal.trim().to_string());
                }
            } else if trimmed == "<|break|>" {
                state.loop_depth = state.loop_depth.saturating_sub(1);
                state.open_loops.pop();
            } else if trimmed.starts_with("<|fork|>goal=") {
                if let Some(goal) = trimmed.strip_prefix("<|fork|>goal=") {
                    state.open_forks.push(goal.trim().to_string());
                }
            } else if trimmed.starts_with("<|read|>") {
                // Pending until <|result|> arrives
                if let Some(fd) = extract_fd(trimmed, "<|read|>") {
                    state.pending_results.push(fd);
                }
            } else if trimmed.starts_with("<|call|>") {
                state.pending_results.push("call".to_string());
            } else if trimmed.starts_with("<|wait|>") {
                if let Some(fd) = extract_fd(trimmed, "<|wait|>") {
                    state.pending_results.push(fd);
                }
            } else if trimmed.starts_with("<|fault|>") {
                state.pending_results.push("fault".to_string());
            } else if trimmed == "<|policy|>" {
                state.pending_results.push("policy".to_string());
            } else if trimmed.starts_with("<|write|>") {
                if let Some(fd) = extract_fd(trimmed, "<|write|>") {
                    state.pending_acks.push(fd);
                }
            } else if trimmed.starts_with("<|result|>") || trimmed.starts_with("<|/result|>") {
                // Result received — clear one pending
                state.pending_results.pop();
            } else if trimmed.starts_with("<|ack|>") {
                state.pending_acks.pop();
            } else if trimmed.starts_with("<|halt|>") {
                // Halt clears fork context (one fork completed)
                state.open_forks.pop();
            }
        }
    }
    state
}

fn extract_fd(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?;
    let rest = rest.strip_prefix("fd=")?;
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    Some(format!("fd{}", &rest[..end]))
}

/// Per-model effective context window (tokens). Mirrors the catalog in
/// `compactor.ts:17-33`. RULER-corrected — uses *effective* window where
/// vendor-claimed and effective diverge (refinement §6).
pub fn model_context_windows() -> &'static HashMap<&'static str, usize> {
    static MAP: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m: HashMap<&'static str, usize> = HashMap::new();
        m.insert("qwen/qwen3.6-plus:free", 32_000);
        m.insert("qwen/qwen3-plus", 32_000);
        m.insert("gemini-2.5-flash", 1_048_576);
        m.insert("gemini-2.5-pro", 1_048_576);
        m.insert("gemini-2.0-flash", 1_048_576);
        m.insert("gemma4", 128_000);
        m.insert("gemma4:e2b", 128_000);
        m.insert("gemma4:e4b", 128_000);
        m.insert("gemma4:26b", 256_000);
        m.insert("gemma4:31b", 256_000);
        m.insert("google/gemma-4-26b-a4b-it", 131_072);
        m.insert("qwen2.5-1.5b-instruct-q4_k_m", 32_768);
        m.insert("gemma-2-2b-it-q4_k_m", 8_192);
        m.insert("gemma-2-2b-it-litertlm", 8_192);
        m
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    pub preserve_recent: usize,
    pub max_estimated_tokens: usize,
    pub llm_summary_min_messages: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            preserve_recent: 4,
            max_estimated_tokens: 10_000,
            llm_summary_min_messages: 4,
        }
    }
}

impl CompactionConfig {
    pub fn for_model(mut self, model_name: &str) -> Self {
        if let Some(&window) = model_context_windows().get(model_name) {
            let from_window = (window as f64 * DEFAULT_RATIO).floor() as usize;
            self.max_estimated_tokens = from_window.max(MIN_THRESHOLD);
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// char/4 + 1 token-cost heuristic. Same as compactor.ts:73-79.
pub fn estimate_tokens(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|m| m.content.len() / 4 + 1)
        .sum()
}

pub fn should_compact(messages: &[ChatMessage], cfg: &CompactionConfig) -> bool {
    messages.len() > cfg.preserve_recent && estimate_tokens(messages) >= cfg.max_estimated_tokens
}

#[derive(Debug, Clone)]
pub struct CompactResult {
    pub messages: Vec<ChatMessage>,
    pub summary: String,
    pub compacted: usize,
    /// ISA state extracted from the dropped window, if nontrivial.
    /// `Some(state)` means a `<|state|>` preamble was injected.
    pub isa_state: Option<IsaState>,
}

/// ISA-aware synchronous textual compaction (§2 NEXT_STEPS). Drops the
/// oldest messages but first extracts ISA state from the dropped window.
/// If state is nontrivial (open loops, pending results, etc.), prepends a
/// `<|state|>{...}<|/state|>` preamble so the GBNF state machine can
/// resume coherently after compaction.
pub fn compact(messages: &[ChatMessage], cfg: &CompactionConfig) -> CompactResult {
    if !should_compact(messages, cfg) {
        return CompactResult {
            messages: messages.to_vec(),
            summary: String::new(),
            compacted: 0,
            isa_state: None,
        };
    }
    let keep_from = messages.len().saturating_sub(cfg.preserve_recent);
    let removed = &messages[..keep_from];
    let preserved = &messages[keep_from..];

    // §2: Extract ISA state from the window being dropped.
    let isa_state = extract_isa_state(removed);
    let summary = summarize_textual(removed);

    let mut out = Vec::with_capacity(preserved.len() + 2);

    // If ISA state is nontrivial, inject the state preamble BEFORE the
    // summary so the parser rehydrates loop depth before seeing new opcodes.
    if isa_state.is_nontrivial() {
        out.push(ChatMessage {
            role: "system".into(),
            content: isa_state.to_preamble(),
        });
    }

    out.push(summary_message(&summary));
    out.extend_from_slice(preserved);

    CompactResult {
        messages: out,
        summary,
        compacted: removed.len(),
        isa_state: if isa_state.is_nontrivial() { Some(isa_state) } else { None },
    }
}

fn summarize_textual(messages: &[ChatMessage]) -> String {
    messages
        .iter()
        .map(|m| {
            let mut content = m.content.clone();
            if content.len() > 160 {
                content.truncate(160);
                content.push('…');
            }
            format!("- {}: {}", m.role, content)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn summary_message(summary: &str) -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: format!(
            "Session continues. Prior context:\n\n{summary}\n\nRecent messages preserved verbatim. Continue without recapping."
        ),
    }
}

/// Async LLM-powered compaction. v0.01 stub: falls through to textual mode.
/// v0.1 wires this through the existing iod HTTP client.
pub fn compact_async(messages: &[ChatMessage], cfg: &CompactionConfig) -> CompactResult {
    // For v0.01, identical to sync compact. Wired here so callers can swap
    // in the LLM-summary path in v0.1 without changing the call site.
    compact(messages, cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
        }
    }

    #[test]
    fn estimate_uses_char_div_4_heuristic() {
        let m = vec![msg("user", "abcdefgh"), msg("assistant", "ab")];
        // 8/4+1 + 2/4+1 = 3 + 1 = 4
        assert_eq!(estimate_tokens(&m), 4);
    }

    #[test]
    fn no_compaction_under_threshold() {
        let cfg = CompactionConfig::default();
        let m = vec![msg("user", "hi"), msg("assistant", "hello")];
        let r = compact(&m, &cfg);
        assert_eq!(r.compacted, 0);
        assert_eq!(r.messages.len(), 2);
        assert!(r.isa_state.is_none());
    }

    #[test]
    fn compaction_preserves_recent_n() {
        let cfg = CompactionConfig {
            preserve_recent: 2,
            max_estimated_tokens: 1,
            llm_summary_min_messages: 4,
        };
        let m = vec![
            msg("user", "old1"),
            msg("user", "old2"),
            msg("user", "old3"),
            msg("user", "recent1"),
            msg("user", "recent2"),
        ];
        let r = compact(&m, &cfg);
        assert_eq!(r.compacted, 3);
        // 1 summary + 2 preserved (no ISA state in plain text).
        assert_eq!(r.messages.len(), 3);
        assert!(r.messages[0].content.contains("Prior context"));
        assert_eq!(r.messages[1].content, "recent1");
        assert_eq!(r.messages[2].content, "recent2");
        assert!(r.isa_state.is_none());
    }

    #[test]
    fn isa_aware_compaction_injects_state_preamble() {
        let cfg = CompactionConfig {
            preserve_recent: 1,
            max_estimated_tokens: 1,
            llm_summary_min_messages: 4,
        };
        let m = vec![
            msg("assistant", "<|loop|>goal=navigate\n<|call|>roclaw.forward {\"speed\":100} <|/call|>"),
            msg("user", "<|result|>{\"ok\":true}<|/result|>"),
            msg("assistant", "still running"),
        ];
        let r = compact(&m, &cfg);
        assert!(r.isa_state.is_some());
        let state = r.isa_state.unwrap();
        assert_eq!(state.loop_depth, 1);
        assert_eq!(state.open_loops, vec!["navigate"]);
        // First message should be the state preamble
        assert!(r.messages[0].content.contains("<|state|>"));
        assert!(r.messages[0].content.contains("loop_depth"));
    }

    #[test]
    fn extract_isa_state_tracks_nested_loops() {
        let msgs = vec![
            msg("assistant", "<|loop|>goal=outer\n<|loop|>goal=inner"),
        ];
        let state = extract_isa_state(&msgs);
        assert_eq!(state.loop_depth, 2);
        assert_eq!(state.open_loops, vec!["outer", "inner"]);
        assert!(state.is_nontrivial());
    }

    #[test]
    fn extract_isa_state_trivial_when_balanced() {
        let msgs = vec![
            msg("assistant", "<|loop|>goal=x\n<|break|>\n<|halt|>status=success"),
        ];
        let state = extract_isa_state(&msgs);
        assert_eq!(state.loop_depth, 0);
        assert!(!state.is_nontrivial());
    }

    #[test]
    fn for_model_overrides_threshold_from_catalog() {
        let cfg = CompactionConfig::default().for_model("gemma4:e2b");
        let expected = (128_000.0_f64 * DEFAULT_RATIO).floor() as usize;
        assert_eq!(cfg.max_estimated_tokens, expected);
    }

    #[test]
    fn for_model_unknown_keeps_default() {
        let cfg = CompactionConfig::default().for_model("not-a-real-model");
        assert_eq!(cfg.max_estimated_tokens, 10_000);
    }
}
