//! Subagent cartridges (v0.5).
//!
//! Cartridges with `subagent: true` get an [`LLMProxy`] handle — they can
//! call `proxy.chat(...)` to do sub-inference. Pattern lifted from
//! `skillos_mini`'s `__skillos.llm` injected proxy for JS-skills
//! cartridges (refinement §2.6).
//!
//! v0.5 ships:
//! - The [`Subagent`] trait.
//! - One built-in subagent: [`summarize`] — takes long text, returns a
//!   short summary. Demonstrates the proxy + dispatch wiring without
//!   needing to load arbitrary code.
//! - Loader acceptance of `subagent: true` (v0.01 rejected this).
//! - Manifest validation: methods on subagent cartridges must declare
//!   their args schema as usual; the body decides whether to call the
//!   proxy.
//!
//! v1.0+ adds:
//! - Dynamic loading of WASM-sandboxed subagent cartridges (skillos_mini
//!   iframe pattern but with WASM instead of iframe — Pi 5 has no DOM).
//! - Recursive subagent calls (subagent A invokes subagent B).
//! - Capability propagation: subagent inherits or restricts the parent
//!   task's capability set via [`crate::capability`].

use crate::cloud::{forward_fault, CloudConfig, Message as CloudMessage};
use crate::dispatch::{DispatchError, DispatchResult};
use serde_json::{json, Value};

/// Trait every subagent implements. `dispatch` is the entry point from
/// the daemon's `Statement::Call` handler when the cartridge has
/// `subagent: true`.
pub trait Subagent: Send + Sync {
    fn name(&self) -> &str;
    fn dispatch(
        &self,
        method: &str,
        args: &Value,
        proxy: &dyn LLMProxy,
    ) -> DispatchResult;
}

/// LLM proxy injected into subagent calls. v0.5 implementation routes
/// to the cloud (since the local model is busy serving the parent task);
/// v1.0 may add a "share local model via slot" path.
pub trait LLMProxy: Send + Sync {
    fn chat(&self, system: &str, user: &str) -> Result<String, String>;
}

/// Default proxy backed by [`crate::cloud`]. Reads creds from env per
/// `docs/dual-brain-deployment.md`.
pub struct CloudProxy {
    cfg: CloudConfig,
}

impl CloudProxy {
    pub fn from_env() -> Self {
        Self {
            cfg: CloudConfig::from_env(),
        }
    }
}

impl LLMProxy for CloudProxy {
    fn chat(&self, system: &str, user: &str) -> Result<String, String> {
        let messages = vec![CloudMessage {
            role: "system".into(),
            content: system.into(),
        }];
        let payload = json!({
            "intent": "subagent_chat",
            "user": user,
        });
        forward_fault(&self.cfg, &messages, &payload).map_err(|e| format!("{e}"))
    }
}

/// Lookup table of built-in subagents. v0.5 has one; v1.0 expands.
pub fn builtin(name: &str) -> Option<Box<dyn Subagent>> {
    match name {
        "summarize" => Some(Box::new(summarize::Summarize)),
        _ => None,
    }
}

// ────────────────────────────────────────────────────────────────────────
// summarize — the v0.5 reference subagent
// ────────────────────────────────────────────────────────────────────────

pub mod summarize {
    use super::*;

    pub struct Summarize;

    impl Subagent for Summarize {
        fn name(&self) -> &str {
            "summarize"
        }

        fn dispatch(
            &self,
            method: &str,
            args: &Value,
            proxy: &dyn LLMProxy,
        ) -> DispatchResult {
            match method {
                "summarize" => {
                    let text = args
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| DispatchError::HandlerError {
                            cart: "summarize".into(),
                            method: "summarize".into(),
                            message: "args.text missing".into(),
                        })?;
                    let max_bullets = args
                        .get("max_bullets")
                        .and_then(Value::as_u64)
                        .unwrap_or(5);
                    let system = format!(
                        "You compress text into at most {max_bullets} bullet points. \
                         Preserve concrete identifiers, numbers, paths, errors. \
                         Output bullets only — no preamble."
                    );
                    let summary = proxy
                        .chat(&system, text)
                        .map_err(|e| DispatchError::HandlerError {
                            cart: "summarize".into(),
                            method: "summarize".into(),
                            message: format!("LLM proxy: {e}"),
                        })?;
                    Ok(json!({
                        "ok": true,
                        "summary": summary,
                        "input_chars": text.len(),
                        "max_bullets": max_bullets,
                    }))
                }
                other => Err(DispatchError::HandlerError {
                    cart: "summarize".into(),
                    method: other.into(),
                    message: "unknown method".into(),
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stub proxy — returns a deterministic canned summary so tests
    /// don't need network.
    struct StubProxy;
    impl LLMProxy for StubProxy {
        fn chat(&self, _system: &str, user: &str) -> Result<String, String> {
            Ok(format!("- summary of {} chars", user.len()))
        }
    }

    #[test]
    fn summarize_returns_envelope_with_summary() {
        let s = summarize::Summarize;
        let r = s
            .dispatch(
                "summarize",
                &json!({"text": "hello world", "max_bullets": 3}),
                &StubProxy,
            )
            .unwrap();
        assert_eq!(r["ok"], true);
        assert_eq!(r["max_bullets"], 3);
        assert!(r["summary"].as_str().unwrap().contains("summary of"));
    }

    #[test]
    fn unknown_method_errors() {
        let s = summarize::Summarize;
        let err = s.dispatch("not_a_method", &json!({}), &StubProxy).unwrap_err();
        assert!(matches!(err, DispatchError::HandlerError { .. }));
    }

    #[test]
    fn builtin_lookup_finds_summarize() {
        assert!(builtin("summarize").is_some());
        assert!(builtin("nonexistent").is_none());
    }
}
