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
use std::collections::HashMap;
use std::sync::OnceLock;

pub const DEFAULT_RATIO: f64 = 0.7;
pub const MIN_THRESHOLD: usize = 8_000;

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
}

/// Synchronous textual compaction (FIFO mode equivalent — same as
/// `compactMessages` in TS). Drops the oldest messages, replaces them with
/// a synthesized "Session continues" preamble carrying a short bullet
/// summary.
pub fn compact(messages: &[ChatMessage], cfg: &CompactionConfig) -> CompactResult {
    if !should_compact(messages, cfg) {
        return CompactResult {
            messages: messages.to_vec(),
            summary: String::new(),
            compacted: 0,
        };
    }
    let keep_from = messages.len().saturating_sub(cfg.preserve_recent);
    let removed = &messages[..keep_from];
    let preserved = &messages[keep_from..];
    let summary = summarize_textual(removed);
    let mut out = Vec::with_capacity(preserved.len() + 1);
    out.push(summary_message(&summary));
    out.extend_from_slice(preserved);
    CompactResult {
        messages: out,
        summary,
        compacted: removed.len(),
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
        // 1 summary + 2 preserved.
        assert_eq!(r.messages.len(), 3);
        assert!(r.messages[0].content.contains("Prior context"));
        assert_eq!(r.messages[1].content, "recent1");
        assert_eq!(r.messages[2].content, "recent2");
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
